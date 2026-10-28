//! Базовый конфиг сервиса, читаемый из окружения.
//!
//! Story 1.1: конфиг минимальный (DATABASE_URL, порт, окружение). Никакой
//! бизнес-логики — только чтение и валидация значений.
//!
//! Story 1.3 (token-vault, AD-16): обязательный мастер-ключ шифрования
//! `PULT_VAULT_MASTER_KEY` (hex, 64 символа = 32 байта) и опциональная
//! test/CI-подмена хоста проб прав `PULT_PROBE_HOST_OVERRIDE`.

use std::env::VarError;

/// Окружение сервиса (метка для логов/метрик; ветвления поведения не задаёт).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Environment {
    /// Локальная разработка (значение по умолчанию).
    Dev,
    /// Промежуточный стенд.
    Stage,
    /// Продакшен.
    Prod,
}

impl Environment {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "dev" => Some(Self::Dev),
            "stage" => Some(Self::Stage),
            "prod" => Some(Self::Prod),
            _ => None,
        }
    }
}

/// Ошибки загрузки конфига.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConfigError {
    /// Обязательная переменная окружения не задана.
    #[error("обязательная переменная окружения {0} не задана")]
    MissingEnv(&'static str),
    /// Мастер-ключ vault не задан — явная ошибка с инструкцией генерации
    /// (Story 1.3, AD-16: шифрование токенов без ключа невозможно).
    #[error(
        "обязательная переменная окружения {0} не задана — сгенерируйте мастер-ключ командой `openssl rand -hex 32` и передайте его hex-значение (64 символа = 32 байта)"
    )]
    MissingVaultKey(&'static str),
    /// Значение переменной окружения невалидно.
    #[error("переменная окружения {0} невалидна: {1}")]
    InvalidEnv(&'static str, String),
}

/// Мастер-ключ шифрования токенов vault (AES-256, AD-16).
///
/// KMS-конверт Ф0: единственный ключ из окружения с фиксированной версией 1;
/// ротация master-ключа (несколько версий) — deferred Ф1+.
/// `Debug` выведен вручную — байты ключа не печатаются (ревью 1.3:
/// derive(Debug) на конфиге протёк бы ключом через любой {:?}/трейсинг).
#[derive(Clone, PartialEq, Eq)]
pub struct VaultMasterKey([u8; 32]);

impl std::fmt::Debug for VaultMasterKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("VaultMasterKey")
            .field(&"***32B-redacted***")
            .finish()
    }
}

impl VaultMasterKey {
    /// Число hex-символов ключа: 64 символа = 32 байта (AES-256).
    pub const HEX_LEN: usize = 64;

    /// Разбирает hex-строку окружения в 32-байтный ключ.
    pub(crate) fn from_hex(raw: &str) -> Result<Self, ConfigError> {
        let hex = raw.trim();
        // Проверка в СИМВОЛАХ и ASCII: многобайтовый ввод (64 байта ≠ 64
        // символа) обязан дать честный InvalidEnv, а не панику индексации
        // пар или тихий нулевой байт (ревью Story 1.3).
        let chars: Vec<char> = hex.chars().collect();
        if chars.len() != Self::HEX_LEN || !hex.is_ascii() {
            return Err(ConfigError::InvalidEnv(
                "PULT_VAULT_MASTER_KEY",
                format!(
                    "ожидались {len} ASCII hex-символов (32 байта), получено {got}",
                    len = Self::HEX_LEN,
                    got = chars.len()
                ),
            ));
        }
        let mut bytes = [0u8; 32];
        for (i, pair) in chars.chunks(2).enumerate() {
            let hi = pair[0].to_digit(16).ok_or_else(|| {
                ConfigError::InvalidEnv(
                    "PULT_VAULT_MASTER_KEY",
                    format!("не hex-символ в позиции {}", i * 2),
                )
            })?;
            let lo = pair[1].to_digit(16).ok_or_else(|| {
                ConfigError::InvalidEnv(
                    "PULT_VAULT_MASTER_KEY",
                    format!("не hex-символ в позиции {}", i * 2 + 1),
                )
            })?;
            bytes[i] = (hi * 16 + lo) as u8;
        }
        Ok(Self(bytes))
    }

