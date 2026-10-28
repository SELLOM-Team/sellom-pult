//! Token-vault — мастер подключения площадок (Story 1.3, FR-2, AD-16).
//!
//! Чистый домен (AD-1): без serde/sqlx/axum/reqwest. Строковые литералы
//! контракта `/v1` (`"wildberries"`, `"fbo"`, …) в домене не живут — их
//! маппит шлюз явным match с тестами. Домен задаёт:
//!
//! - закрытый список площадок ([`Marketplace`]) и схем ([`Scheme`]) с правилом
//!   «какая схема допустима для какой площадки» (WB: fbo/fbs/realfbs;
//!   Ozon: fbo/fbs/rfbs);
//! - правила валидации токена/Client-Id до пробы и БД (422-ветки шлюза);
//! - модель результата пробы прав ([`ProbeCheck`]) и агрегатора
//!   [`summarize_probes`]: 200 = право подтверждено; 401/403 = отказ с
//!   человекочитаемой подсказкой; таймаут/5xx — честный
//!   [`ConnectionStatus::ProbeUnavailable`], не «подключено»;
//! - статусы подключения ([`ConnectionStatus`]).
//!
//! Расшифрование в домене отсутствует: write-only vault — расшифровка нужна
//! только адаптерам Epic 2 (порт [`crate::ports::TokenCipher`] реализуется
//! шлюзом; decrypt тестируется юнит-тестами, в проде-пути Ф0 не вызывается).

/// Площадка подключения (Ф0: две площадки; ЯМ — позже отдельной сторис).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Marketplace {
    /// Wildberries.
    Wildberries,
    /// Ozon.
    Ozon,
}

/// Схема продажи (чекбоксы мастера, шаг 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Scheme {
    /// FBO — продажи со склада площадки.
    Fbo,
    /// FBS — продажи со склада селлера.
    Fbs,
    /// realFBS — только Wildberries.
    RealFbs,
    /// rFBS — только Ozon.
    Rfbs,
}

/// Статус подключения (значения контракта маппит шлюз; в БД пишется только
/// `Verified` — неуспешный автотест токен не сохраняет).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionStatus {
    /// Права подтверждены пробами, шифротекст записан.
    Verified,
    /// Токен жив, но право не подтвердилось (401/403) — ничего не сохранено.
    Rejected,
    /// Площадка не ответила (таймаут/сеть/5xx) — ничего не сохранено.
    ProbeUnavailable,
}

/// Право-категория: одна проверка пробы (UI рисует таблицу прав).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RightsCheck {
    /// Человекочитаемое имя категории («Статистика», «Цены и скидки», «Товары»).
    pub name: String,
    /// Право подтверждено пробой (200).
    pub ok: bool,
    /// Человекочитаемая подсказка при отказе/недоступности (без стек-трейсов).
    pub hint: Option<String>,
}

/// Учётные данные площадки (шаг 2 мастера). WB — один токен; Ozon — пара
/// Client-Id (не секрет, хранится открыто) + Api-Key (секрет, шифруется).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Credentials {
    /// Токен Wildberries (категории «Статистика» + «Цены и скидки»).
    Wb {
        /// API-токен (секрет).
        token: String,
    },
    /// Пара Ozon.
    Ozon {
        /// Client-Id кабинета (НЕ секрет).
        client_id: String,
        /// Api-Key (секрет).
        api_key: String,
    },
}

impl Credentials {
    /// Секретная часть (шифруется в vault): токен WB или Api-Key Ozon.
    pub fn secret(&self) -> &str {
        match self {
            Credentials::Wb { token } => token,
            Credentials::Ozon { api_key, .. } => api_key,
        }
    }

    /// Открытая часть: Client-Id Ozon; у WB её нет.
    pub fn client_id(&self) -> Option<&str> {
        match self {
            Credentials::Wb { .. } => None,
            Credentials::Ozon { client_id, .. } => Some(client_id),
        }
    }
}

/// Ошибка валидации набора схем (422 `invalid_schemes` на шлюзе).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchemesError {
    /// Список схем пуст — нельзя подключить площадку «вообще без схемы».
    Empty,
    /// Схема не существует у этой площадки (например, realFBS у Ozon).
    NotSupported(Scheme),
}

/// Ошибка валидации токена (422 `invalid_token` на шлюзе).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenError {
    /// Пустой (после trim) токен.
    Empty,
    /// Токен аномально длинный — мусор, а не токен площадки.
    TooLong,
    /// Управляющие символы (переносы строк и пр.) — мусор ввода.
    ControlChars,
}