    /// Байты ключа (только для шифратора шлюза; в логи не попадают).
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

/// Базовый конфиг сервиса «Пульт селлера».
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceConfig {
    /// Строка подключения к PostgreSQL 18 (обязательная `DATABASE_URL`,
    /// сохраняется trim-нутой).
    pub database_url: String,
    /// Порт HTTP-сервиса (`PULT_HTTP_PORT`, по умолчанию 8080; `0` и
    /// не-Unicode значения — ошибка, а не случайный порт).
    pub http_port: u16,
    /// Окружение (`PULT_ENV`, по умолчанию `dev`).
    pub environment: Environment,
    /// Мастер-ключ vault (`PULT_VAULT_MASTER_KEY`) — обязательна в любом
    /// окружении: шифрование токенов AD-16 без ключа не стартует.
    pub vault_master_key: VaultMasterKey,
    /// Подмена хоста ВСЕХ проб прав (`PULT_PROBE_HOST_OVERRIDE`, например
    /// `http://127.0.0.1:9`) — ТОЛЬКО для CI/тестов: детерминированный
    /// `probe_unavailable` без выхода наружу. В проде не задаётся.
    pub probe_override_host: Option<String>,
}

impl ServiceConfig {
    /// Читает конфиг из переменных окружения.
    ///
    /// - `DATABASE_URL` — обязательна (хранится trim-нутой);
    /// - `PULT_HTTP_PORT` — опциональна, по умолчанию 8080; `0` и
    ///   не-Unicode значения — явная ошибка;
    /// - `PULT_ENV` — опциональна: `dev` (по умолчанию) | `stage` | `prod`;
    /// - `PULT_VAULT_MASTER_KEY` — обязательна (hex, 64 символа = 32 байта);
    ///   отсутствие — явная ошибка с инструкцией генерации;
    /// - `PULT_PROBE_HOST_OVERRIDE` — опциональна, только CI/тесты.
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_lookup(|name| std::env::var(name))
    }

    /// Тот же разбор, но источник значений подменяемый — детерминированные
    /// юнит-тесты всех веток без мутации общего окружения процесса.
    fn from_lookup(
        get_var: impl Fn(&str) -> Result<String, VarError>,
    ) -> Result<Self, ConfigError> {
        let database_url = get_var("DATABASE_URL").map_err(|e| match e {
            VarError::NotPresent => ConfigError::MissingEnv("DATABASE_URL"),
            VarError::NotUnicode(_) => {
                ConfigError::InvalidEnv("DATABASE_URL", "значение не является Unicode".to_owned())
            }
        })?;
        let database_url = database_url.trim().to_owned();
        if database_url.is_empty() {
            return Err(ConfigError::InvalidEnv(
                "DATABASE_URL",
                "пустая строка подключения".to_owned(),
            ));
        }

        let http_port = match get_var("PULT_HTTP_PORT") {
            Ok(raw) => {
                let port = raw
                    .trim()
                    .parse::<u16>()
                    .map_err(|err| ConfigError::InvalidEnv("PULT_HTTP_PORT", err.to_string()))?;
                if port == 0 {
                    return Err(ConfigError::InvalidEnv(
                        "PULT_HTTP_PORT",
                        "0 — случайный порт, укажите конкретный".to_owned(),
                    ));
                }
                port
            }
            Err(VarError::NotPresent) => 8080,
            Err(VarError::NotUnicode(_)) => {
                return Err(ConfigError::InvalidEnv(
                    "PULT_HTTP_PORT",
                    "значение не является Unicode".to_owned(),
                ));
            }
        };

        let environment = match get_var("PULT_ENV") {
            Ok(raw) => Environment::parse(raw.trim())
                .ok_or_else(|| ConfigError::InvalidEnv("PULT_ENV", raw.clone()))?,
            Err(VarError::NotPresent) => Environment::Dev,
            Err(VarError::NotUnicode(_)) => {
                return Err(ConfigError::InvalidEnv(
                    "PULT_ENV",
                    "значение не является Unicode".to_owned(),
                ));
            }
        };

        let vault_master_key = match get_var("PULT_VAULT_MASTER_KEY") {
            Ok(raw) => VaultMasterKey::from_hex(&raw)?,
            Err(VarError::NotPresent) => {
                return Err(ConfigError::MissingVaultKey("PULT_VAULT_MASTER_KEY"));
            }
            Err(VarError::NotUnicode(_)) => {
                return Err(ConfigError::InvalidEnv(
                    "PULT_VAULT_MASTER_KEY",
                    "значение не является Unicode".to_owned(),
                ));
            }
        };

        let probe_override_host = match get_var("PULT_PROBE_HOST_OVERRIDE") {
            Ok(raw) => {
                let host = raw.trim().to_owned();
                if !host.is_empty() {
                    validate_probe_override_host(&host)?;
                    Some(host)
                } else {
                    None
                }
            }
            Err(VarError::NotPresent) => None,
            Err(VarError::NotUnicode(_)) => {
                return Err(ConfigError::InvalidEnv(
                    "PULT_PROBE_HOST_OVERRIDE",
                    "значение не является Unicode".to_owned(),
                ));
            }
        };

        // Подмена хоста проб — ТОЛЬКО инструмент CI/тестов: в prod она
        // означала бы отправку реальных токенов в заголовках на произвольный
        // хост — честный отказ старта (ревью Story 1.3).
        if environment == Environment::Prod && probe_override_host.is_some() {
            return Err(ConfigError::InvalidEnv(
                "PULT_PROBE_HOST_OVERRIDE",
                "подмена хоста проб запрещена в prod (инструмент CI/тестов)".to_owned(),
            ));
        }

        Ok(Self {
            database_url,
            http_port,
            environment,
            vault_master_key,
            probe_override_host,
        })
    }
}

/// Валидация подмены хоста проб: только абсолютный http(s)-origin с хостом
/// (`http://127.0.0.1:9`) — путь пробы остаётся константой матрицы.
fn validate_probe_override_host(host: &str) -> Result<(), ConfigError> {
    let rest = host
        .strip_prefix("https://")
        .or_else(|| host.strip_prefix("http://"))
        .ok_or_else(|| {
            ConfigError::InvalidEnv(
                "PULT_PROBE_HOST_OVERRIDE",
                "ожидался абсолютный origin вида http(s)://host[:port]".to_owned(),
            )
        })?;
    // Только origin: без пути/запроса/фрагмента/userinfo — иначе путь пробы
    // искажается тихо (host/base/api/v1/...) (формальное ревью 1.3).
    if rest.is_empty()
        || rest.chars().any(char::is_whitespace)
        || rest.contains(['/', '?', '#', '@'])
    {
        return Err(ConfigError::InvalidEnv(
            "PULT_PROBE_HOST_OVERRIDE",
            "ожидался чистый origin http(s)://host[:port] — без пути, запроса и userinfo"
                .to_owned(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    /// Фиксированный тестовый ключ (64 hex-символа = 32 байта) — только тесты.
    const TEST_KEY: &str = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";

    /// Фейковый источник переменных: без гонок за общее окружение процесса.
    ///
    /// Мастер-ключ vault подмешивается по умолчанию (обязателен с Story 1.3):
    /// тесты веток самого ключа передают его явно в map либо используют
    /// собственное замыкание без подмешивания.
    fn lookup<'a>(map: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Result<String, VarError> + 'a {
        move |name: &str| {
            if name == "PULT_VAULT_MASTER_KEY" && !map.iter().any(|(key, _)| *key == name) {
                return Ok(TEST_KEY.to_owned());
            }
            map.iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| (*value).to_owned())
                .ok_or(VarError::NotPresent)
        }
    }

    #[test]
    fn environment_labels_parse() {
        assert_eq!(Environment::parse("dev"), Some(Environment::Dev));
        assert_eq!(Environment::parse("stage"), Some(Environment::Stage));
        assert_eq!(Environment::parse("prod"), Some(Environment::Prod));
        assert_eq!(Environment::parse("production"), None);
    }

    #[test]
    fn defaults_with_only_database_url() {
        let cfg =
            ServiceConfig::from_lookup(lookup(&[("DATABASE_URL", "postgres://pult:pult@db/pult")]))
                .expect("конфиг валиден");
        assert_eq!(cfg.http_port, 8080);
        assert_eq!(cfg.environment, Environment::Dev);
    }

    #[test]
    fn missing_database_url_is_error() {
        let err = ServiceConfig::from_lookup(lookup(&[])).expect_err("DATABASE_URL обязательна");
        assert_eq!(err, ConfigError::MissingEnv("DATABASE_URL"));
    }

    #[test]
    fn blank_database_url_is_error() {
        let err = ServiceConfig::from_lookup(lookup(&[("DATABASE_URL", "   ")]))
            .expect_err("пустая строка подключения невалидна");
        assert_eq!(
            err,
            ConfigError::InvalidEnv("DATABASE_URL", "пустая строка подключения".to_owned())
        );
    }

    #[test]
    fn database_url_is_trimmed() {
        let cfg = ServiceConfig::from_lookup(lookup(&[(
            "DATABASE_URL",
            "  postgres://pult:pult@db/pult \t",
        )]))
        .expect("конфиг валиден");
        assert_eq!(cfg.database_url, "postgres://pult:pult@db/pult");
    }

    #[test]
    fn explicit_port_and_env_are_accepted() {
        let cfg = ServiceConfig::from_lookup(lookup(&[
            ("DATABASE_URL", "postgres://pult:pult@db/pult"),
            ("PULT_HTTP_PORT", "9090"),
            ("PULT_ENV", "prod"),
        ]))
        .expect("конфиг валиден");
        assert_eq!(cfg.http_port, 9090);
        assert_eq!(cfg.environment, Environment::Prod);
    }

    #[test]
    fn invalid_port_is_error() {
        let err = ServiceConfig::from_lookup(lookup(&[
            ("DATABASE_URL", "postgres://pult:pult@db/pult"),
            ("PULT_HTTP_PORT", "abc"),
        ]))
        .expect_err("порт невалиден");
        assert!(matches!(err, ConfigError::InvalidEnv("PULT_HTTP_PORT", _)));
    }

    #[test]
    fn zero_port_is_rejected() {
        let err = ServiceConfig::from_lookup(lookup(&[
            ("DATABASE_URL", "postgres://pult:pult@db/pult"),
            ("PULT_HTTP_PORT", "0"),
        ]))
        .expect_err("порт 0 даёт случайный порт — должен отклоняться");
        assert!(matches!(err, ConfigError::InvalidEnv("PULT_HTTP_PORT", _)));
    }

    #[test]
    fn not_unicode_port_is_error() {
        let err = ServiceConfig::from_lookup(|name| match name {
            "DATABASE_URL" => Ok("postgres://pult:pult@db/pult".to_owned()),
            "PULT_HTTP_PORT" => Err(VarError::NotUnicode(OsString::from("8080"))),
            _ => Err(VarError::NotPresent),
        })
        .expect_err("не-Unicode порт — явная ошибка, а не молчаливый дефолт");
        assert!(matches!(err, ConfigError::InvalidEnv("PULT_HTTP_PORT", _)));
    }

    #[test]
    fn invalid_environment_is_error() {
        let err = ServiceConfig::from_lookup(lookup(&[
            ("DATABASE_URL", "postgres://pult:pult@db/pult"),
            ("PULT_ENV", "production"),
        ]))
        .expect_err("окружение невалидно");
        assert_eq!(
            err,
            ConfigError::InvalidEnv("PULT_ENV", "production".to_owned())
        );
    }

    #[test]
    fn not_unicode_database_url_is_invalid_env_not_missing() {
        // NotUnicode обязан отличаться от «не задана»: переменная есть, но её
        // значение не читается — misleading-диагностика недопустима.
        let err = ServiceConfig::from_lookup(|name| match name {
            "DATABASE_URL" => Err(VarError::NotUnicode(OsString::from("pg://…"))),
            _ => Err(VarError::NotPresent),
        })
        .expect_err("не-Unicode DATABASE_URL — явная ошибка значения");
        assert!(matches!(err, ConfigError::InvalidEnv("DATABASE_URL", _)));
    }

    #[test]
    fn port_and_env_are_trimmed() {
        // Нормализация единая для всех переменных (DATABASE_URL уже тримится).
        let cfg = ServiceConfig::from_lookup(lookup(&[
            ("DATABASE_URL", "postgres://pult:pult@db/pult"),
            ("PULT_HTTP_PORT", " 9090 "),
            ("PULT_ENV", "prod "),
        ]))
        .expect("пробельные обводки значений не меняют семантику");
        assert_eq!(cfg.http_port, 9090);
        assert_eq!(cfg.environment, Environment::Prod);
    }

    // --- Story 1.3: PULT_VAULT_MASTER_KEY (обязательный мастер-ключ) ---

    #[test]
    fn vault_key_parses_hex_into_32_bytes() {
        let cfg = ServiceConfig::from_lookup(lookup(&[
            ("DATABASE_URL", "postgres://pult:pult@db/pult"),
            ("PULT_VAULT_MASTER_KEY", TEST_KEY),
        ]))
        .expect("валидный hex-ключ принят");
        assert_eq!(cfg.vault_master_key.as_bytes().len(), 32);
        assert_eq!(cfg.vault_master_key.as_bytes()[0], 0x00);
        assert_eq!(cfg.vault_master_key.as_bytes()[31], 0x1f);
    }

    #[test]
    fn vault_key_is_trimmed_and_uppercase_hex_accepted() {
        let cfg = ServiceConfig::from_lookup(lookup(&[
            ("DATABASE_URL", "postgres://pult:pult@db/pult"),
            ("PULT_VAULT_MASTER_KEY", TEST_KEY.to_uppercase().as_str()),
        ]))
        .expect("верхний регистр hex и обводка пробелами валидны");
        assert_eq!(cfg.vault_master_key.as_bytes()[0], 0x00);
    }

    #[test]
    fn missing_vault_key_is_error_with_generation_instruction() {
        // Отсутствие ключа в ЛЮБОМ окружении (включая dev) — явная ошибка
        // старта с инструкцией генерации (спека 1.3).
        let err = ServiceConfig::from_lookup(|name| match name {
            "DATABASE_URL" => Ok("postgres://pult:pult@db/pult".to_owned()),
            _ => Err(VarError::NotPresent),
        })
        .expect_err("без PULT_VAULT_MASTER_KEY старт запрещён");
        assert_eq!(err, ConfigError::MissingVaultKey("PULT_VAULT_MASTER_KEY"));
        let rendered = err.to_string();
        assert!(
            rendered.contains("openssl rand -hex 32"),
            "инструкция генерации обязана быть в сообщении: {rendered}"
        );
    }

    #[test]
    fn vault_key_wrong_length_is_invalid_env() {
        for bad in ["", "00", "a".repeat(63).as_str(), "a".repeat(65).as_str()] {
            let err = ServiceConfig::from_lookup(lookup(&[
                ("DATABASE_URL", "postgres://pult:pult@db/pult"),
                ("PULT_VAULT_MASTER_KEY", bad),
            ]))
            .expect_err("длина hex ≠ 64 отклоняется");
            assert!(
                matches!(err, ConfigError::InvalidEnv("PULT_VAULT_MASTER_KEY", _)),
                "ключ {bad:?} должен быть InvalidEnv"
            );
        }
    }

    #[test]
    fn vault_key_multibyte_is_invalid_env_not_panic() {
        // 64 БАЙТА многобайтовой строки = меньше 64 символов — честный
        // InvalidEnv без паники индексации пар (ревью 1.3).
        let multibyte = "к".repeat(20) + &"a".repeat(30); // 50 симв., 70 байт
        for bad in [multibyte.as_str(), &"к".repeat(32)] {
            let err = ServiceConfig::from_lookup(lookup(&[
                ("DATABASE_URL", "postgres://pult:pult@db/pult"),
                ("PULT_VAULT_MASTER_KEY", bad),
            ]))
            .expect_err("не-ASCII hex отклоняется");
            assert!(matches!(
                err,
                ConfigError::InvalidEnv("PULT_VAULT_MASTER_KEY", _)
            ));
        }
        // 62 ASCII-символа не дают тихий нулевой байт — тоже InvalidEnv.
        let err = ServiceConfig::from_lookup(lookup(&[
            ("DATABASE_URL", "postgres://pult:pult@db/pult"),
            ("PULT_VAULT_MASTER_KEY", &"a".repeat(62)),
        ]))
        .expect_err("62 символа отклоняются");
        assert!(matches!(
            err,
            ConfigError::InvalidEnv("PULT_VAULT_MASTER_KEY", _)
        ));
    }

    #[test]
    fn debug_output_never_contains_key_bytes() {
        // Ревью 1.3: {:?} конфига и MasterKey не печатает байты ключа.
        let key = VaultMasterKey::from_hex(&"ab".repeat(32)).expect("валидный ключ");
        let debug = format!("{key:?}");
        assert!(!debug.contains("abab"), "Debug протёк байтами: {debug}");
        assert!(
            debug.contains("redacted"),
            "Debug обязан маскировать: {debug}"
        );
    }

    #[test]
    fn probe_override_rejects_non_origins() {
        // Ревью 1.3: путь/запрос/userinfo тихо искажали бы URL проб.
        for bad in [
            "http://host/base",
            "http://host?x=1",
            "http://user:pass@host",
            "http://host/#frag",
        ] {
            let err = ServiceConfig::from_lookup(lookup(&[
                ("DATABASE_URL", "postgres://pult:pult@db/pult"),
                ("PULT_VAULT_MASTER_KEY", TEST_KEY),
                ("PULT_PROBE_HOST_OVERRIDE", bad),
            ]))
            .expect_err("не-origin отклоняется");
            assert!(matches!(
                err,
                ConfigError::InvalidEnv("PULT_PROBE_HOST_OVERRIDE", _)
            ));
        }
    }

    #[test]
    fn probe_override_rejected_in_prod() {
        // Подмена хоста проб — инструмент CI/тестов: в prod реальный токен
        // ушёл бы в заголовках на произвольный хост (ревью 1.3).
        let err = ServiceConfig::from_lookup(lookup(&[
            ("DATABASE_URL", "postgres://pult:pult@db/pult"),
            (
                "PULT_VAULT_MASTER_KEY",
                "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f",
            ),
            ("PULT_ENV", "prod"),
            ("PULT_PROBE_HOST_OVERRIDE", "http://127.0.0.1:9"),
        ]))
        .expect_err("override в prod — отказ старта");
        assert!(matches!(
            err,
            ConfigError::InvalidEnv("PULT_PROBE_HOST_OVERRIDE", _)
        ));

        // В dev/stage — допустим (CI-механика probe_unavailable).
        for env in ["dev", "stage"] {
            let ok = ServiceConfig::from_lookup(lookup(&[
                ("DATABASE_URL", "postgres://pult:pult@db/pult"),
                (
                    "PULT_VAULT_MASTER_KEY",
                    "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f",
                ),
                ("PULT_ENV", env),
                ("PULT_PROBE_HOST_OVERRIDE", "http://127.0.0.1:9"),
            ]))
            .unwrap_or_else(|err| panic!("{env} + override должен быть валиден: {err}"));
            assert_eq!(
                ok.probe_override_host.as_deref(),
                Some("http://127.0.0.1:9")
            );
        }
    }

    #[test]
    fn vault_key_non_hex_is_invalid_env() {
        let err = ServiceConfig::from_lookup(lookup(&[
            ("DATABASE_URL", "postgres://pult:pult@db/pult"),
            (
                "PULT_VAULT_MASTER_KEY",
                "zz1102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f",
            ),
        ]))
        .expect_err("не-hex символы отклоняются");
        assert!(matches!(
            err,
            ConfigError::InvalidEnv("PULT_VAULT_MASTER_KEY", _)
        ));
    }

    #[test]
    fn not_unicode_vault_key_is_invalid_env() {
        let err = ServiceConfig::from_lookup(|name| match name {
            "DATABASE_URL" => Ok("postgres://pult:pult@db/pult".to_owned()),
            "PULT_VAULT_MASTER_KEY" => Err(VarError::NotUnicode(OsString::from("ключ"))),
            _ => Err(VarError::NotPresent),
        })
        .expect_err("не-Unicode ключ — явная ошибка значения");
        assert!(matches!(
            err,
            ConfigError::InvalidEnv("PULT_VAULT_MASTER_KEY", _)
        ));
    }

    // --- Story 1.3: PULT_PROBE_HOST_OVERRIDE (только CI/тесты) ---

    #[test]
    fn probe_override_host_parses_when_set() {
        let cfg = ServiceConfig::from_lookup(lookup(&[
            ("DATABASE_URL", "postgres://pult:pult@db/pult"),
            ("PULT_PROBE_HOST_OVERRIDE", " http://127.0.0.1:9 "),
        ]))
        .expect("валидный override принят");
        assert_eq!(
            cfg.probe_override_host.as_deref(),
            Some("http://127.0.0.1:9")
        );

        let none =
            ServiceConfig::from_lookup(lookup(&[("DATABASE_URL", "postgres://pult:pult@db/pult")]))
                .expect("без переменной — None");
        assert_eq!(none.probe_override_host, None);
    }

    #[test]
    fn probe_override_host_rejects_non_origin_values() {
        for bad in [
            "127.0.0.1:9",        // без схемы
            "http://",            // схема без хоста
            "https://",           // схема без хоста
            "ftp://127.0.0.1",    // чужая схема
            "http://127.0.0.1 x", // пробел внутри
            "",                   // пустая — трактуется как «не задана», не ошибка
        ] {
            let result = ServiceConfig::from_lookup(lookup(&[
                ("DATABASE_URL", "postgres://pult:pult@db/pult"),
                ("PULT_PROBE_HOST_OVERRIDE", bad),
            ]));
            if bad.is_empty() {
                assert!(
                    result
                        .expect("пустая override — «не задана»")
                        .probe_override_host
                        .is_none(),
                    "пустая строка ≠ ошибка"
                );
            } else {
                assert!(
                    matches!(
                        result,
                        Err(ConfigError::InvalidEnv("PULT_PROBE_HOST_OVERRIDE", _))
                    ),
                    "значение {bad:?} должно быть отклонено"
                );
            }
        }
    }
}