/// Ошибка валидации Client-Id Ozon (422 `invalid_client_id` на шлюзе).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientIdError {
    /// Пустой (после trim) Client-Id.
    Empty,
    /// Сверхдлинное значение — мусор ввода.
    TooLong,
    /// Управляющие символы.
    ControlChars,
}

/// Жёсткий потолок длины токена: реальные токены WB (JWT-подобные) и ключи
/// Ozon заметно короче; длиннее — краулерский мусор.
const MAX_TOKEN_LEN: usize = 1024;

/// Жёсткий потолок длины Client-Id Ozon (короткий числовой идентификатор).
const MAX_CLIENT_ID_LEN: usize = 128;

impl Marketplace {
    /// Все площадки Ф0 (мастер показывает обе независимо).
    pub const ALL: [Marketplace; 2] = [Marketplace::Wildberries, Marketplace::Ozon];

    /// Существует ли схема у площадки (WB: fbo/fbs/realfbs; Ozon: fbo/fbs/rfbs).
    pub fn supports(self, scheme: Scheme) -> bool {
        match self {
            Marketplace::Wildberries => {
                matches!(scheme, Scheme::Fbo | Scheme::Fbs | Scheme::RealFbs)
            }
            Marketplace::Ozon => matches!(scheme, Scheme::Fbo | Scheme::Fbs | Scheme::Rfbs),
        }
    }
}

/// Валидация набора схем выбранной площадки: непустой список, каждая схема
/// существует у площадки. Дедупликацию и порядок нормализует шлюз.
pub fn validate_schemes(marketplace: Marketplace, schemes: &[Scheme]) -> Result<(), SchemesError> {
    if schemes.is_empty() {
        return Err(SchemesError::Empty);
    }
    for scheme in schemes {
        if !marketplace.supports(*scheme) {
            return Err(SchemesError::NotSupported(*scheme));
        }
    }
    Ok(())
}

/// Валидация токена до пробы и БД: непустой, без управляющих символов,
/// в пределах жёсткого потолка длины. Содержимое не анализируется — формат
/// знает только площадка (проба прав).
pub fn validate_token(token: &str) -> Result<(), TokenError> {
    validate_secret_string(token, MAX_TOKEN_LEN).map_err(|err| match err {
        SecretStringError::Empty => TokenError::Empty,
        SecretStringError::TooLong => TokenError::TooLong,
        SecretStringError::ControlChars => TokenError::ControlChars,
    })
}

/// Валидация Client-Id Ozon до пробы и БД (Client-Id — не секрет, но мусор
/// ввода отвергаем так же честно).
pub fn validate_client_id(client_id: &str) -> Result<(), ClientIdError> {
    validate_secret_string(client_id, MAX_CLIENT_ID_LEN).map_err(|err| match err {
        SecretStringError::Empty => ClientIdError::Empty,
        SecretStringError::TooLong => ClientIdError::TooLong,
        SecretStringError::ControlChars => ClientIdError::ControlChars,
    })
}

/// Внутренняя унификация строковой гигиены секрета.
enum SecretStringError {
    Empty,
    TooLong,
    ControlChars,
}

fn validate_secret_string(value: &str, max_len: usize) -> Result<(), SecretStringError> {
    if value.trim().is_empty() {
        return Err(SecretStringError::Empty);
    }
    if value.chars().count() > max_len {
        return Err(SecretStringError::TooLong);
    }
    if value.chars().any(char::is_control) {
        return Err(SecretStringError::ControlChars);
    }
    Ok(())
}

/// Результат ОДНОЙ пробы права. Классификация HTTP-ответа площадки —
/// на шлюзе (адаптер пробы): 2xx → `Confirmed`; 401/403 → `Denied(hint)`;
/// таймаут/сеть/прочие коды → `Unreachable`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeCheck {
    /// Человекочитаемое имя категории прав (как в [`RightsCheck::name`]).
    pub name: String,
    /// Результат пробы.
    pub result: ProbeResult,
}

/// Результат одной пробы.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProbeResult {
    /// Проба ответила 2xx — право подтверждено.
    Confirmed,
    /// Проба ответила 401/403 — право не подтвердилось; подсказка прилагается.
    Denied(String),
    /// Проба не ответила (таймаут/сеть/5xx/прочее) — honest-ошибка, не отказ.
    Unreachable,
}

/// Человекочитаемая подсказка недоступной площадки (единая для всех проб).
pub const UNREACHABLE_HINT: &str = "площадка не ответила — повторите попытку позже";

/// Итог автотеста прав (шаг 3 мастера).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeSummary {
    /// Агрегированный статус (правила: любая недоступная проба →
    /// `ProbeUnavailable`; иначе любой отказ → `Rejected`; иначе `Verified`).
    pub status: ConnectionStatus,
    /// Список проверок по каждой пробе (для UI-таблицы прав/диагностики).
    pub checks: Vec<RightsCheck>,
}

/// Агрегирует результаты всех проб площадки в честный итог:
///
/// - пустой список проб — не «подключено»: без единой проверки прав нет —
///   честный [`ConnectionStatus::ProbeUnavailable`] (ревью Story 1.3:
///   иначе баг проб-матрицы дал бы Verified с пустыми правами);
/// - хоть одна проба [`ProbeResult::Unreachable`] → [`ConnectionStatus::ProbeUnavailable`]:
///   нельзя заявлять «отказ прав», не увидев ответ площадки целиком, и нельзя
///   «подключать» — инвариант «неуспешная проба не сохраняет»;
/// - иначе хоть один [`ProbeResult::Denied`] → [`ConnectionStatus::Rejected`];
/// - иначе все подтверждены → [`ConnectionStatus::Verified`].
pub fn summarize_probes(checks: &[ProbeCheck]) -> ProbeSummary {
    if checks.is_empty() {
        return ProbeSummary {
            status: ConnectionStatus::ProbeUnavailable,
            checks: Vec::new(),
        };
    }
    let mut out = Vec::with_capacity(checks.len());
    let mut any_unreachable = false;
    let mut any_denied = false;
    for check in checks {
        let (ok, hint) = match &check.result {
            ProbeResult::Confirmed => (true, None),
            ProbeResult::Denied(hint) => {
                any_denied = true;
                (false, Some(hint.clone()))
            }
            ProbeResult::Unreachable => {
                any_unreachable = true;
                (false, Some(UNREACHABLE_HINT.to_owned()))
            }
        };
        out.push(RightsCheck {
            name: check.name.clone(),
            ok,
            hint,
        });
    }
    let status = if any_unreachable {
        ConnectionStatus::ProbeUnavailable
    } else if any_denied {
        ConnectionStatus::Rejected
    } else {
        ConnectionStatus::Verified
    };
    ProbeSummary {
        status,
        checks: out,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marketplace_supports_matrix() {
        // WB: fbo/fbs/realfbs; Ozon: fbo/fbs/rfbs (I/O-матрица контракта).
        let cases = [
            (Marketplace::Wildberries, Scheme::Fbo, true),
            (Marketplace::Wildberries, Scheme::Fbs, true),
            (Marketplace::Wildberries, Scheme::RealFbs, true),
            (Marketplace::Wildberries, Scheme::Rfbs, false),
            (Marketplace::Ozon, Scheme::Fbo, true),
            (Marketplace::Ozon, Scheme::Fbs, true),
            (Marketplace::Ozon, Scheme::Rfbs, true),
            (Marketplace::Ozon, Scheme::RealFbs, false),
        ];
        for (marketplace, scheme, expected) in cases {
            assert_eq!(
                marketplace.supports(scheme),
                expected,
                "матрица {marketplace:?} × {scheme:?}"
            );
        }
        assert_eq!(Marketplace::ALL.len(), 2);
    }

    #[test]
    fn validate_schemes_rules() {
        assert_eq!(
            validate_schemes(Marketplace::Wildberries, &[]),
            Err(SchemesError::Empty)
        );
        assert_eq!(
            validate_schemes(Marketplace::Ozon, &[Scheme::RealFbs]),
            Err(SchemesError::NotSupported(Scheme::RealFbs))
        );
        assert_eq!(
            validate_schemes(Marketplace::Wildberries, &[Scheme::Rfbs]),
            Err(SchemesError::NotSupported(Scheme::Rfbs))
        );
        assert_eq!(
            validate_schemes(Marketplace::Ozon, &[Scheme::Fbo, Scheme::Fbs, Scheme::Rfbs]),
            Ok(())
        );
        assert_eq!(
            validate_schemes(
                Marketplace::Wildberries,
                &[Scheme::Fbo, Scheme::Fbs, Scheme::RealFbs]
            ),
            Ok(())
        );
    }

    #[test]
    fn validate_token_rules() {
        assert_eq!(validate_token(""), Err(TokenError::Empty));
        assert_eq!(validate_token("   "), Err(TokenError::Empty));
        assert_eq!(validate_token(&"x".repeat(1025)), Err(TokenError::TooLong));
        assert_eq!(
            validate_token("bad\nmultiline"),
            Err(TokenError::ControlChars)
        );
        assert_eq!(validate_token("tab\tchar"), Err(TokenError::ControlChars));
        assert_eq!(validate_token(&"x".repeat(1024)), Ok(()));
        assert_eq!(validate_token("eyABC123-_.~"), Ok(()));
    }

    #[test]
    fn validate_client_id_rules() {
        assert_eq!(validate_client_id(""), Err(ClientIdError::Empty));
        assert_eq!(
            validate_client_id(&"9".repeat(129)),
            Err(ClientIdError::TooLong)
        );
        assert_eq!(
            validate_client_id("12\n34"),
            Err(ClientIdError::ControlChars)
        );
        assert_eq!(validate_client_id("12345"), Ok(()));
        assert_eq!(validate_client_id(&"9".repeat(128)), Ok(()));
    }

    #[test]
    fn credentials_split_secret_and_open_parts() {
        let wb = Credentials::Wb {
            token: "wb-token".to_owned(),
        };
        assert_eq!(wb.secret(), "wb-token");
        assert_eq!(wb.client_id(), None);

        let ozon = Credentials::Ozon {
            client_id: "42".to_owned(),
            api_key: "ozon-key".to_owned(),
        };
        assert_eq!(ozon.secret(), "ozon-key");
        assert_eq!(ozon.client_id(), Some("42"));
    }

    #[test]
    fn summarize_empty_checks_is_never_verified() {
        // Пустой прогон проб — не «подключено» (ревью 1.3): нет ни одной
        // проверки прав — честный probe_unavailable.
        let summary = summarize_probes(&[]);
        assert_eq!(summary.status, ConnectionStatus::ProbeUnavailable);
        assert!(summary.checks.is_empty());
    }

    #[test]
    fn summarize_all_confirmed_is_verified() {
        let checks = vec![
            ProbeCheck {
                name: "Статистика".to_owned(),
                result: ProbeResult::Confirmed,
            },
            ProbeCheck {
                name: "Цены и скидки".to_owned(),
                result: ProbeResult::Confirmed,
            },
        ];
        let summary = summarize_probes(&checks);
        assert_eq!(summary.status, ConnectionStatus::Verified);
        assert_eq!(
            summary.checks,
            vec![
                RightsCheck {
                    name: "Статистика".to_owned(),
                    ok: true,
                    hint: None
                },
                RightsCheck {
                    name: "Цены и скидки".to_owned(),
                    ok: true,
                    hint: None
                },
            ]
        );
    }

    #[test]
    fn summarize_denied_is_rejected_with_hints() {
        let checks = vec![
            ProbeCheck {
                name: "Статистика".to_owned(),
                result: ProbeResult::Confirmed,
            },
            ProbeCheck {
                name: "Цены и скидки".to_owned(),
                result: ProbeResult::Denied(
                    "в кабинете WB пересоздайте токен с категорией «Цены и скидки»".to_owned(),
                ),
            },
        ];
        let summary = summarize_probes(&checks);
        assert_eq!(summary.status, ConnectionStatus::Rejected);
        assert!(summary.checks[0].ok);
        assert!(!summary.checks[1].ok);
        assert_eq!(summary.checks[0].hint, None);
        assert!(summary.checks[1].hint.is_some());
    }

    #[test]
    fn summarize_unreachable_overrides_rejected() {
        // Смешанный прогон (один отказ, одна недоступность) — честный
        // probe_unavailable: полной картины прав нет, подключать нельзя.
        let checks = vec![
            ProbeCheck {
                name: "Статистика".to_owned(),
                result: ProbeResult::Denied("отказ".to_owned()),
            },
            ProbeCheck {
                name: "Цены и скидки".to_owned(),
                result: ProbeResult::Unreachable,
            },
        ];
        let summary = summarize_probes(&checks);
        assert_eq!(summary.status, ConnectionStatus::ProbeUnavailable);
        assert_eq!(summary.checks[1].hint.as_deref(), Some(UNREACHABLE_HINT));
    }

    #[test]
    fn summarize_only_unreachable_is_probe_unavailable() {
        let checks = vec![ProbeCheck {
            name: "Товары".to_owned(),
            result: ProbeResult::Unreachable,
        }];
        let summary = summarize_probes(&checks);
        assert_eq!(summary.status, ConnectionStatus::ProbeUnavailable);
        assert!(!summary.checks[0].ok);
    }
}
