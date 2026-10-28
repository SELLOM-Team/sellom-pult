//! Шаг 0 онбординга — фильтр периметра «ваш бизнес» (Story 1.2, FR-1).
//!
//! Маршруты `/v1` (AD-13 — вход только через шлюз; ошибки — конверт
//! `{error:{code,message,request_id,details}}`):
//! - `GET /v1/onboarding/state` — upsert организации и пользователя по
//!   placement-контексту Б24 и состояние шага 0;
//! - `POST /v1/perimeter/verdict` — ответы вопросника → иммутабельный
//!   per-user вердикт (повторный POST возвращает сохранённый без перезаписи);
//! - `GET /v1/metrics/perimeter` — доля установок, прошедших шаг 0
//!   (агрегат без PII, без dev-портала).
//!
//! Роли: правила вердикта — чистый домен `core/identity::perimeter`
//! (`decide_verdict`); этот модуль — композиция: sqlx-стор, реализующий порт
//! [`PerimeterStore`], и явный маппинг строковых литералов API ↔ enum.
//! Идентичность — только из placement-контекста Б24 (query: DOMAIN/USER_ID/
//! PLACEMENT); реальная проверка подписи/хендшейка — Story 1.3. Без
//! placement-параметров работает документированный dev-фоллбэк
//! `dev-portal`/`dev-user`, исключаемый из метрик.

use std::collections::HashMap;
use std::future::Future;
use std::time::Duration;

use axum::Json;
use axum::Router;
use axum::extract::rejection::{JsonRejection, QueryRejection};
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tracing::{debug, info, warn};
use uuid::Uuid;

use pult_core_identity::perimeter::{PerimeterAnswers, TradeModel, Verdict, decide_verdict};
use pult_core_identity::ports::PerimeterStore;
use pult_platform::error::ErrorEnvelope;

use crate::AppState;

/// Бюджет ОДНОГО обращения к БД в /v1-хендлерах (паттерн /health: запрос,
/// не уложившийся в бюджет, — «БД недоступна», а не зависший запрос).
/// Хендлеры цепляют до трёх последовательных операций (ensure_organization →
/// ensure_user → load/save): 3 × 2с = 6с < 8с клиентского бюджета
/// REQUEST_TIMEOUT_MS (SPA api.ts) — иначе клиент ушёл бы в офлайн раньше,
/// чем шлюз честно ответил деградацией (формальное ревью Story 1.2).
const STORE_BUDGET: Duration = Duration::from_secs(2);

/// Закрытый список площадок шага 0 (контракт Ф0: WB/Ozon/ЯМ) — свободный
/// текст в jsonb не попадает (ревью Story 1.2: валидация закрытым списком).
const ALLOWED_MARKETPLACES: [&str; 3] = ["wildberries", "ozon", "yandex_market"];

/// member_id dev-фоллбэка: организация-фикстура, исключается из метрик
/// (не «установка», а документированный dev-режим без placement-параметров).
const DEV_MEMBER_ID: &str = "dev-portal";
/// external_ref dev-фоллбэка пользователя.
const DEV_USER_REF: &str = "dev-user";

/// Идентичность из placement-контекста Б24: member_id (DOMAIN) → ключ
/// реестра установок, user_ref (USER_ID) → внешний ref пользователя.
///
/// Полное отсутствие placement-параметров — документированный dev-фоллбэк.
/// Частичный контекст (есть DOMAIN без USER_ID или наоборот) пользователя
/// не идентифицирует, но и dev-режимом быть не должен: реальный портал с
/// потерянным параметром не может молча слиться с фикстурой — честный отказ
/// 422 `invalid_placement` (формальное ревью Story 1.2). Сверхдлинные
/// значения тоже отвергаются: краулерский мусор не становится постоянной
/// строкой реестра установок. Никакой собственной аутентификации (NFR-6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PlacementIdentity {
    /// Ключ реестра установок (organizations.b24_member_id).
    pub member_id: String,
    /// Внешний ref пользователя (users.external_ref).
    pub user_ref: String,
}

/// Ошибка распознавания placement-контекста.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PlacementIdentityError {
    /// DOMAIN коллизирует с зарезервированным dev-идентификатором —
    /// реальный портал не должен молча сливаться с dev-фикстурой.
    ReservedDomain,
    /// Частичный контекст (есть DOMAIN без USER_ID или наоборот) — не
    /// идентифицирует пользователя; тихая запись реального портала в
    /// dev-фикстуру запрещена.
    PartialContext,
    /// Значение DOMAIN/USER_ID слишком длинное (краулер/мусор в query).
    Oversized,
}

impl PlacementIdentity {
    /// Читает placement-контекст из query-параметров запроса.
    ///
    /// DOMAIN канонизируется (нижний регистр + хвостовая точка FQDN —
    /// варианты одного портала не должны плодить дубликаты организаций в
    /// реестре установок); частичный контекст и сверхдлинные значения —
    /// отказ (422 `invalid_placement`); литерал dev-портала в явном
    /// DOMAIN — отказ (422 `reserved_member_id`).
    pub(crate) fn from_params(
        params: &HashMap<String, String>,
    ) -> Result<Self, PlacementIdentityError> {
        let domain = params.get("DOMAIN").map(|v| v.trim()).unwrap_or_default();
        let user = params.get("USER_ID").map(|v| v.trim()).unwrap_or_default();
        let placement = params
            .get("PLACEMENT")
            .map(|v| v.trim())
            .unwrap_or_default();
        if domain.is_empty() && user.is_empty() {
            debug!(
                placement,
                "placement-контекст отсутствует — dev-фоллбэк (в метрики не попадёт)"
            );
            return Ok(Self {
                member_id: DEV_MEMBER_ID.to_owned(),
                user_ref: DEV_USER_REF.to_owned(),
            });
        }
        if domain.is_empty() || user.is_empty() {
            debug!(
                placement,
                "placement-контекст частичный (DOMAIN без USER_ID или наоборот) — отказ"
            );
            return Err(PlacementIdentityError::PartialContext);
        }
        // Б24-домен — DNS-имя (≤ 253), USER_ID — короткий внешний ref;
        // длинные значения — мусор краулеров, не идентичность (write-on-GET
        // публичен до хендшейка 1.3 — не плодим постоянные строки-гиганты).
        if domain.len() > 253 || user.len() > 64 {
            return Err(PlacementIdentityError::Oversized);
        }
        // Канонизация: регистр + хвостовая точка FQDN — варианты одного
        // портала не должны плодить дубликаты организаций (знаменатель UJ-1).
        let member_id = domain.to_lowercase();
        let member_id = member_id.trim_end_matches('.');
        if member_id == DEV_MEMBER_ID {
            return Err(PlacementIdentityError::ReservedDomain);
        }
        debug!(domain = %member_id, user, placement, "placement-контекст Б24 распознан");
        Ok(Self {
            member_id: member_id.to_owned(),
            user_ref: user.to_owned(),
        })
    }

    /// true — идентичность подставлена dev-фоллбэком.
    pub(crate) fn is_dev_fallback(&self) -> bool {
        self.member_id == DEV_MEMBER_ID
    }
}

/// Ошибки sqlx-стора вердиктов.
#[derive(Debug)]
pub(crate) enum StoreError {
    /// Ошибка БД (недоступна/отказ запроса) → 503 `database_unavailable`.
    Database(sqlx::Error),
    /// `verdict` в БД вне {'seller','non_seller'} — повреждение данных,
    /// которое CHECK-ограничение обязано исключать → 500 `internal_error`.
    CorruptVerdict(String),
}

/// sqlx-стор шага 0: реализует порт [`PerimeterStore`] домена identity
/// (AD-1) и держит композиционный upsert организации/пользователя по
/// placement-контексту. SQL — рантайм-запросы (паттерн пробы /health;
/// compile-time — когда PG появится в dev-цикле).
#[derive(Clone)]
pub(crate) struct PgPerimeterStore {
    db: PgPool,
}

impl PgPerimeterStore {
    pub(crate) fn new(db: PgPool) -> Self {
        Self { db }
    }

    /// Идемпотентный upsert организации по ключу реестра установок
    /// `b24_member_id` (UNIQUE). Имя организации — домен портала (до
    /// хендшейка 1.3 реальных названий нет; PII не собираем).
    pub(crate) async fn ensure_organization(&self, member_id: &str) -> Result<Uuid, sqlx::Error> {
        let inserted: Option<(Uuid,)> = sqlx::query_as(
            "INSERT INTO organizations (id, name, b24_member_id) \
                 VALUES ($1, $2, $3) \
                 ON CONFLICT (b24_member_id) DO NOTHING \
                 RETURNING id",
        )
        .bind(Uuid::now_v7())
        .bind(member_id)
        .bind(member_id)
        .fetch_optional(&self.db)
        .await?;
        if let Some((id,)) = inserted {
            return Ok(id);
        }
        let (existing,): (Uuid,) =
            sqlx::query_as("SELECT id FROM organizations WHERE b24_member_id = $1")
                .bind(member_id)
                .fetch_one(&self.db)
                .await?;
        Ok(existing)
    }

    /// Идемпотентный upsert пользователя по (organization_id, external_ref).
    /// display_name = «Б24 user {external_ref}» — имя не собираем (шаг 0 без
    /// PII); реальные имена — после хендшейка 1.3 под PII-периметром AD-16.
    async fn ensure_user(
        &self,
        organization_id: Uuid,
        external_ref: &str,
    ) -> Result<Uuid, sqlx::Error> {
        let display_name = format!("Б24 user {external_ref}");
        let inserted: Option<(Uuid,)> = sqlx::query_as(
            "INSERT INTO users (id, organization_id, display_name, external_ref) \
                 VALUES ($1, $2, $3, $4) \
                 ON CONFLICT (organization_id, external_ref) DO NOTHING \
                 RETURNING id",
        )
        .bind(Uuid::now_v7())
        .bind(organization_id)
        .bind(display_name)
        .bind(external_ref)
        .fetch_optional(&self.db)
        .await?;
        if let Some((id,)) = inserted {
            return Ok(id);
        }
        let (existing,): (Uuid,) =
            sqlx::query_as("SELECT id FROM users WHERE organization_id = $1 AND external_ref = $2")
                .bind(organization_id)
                .bind(external_ref)
                .fetch_one(&self.db)
                .await?;
        Ok(existing)
    }
    /// Метрика шага 0 — детерминированный SQL-агрегат по таблице вердиктов
    /// (событийный конвейер AD-22 появится в 1.4+). Исключаются: dev-портал
    /// (`b24_member_id = $1` — фикстура, не установка; параметр биндится
    /// константой DEV_MEMBER_ID — единственный источник литерала) и строки
    /// без member_id (не созданы placement-потоком). Установка «прошла шаг 0»
    /// = в организации есть хотя бы один вердикт (шаг запоминается per-user;
    /// знаменатель активации UJ-1 — org-уровень).
    pub(crate) async fn perimeter_metrics(&self) -> Result<(i64, i64, i64), sqlx::Error> {
        sqlx::query_as(
            "SELECT \
                (SELECT count(*) FROM organizations o \
                 WHERE o.b24_member_id IS NOT NULL AND o.b24_member_id <> $1) AS installs, \
                (SELECT count(DISTINCT v.organization_id) FROM perimeter_verdicts v \
                 JOIN organizations o ON o.id = v.organization_id \
                 WHERE o.b24_member_id IS NOT NULL AND o.b24_member_id <> $1) AS step0_completed, \
                (SELECT count(DISTINCT v.organization_id) FROM perimeter_verdicts v \
                 JOIN organizations o ON o.id = v.organization_id \
                 WHERE o.b24_member_id IS NOT NULL AND o.b24_member_id <> $1 \
                   AND v.verdict = 'seller') AS sellers",
        )
        .bind(DEV_MEMBER_ID)
        .fetch_one(&self.db)
        .await
    }
}

// --- Бюджет обращений к БД (ревью Story 1.2: зависший запрос — тоже
// деградация; паттерн /health распространён на все /v1-хендлеры) ---

/// Оборачивает sqlx-футуру в бюджет {STORE_BUDGET:?}; просрочка = 503-конверт.
async fn budgeted_sqlx<T>(
    fut: impl Future<Output = Result<T, sqlx::Error>>,
) -> Result<T, (StatusCode, Json<ErrorEnvelope>)> {
    match tokio::time::timeout(STORE_BUDGET, fut).await {
        Ok(res) => res.map_err(|err| {
            warn!(?err, "store шага 0: ошибка БД");
            db_unavailable()
        }),
        Err(_) => {
            warn!("store шага 0: превышен бюджет {STORE_BUDGET:?} — БД не отвечает");
            Err(db_unavailable())
        }
    }
}

/// То же для вызовов стора с доменной ошибкой StoreError.
async fn budgeted_store<T>(
    fut: impl Future<Output = Result<T, StoreError>>,
) -> Result<T, (StatusCode, Json<ErrorEnvelope>)> {
    match tokio::time::timeout(STORE_BUDGET, fut).await {
        Ok(res) => res.map_err(store_error_response),
        Err(_) => {
            warn!("store шага 0: превышен бюджет {STORE_BUDGET:?} — БД не отвечает");
            Err(db_unavailable())
        }
    }
}

impl PerimeterStore for PgPerimeterStore {
    type Error = StoreError;

    async fn load_verdict(&self, user_id: Uuid) -> Result<Option<Verdict>, StoreError> {
        let row: Option<(String,)> =
            sqlx::query_as("SELECT verdict FROM perimeter_verdicts WHERE user_id = $1")
                .bind(user_id)
                .fetch_optional(&self.db)
                .await
                .map_err(StoreError::Database)?;
        match row {
            None => Ok(None),
            Some((raw,)) => match verdict_from_str(&raw) {
                Some(verdict) => Ok(Some(verdict)),
                None => Err(StoreError::CorruptVerdict(raw)),
            },
        }
    }

    async fn save_verdict(
        &self,
        organization_id: Uuid,
        user_id: Uuid,
        answers: &PerimeterAnswers,
        verdict: Verdict,
    ) -> Result<Verdict, StoreError> {
        // Идемпотентность (Design Notes 1.2): ON CONFLICT (user_id) DO NOTHING
        // + дочитать существующую строку — вердикт иммутабелен, гонки двух
        // параллельных POST дают один результат.
        let inserted: Option<(String,)> = sqlx::query_as(
            "INSERT INTO perimeter_verdicts \
                (id, organization_id, user_id, trade_model, marketplaces, verdict) \
             VALUES ($1, $2, $3, $4, $5, $6) \
             ON CONFLICT (user_id) DO NOTHING \
             RETURNING verdict",
        )
        .bind(Uuid::now_v7())
        .bind(organization_id)
        .bind(user_id)
        .bind(trade_model_as_str(&answers.trade_model))
        .bind(serde_json::json!(answers.marketplaces))
        .bind(verdict_as_str(&verdict))
        .fetch_optional(&self.db)
        .await
        .map_err(StoreError::Database)?;
        let raw = match inserted {
            Some((raw,)) => raw,
            None => {
                info!("вердикт уже сохранён — возвращён существующий (идемпотентность per-user)");
                let (existing,): (String,) =
                    sqlx::query_as("SELECT verdict FROM perimeter_verdicts WHERE user_id = $1")
                        .bind(user_id)
                        .fetch_one(&self.db)
                        .await
                        .map_err(StoreError::Database)?;
                existing
            }
        };
        match verdict_from_str(&raw) {
            Some(verdict) => Ok(verdict),
            None => Err(StoreError::CorruptVerdict(raw)),
        }
    }
}

// --- Явный маппинг строковых литералов контракта /v1 ↔ enum домена (AD-1:
// литералы живут в шлюзе, не в чистом домене). Покрыт тестами ниже. ---

/// Строковый литерал торговой модели → enum домена; `None` для неизвестных.
fn trade_model_from_str(raw: &str) -> Option<TradeModel> {
    match raw {
        "marketplace_seller" => Some(TradeModel::MarketplaceSeller),
        "marketplace_planned" => Some(TradeModel::MarketplacePlanned),
        "own_channels_no_marketplaces" => Some(TradeModel::OwnChannelsNoMarketplaces),
        "services_other" => Some(TradeModel::ServicesOther),
        _ => None,
    }
}

/// Enum домена → строковый литерал контракта.
fn trade_model_as_str(model: &TradeModel) -> &'static str {
    match model {
        TradeModel::MarketplaceSeller => "marketplace_seller",
        TradeModel::MarketplacePlanned => "marketplace_planned",
        TradeModel::OwnChannelsNoMarketplaces => "own_channels_no_marketplaces",
        TradeModel::ServicesOther => "services_other",
    }
}

/// Строковый литерал вердикта → enum; `None` для повреждённых значений.
fn verdict_from_str(raw: &str) -> Option<Verdict> {
    match raw {
        "seller" => Some(Verdict::Seller),
        "non_seller" => Some(Verdict::NonSeller),
        _ => None,
    }
}

/// Enum вердикта → строковый литерал контракта.
fn verdict_as_str(verdict: &Verdict) -> &'static str {
    match verdict {
        Verdict::Seller => "seller",
        Verdict::NonSeller => "non_seller",
    }
}

// --- DTO контрактов /v1 ---

/// Тело POST /v1/perimeter/verdict.
#[derive(Debug, Deserialize)]
struct VerdictRequestBody {
    /// Торговая модель (один из 4 литералов); отсутствие — 422.
    trade_model: Option<String>,
    /// Площадки (для селлерских моделей обязательны непустым списком).
    #[serde(default)]
    marketplaces: Vec<String>,
}

/// Ответ GET /v1/onboarding/state.
#[derive(Debug, Serialize)]
struct OnboardingStateResponse {
    state: &'static str,
}

/// Ответ POST /v1/perimeter/verdict.
#[derive(Debug, Serialize)]
struct VerdictResponse {
    verdict: &'static str,
    state: &'static str,
}

/// Ответ GET /v1/metrics/perimeter.
#[derive(Debug, Serialize)]
struct PerimeterMetricsResponse {
    installs: i64,
    step0_completed: i64,
    sellers: i64,
    step0_share: f64,
}

/// Ошибки валидации ответов шага 0 → 422 error-конверт.
#[derive(Debug, PartialEq, Eq)]
enum VerdictValidationError {
    /// trade_model отсутствует или не входит в 4 литерала домена.
    InvalidTradeModel(String),
    /// Селлерская модель без площадок.
    MarketplacesRequired,
    /// Пустая (после trim) метка площадки в списке.
    InvalidMarketplaces,
    /// Площадка вне закрытого списка Ф0 (wildberries/ozon/yandex_market).
    UnknownMarketplace,
}

/// Валидация тела запроса → ответы домена: строковая гигиена (trim,
/// закрытый список площадок, дедупликация) — здесь; бизнес-инвариант
/// «селлер ⇒ площадки» — в [`PerimeterAnswers::new`] домена.
fn answers_from_request(
    body: VerdictRequestBody,
) -> Result<PerimeterAnswers, VerdictValidationError> {
    let raw_model = body.trade_model.unwrap_or_default();
    let trade_model = trade_model_from_str(raw_model.trim())
        .ok_or_else(|| VerdictValidationError::InvalidTradeModel(raw_model.clone()))?;
    let mut marketplaces: Vec<String> = body
        .marketplaces
        .iter()
        .map(|m| m.trim().to_lowercase())
        .collect();
    // Площадки валидируются только для селлерских моделей: для не-селлеров
    // домен всё равно нормализует список в пустой (PerimeterAnswers::new) —
    // 422 на «лишний» список был бы контрактом-односторонкой: ozon проходит,
    // aliexpress падает, при одинаковом результате (формальное ревью 1.2).
    if matches!(
        trade_model,
        TradeModel::MarketplaceSeller | TradeModel::MarketplacePlanned
    ) {
        if marketplaces.iter().any(|m| m.is_empty()) {
            return Err(VerdictValidationError::InvalidMarketplaces);
        }
        if marketplaces
            .iter()
            .any(|m| !ALLOWED_MARKETPLACES.contains(&m.as_str()))
        {
            return Err(VerdictValidationError::UnknownMarketplace);
        }
    } else {
        marketplaces.clear();
    }
    marketplaces.sort();
    marketplaces.dedup();
    PerimeterAnswers::new(trade_model, marketplaces)
        .map_err(|_| VerdictValidationError::MarketplacesRequired)
}

/// Доля шага 0: step0_completed / installs, округление до 4 знаков;
/// без установок — честный 0.
fn step0_share(installs: i64, step0_completed: i64) -> f64 {
    if installs <= 0 {
        return 0.0;
    }
    ((step0_completed as f64 / installs as f64) * 10_000.0).round() / 10_000.0
}

// --- Общие построители ответов ---

pub(crate) type ApiResult<T> = Result<(StatusCode, Json<T>), (StatusCode, Json<ErrorEnvelope>)>;

pub(crate) fn error_response(
    status: StatusCode,
    code: &str,
    message: impl Into<String>,
) -> (StatusCode, Json<ErrorEnvelope>) {
    let message = message.into();
    let request_id = Uuid::now_v7();
    // request_id обязан попадать в логи: сообщённый пользователем идентификатор
    // коррелируется с записями шлюза (ревью Story 1.2).
    warn!(code, %request_id, %message, "запрос отклонён — конверт ошибки /v1");
    (
        status,
        Json(ErrorEnvelope::new(code, message).with_request_id(request_id)),
    )
}

/// 503-конверт «БД недоступна» (деградация не ломает маршруты /v1).
pub(crate) fn db_unavailable() -> (StatusCode, Json<ErrorEnvelope>) {
    error_response(
        StatusCode::SERVICE_UNAVAILABLE,
        "database_unavailable",
        "База данных недоступна — повторите попытку позже",
    )
}

/// Маппит ошибку стора в HTTP-ответ: БД — 503, повреждённый вердикт — 500.
fn store_error_response(err: StoreError) -> (StatusCode, Json<ErrorEnvelope>) {
    match err {
        StoreError::Database(db_err) => {
            warn!(?db_err, "store шага 0: ошибка БД");
            db_unavailable()
        }
        StoreError::CorruptVerdict(raw) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            format!("сохранённый вердикт повреждён: {raw:?}"),
        ),
    }
}

/// Эхо чужого ввода в сообщении об ошибке: ≤ 64 символа, control-символы
/// вырезаны — ни мегабайт, ни подделка многострочных лог-записей через
/// warn!-лог не проходят (формальное ревью Story 1.2).
pub(crate) fn echo(raw: &str) -> String {
    let filtered: String = raw.chars().filter(|c| !c.is_control()).collect();
    let mut shown: String = filtered.chars().take(64).collect();
    if filtered.chars().count() > 64 {
        shown.push('…');
    }
    shown
}

/// 422-конверт ошибки валидации ответов шага 0 (code человекочитаемый;
/// отражение чужого ввода обрезается — мегабайтная строка не возвращается
/// в сообщении).
fn validation_response(err: VerdictValidationError) -> (StatusCode, Json<ErrorEnvelope>) {
    let (code, message): (&str, String) = match err {
        VerdictValidationError::InvalidTradeModel(raw) => (
            "invalid_trade_model",
            format!(
                "торговая модель не распознана: {:?}; допустимы marketplace_seller, \
                 marketplace_planned, own_channels_no_marketplaces, services_other",
                echo(&raw)
            ),
        ),
        VerdictValidationError::MarketplacesRequired => (
            "marketplaces_required",
            "для торговли на маркетплейсах укажите хотя бы одну площадку".to_owned(),
        ),
        VerdictValidationError::InvalidMarketplaces => (
            "invalid_marketplaces",
            "список площадок содержит пустое значение".to_owned(),
        ),
        VerdictValidationError::UnknownMarketplace => (
            "unknown_marketplace",
            "площадка вне списка Ф0: wildberries, ozon, yandex_market".to_owned(),
        ),
    };
    error_response(StatusCode::UNPROCESSABLE_ENTITY, code, message)
}

/// 422-конверт зарезервированного DOMAIN (dev-портал — фикстура, не портал).
fn reserved_domain_response() -> (StatusCode, Json<ErrorEnvelope>) {
    error_response(
        StatusCode::UNPROCESSABLE_ENTITY,
        "reserved_member_id",
        "DOMAIN зарезервирован для dev-режима — укажите домен портала Б24",
    )
}

/// Маппит ошибки placement-контекста: резерв — 422 `reserved_member_id`,
/// частичный/сверхдлинный — 422 `invalid_placement` (честная диагностика
/// вместо тихой записи в dev-фикстуру или реестра-мусора).
pub(crate) fn placement_error_response(
    err: PlacementIdentityError,
) -> (StatusCode, Json<ErrorEnvelope>) {
    match err {
        PlacementIdentityError::ReservedDomain => reserved_domain_response(),
        PlacementIdentityError::PartialContext => error_response(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_placement",
            "placement-контекст частичный: DOMAIN и USER_ID передаются вместе \
             (или не передаются вовсе — тогда это dev-режим)",
        ),
        PlacementIdentityError::Oversized => error_response(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_placement",
            "значение DOMAIN/USER_ID слишком длинное",
        ),
    }
}

// --- Хендлеры ---

/// `GET /v1/onboarding/state` — upsert org+user по placement-контексту и
/// состояние шага 0: `step0_required` | `funnel` | `exit_screen`.
async fn onboarding_state(
    State(state): State<AppState>,
    query: Result<Query<HashMap<String, String>>, QueryRejection>,
) -> ApiResult<OnboardingStateResponse> {
    let params = match query {
        Ok(Query(params)) => params,
        Err(rejection) => {
            return Err(error_response(
                StatusCode::UNPROCESSABLE_ENTITY,
                "invalid_query",
                format!(
                    "query-параметры не читаются: {}",
                    echo(rejection.body_text().as_str())
                ),
            ));
        }
    };
    let identity = PlacementIdentity::from_params(&params).map_err(placement_error_response)?;
    let org_id = budgeted_sqlx(state.perimeter.ensure_organization(&identity.member_id)).await?;
    let user_id = budgeted_sqlx(state.perimeter.ensure_user(org_id, &identity.user_ref)).await?;
    let verdict = budgeted_store(state.perimeter.load_verdict(user_id)).await?;
    let state_str = match verdict {
        None => "step0_required",
        Some(Verdict::Seller) => "funnel",
        Some(Verdict::NonSeller) => "exit_screen",
    };
    Ok((
        StatusCode::OK,
        Json(OnboardingStateResponse { state: state_str }),
    ))
}

/// `POST /v1/perimeter/verdict` — фиксирует иммутабельный вердикт per-user
/// и возвращает `{verdict, state}` (повторный POST — сохранённый вердикт).
async fn perimeter_verdict(
    State(state): State<AppState>,
    query: Result<Query<HashMap<String, String>>, QueryRejection>,
    body: Result<Json<VerdictRequestBody>, JsonRejection>,
) -> ApiResult<VerdictResponse> {
    let params = match query {
        Ok(Query(params)) => params,
        Err(rejection) => {
            return Err(error_response(
                StatusCode::UNPROCESSABLE_ENTITY,
                "invalid_query",
                format!(
                    "query-параметры не читаются: {}",
                    echo(rejection.body_text().as_str())
                ),
            ));
        }
    };
    let Json(body) = body.map_err(|rejection| {
        error_response(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_body",
            format!(
                "тело запроса не читается: {}",
                echo(rejection.body_text().as_str())
            ),
        )
    })?;
    // Валидация до каких-либо касаний БД: 422 не создаёт сущностей и
    // работает даже в деградации (маршруты живы).
    let answers = answers_from_request(body).map_err(validation_response)?;
    let identity = PlacementIdentity::from_params(&params).map_err(placement_error_response)?;
    let org_id = budgeted_sqlx(state.perimeter.ensure_organization(&identity.member_id)).await?;
    let user_id = budgeted_sqlx(state.perimeter.ensure_user(org_id, &identity.user_ref)).await?;
    let verdict = decide_verdict(&answers);
    let stored = budgeted_store(
        state
            .perimeter
            .save_verdict(org_id, user_id, &answers, verdict),
    )
    .await?;
    if identity.is_dev_fallback() {
        info!("шаг 0 завершён в dev-режиме (dev-фоллбэк, в метрики не попадёт)");
    }
    let (verdict_str, state_str) = match stored {
        Verdict::Seller => ("seller", "funnel"),
        Verdict::NonSeller => ("non_seller", "exit_screen"),
    };
    info!(%user_id, verdict = verdict_str, "шаг 0 завершён: действующий вердикт");
    Ok((
        StatusCode::OK,
        Json(VerdictResponse {
            verdict: verdict_str,
            state: state_str,
        }),
    ))
}

/// `GET /v1/metrics/perimeter` — агрегат стора (без PII, без dev-портала).
async fn perimeter_metrics(State(state): State<AppState>) -> ApiResult<PerimeterMetricsResponse> {
    let (installs, step0_completed, sellers) =
        budgeted_sqlx(state.perimeter.perimeter_metrics()).await?;
    Ok((
        StatusCode::OK,
        Json(PerimeterMetricsResponse {
            installs,
            step0_completed,
            sellers,
            step0_share: step0_share(installs, step0_completed),
        }),
    ))
}

/// 404-конверт для неизвестных маршрутов (договор об ошибках — конверт всегда).
pub(crate) async fn not_found() -> (StatusCode, Json<ErrorEnvelope>) {
    error_response(StatusCode::NOT_FOUND, "not_found", "маршрут не найден")
}

/// 405-конверт для неверного метода.
pub(crate) async fn method_not_allowed() -> (StatusCode, Json<ErrorEnvelope>) {
    error_response(
        StatusCode::METHOD_NOT_ALLOWED,
        "method_not_allowed",
        "метод не поддерживается этим маршрутом",
    )
}

/// Роутер шага 0 (подключается в main к /health и /ready).
pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route("/v1/onboarding/state", get(onboarding_state))
        .route("/v1/perimeter/verdict", post(perimeter_verdict))
        .route("/v1/metrics/perimeter", get(perimeter_metrics))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    mod placement_identity {
        use super::*;

        #[test]
        fn full_context_maps_domain_and_user() {
            let identity = PlacementIdentity::from_params(&params(&[
                ("DOMAIN", "shop.example"),
                ("USER_ID", "42"),
                ("PLACEMENT", "DEFAULT"),
            ]))
            .expect("полный контекст — не ошибка");
            assert_eq!(identity.member_id, "shop.example");
            assert_eq!(identity.user_ref, "42");
            assert!(!identity.is_dev_fallback());
        }

        #[test]
        fn domain_is_canonicalized_to_lowercase() {
            // Варианты регистра не должны плодить дубликаты организаций
            // в реестре установок (инфляция знаменателя UJ-1).
            let identity = PlacementIdentity::from_params(&params(&[
                ("DOMAIN", "SHOP.Example"),
                ("USER_ID", "42"),
            ]))
            .expect("валидный контекст");
            assert_eq!(identity.member_id, "shop.example");
        }

        #[test]
        fn literal_dev_portal_domain_is_reserved() {
            // Явный DOMAIN=dev-portal — коллизия с фикстурой: 422, не слияние.
            let err = PlacementIdentity::from_params(&params(&[
                ("DOMAIN", "DEV-portal"),
                ("USER_ID", "1"),
            ]))
            .expect_err("зарезервированный литерал — отказ");
            assert_eq!(err, PlacementIdentityError::ReservedDomain);
        }

        #[test]
        fn empty_context_falls_back_to_dev() {
            let identity = PlacementIdentity::from_params(&HashMap::new())
                .expect("пустой контекст — фоллбэк, не ошибка");
            assert_eq!(identity.member_id, "dev-portal");
            assert_eq!(identity.user_ref, "dev-user");
            assert!(identity.is_dev_fallback());
        }

        #[test]
        fn partial_context_is_rejected() {
            // Частичный контекст не идентифицирует пользователя и не должен
            // молча превращать реальный портал в dev-фикстуру (ревью 1.2).
            let only_domain =
                PlacementIdentity::from_params(&params(&[("DOMAIN", "shop.example")]))
                    .expect_err("DOMAIN без USER_ID — отказ");
            assert_eq!(only_domain, PlacementIdentityError::PartialContext);

            let only_user = PlacementIdentity::from_params(&params(&[("USER_ID", "42")]))
                .expect_err("USER_ID без DOMAIN — отказ");
            assert_eq!(only_user, PlacementIdentityError::PartialContext);
        }

        #[test]
        fn oversized_identity_values_are_rejected() {
            // Краулерский мусор не становится постоянной строкой реестра
            // (write-on-GET публичен до хендшейка 1.3).
            let huge_domain = "d".repeat(254);
            let err = PlacementIdentity::from_params(&params(&[
                ("DOMAIN", huge_domain.as_str()),
                ("USER_ID", "42"),
            ]))
            .expect_err("домен длиннее DNS-лимита — отказ");
            assert_eq!(err, PlacementIdentityError::Oversized);

            let huge_user = "u".repeat(65);
            let err = PlacementIdentity::from_params(&params(&[
                ("DOMAIN", "shop.example"),
                ("USER_ID", huge_user.as_str()),
            ]))
            .expect_err("USER_ID длиннее 64 — отказ");
            assert_eq!(err, PlacementIdentityError::Oversized);
        }

        #[test]
        fn domain_trailing_dot_is_canonicalized() {
            // Вариант FQDN того же портала не должен плодить дубликат
            // организации (знаменатель UJ-1).
            let identity = PlacementIdentity::from_params(&params(&[
                ("DOMAIN", "shop.example."),
                ("USER_ID", "42"),
            ]))
            .expect("валидный контекст");
            assert_eq!(identity.member_id, "shop.example");
        }

        #[test]
        fn whitespace_values_are_trimmed_or_fall_back() {
            let identity = PlacementIdentity::from_params(&params(&[
                ("DOMAIN", "  shop.example  "),
                ("USER_ID", " 42 "),
            ]))
            .expect("валидный контекст");
            assert_eq!(identity.member_id, "shop.example");
            assert_eq!(identity.user_ref, "42");

            // Оба значения пусты после trim — placement-контекст отсутствует:
            // документированный dev-фоллбэк.
            let blank_both =
                PlacementIdentity::from_params(&params(&[("DOMAIN", "   "), ("USER_ID", "   ")]))
                    .expect("пустой контекст — фоллбэк");
            assert!(blank_both.is_dev_fallback());

            // Пустой DOMAIN при заданном USER_ID — частичный контекст:
            // отказ, а не тихая запись реального портала в фикстуру (ревью 1.2).
            let blank_domain =
                PlacementIdentity::from_params(&params(&[("DOMAIN", "   "), ("USER_ID", "42")]))
                    .expect_err("частичный контекст — отказ");
            assert_eq!(blank_domain, PlacementIdentityError::PartialContext);
        }
    }

    mod literal_mapping {
        use super::*;

        #[test]
        fn trade_model_literals_roundtrip() {
            for literal in [
                "marketplace_seller",
                "marketplace_planned",
                "own_channels_no_marketplaces",
                "services_other",
            ] {
                let model = trade_model_from_str(literal)
                    .unwrap_or_else(|| panic!("литерал {literal} обязан распознаваться"));
                assert_eq!(trade_model_as_str(&model), literal);
            }
        }

        #[test]
        fn trade_model_rejects_unknown_literals() {
            for bad in ["", "wholesale", "MarketplaceSeller", "seller"] {
                assert_eq!(
                    trade_model_from_str(bad),
                    None,
                    "литерал {bad:?} не входит в 4 модели"
                );
            }
        }

        #[test]
        fn verdict_literals_roundtrip() {
            assert_eq!(verdict_from_str("seller"), Some(Verdict::Seller));
            assert_eq!(verdict_as_str(&Verdict::Seller), "seller");
            assert_eq!(verdict_from_str("non_seller"), Some(Verdict::NonSeller));
            assert_eq!(verdict_as_str(&Verdict::NonSeller), "non_seller");
        }

        #[test]
        fn verdict_rejects_corrupt_values() {
            for bad in ["", "Seller", "non-seller", "unknown"] {
                assert_eq!(verdict_from_str(bad), None);
            }
        }
    }

    mod request_validation {
        use super::*;

        fn request(trade_model: Option<&str>, marketplaces: &[&str]) -> VerdictRequestBody {
            VerdictRequestBody {
                trade_model: trade_model.map(str::to_owned),
                marketplaces: marketplaces.iter().map(|s| (*s).to_owned()).collect(),
            }
        }

        #[test]
        fn valid_seller_answers_pass() {
            let answers = answers_from_request(request(
                Some("marketplace_seller"),
                &[" Wildberries ", "Ozon"],
            ))
            .expect("валидные ответы селлера");
            assert_eq!(answers.trade_model, TradeModel::MarketplaceSeller);
            // Нормализация: trim + нижний регистр + сортировка + дедуп.
            assert_eq!(answers.marketplaces, ["ozon", "wildberries"]);
            assert_eq!(decide_verdict(&answers), Verdict::Seller);
        }

        #[test]
        fn marketplaces_are_deduplicated() {
            let answers = answers_from_request(request(
                Some("marketplace_seller"),
                &["ozon", "OZON", " ozon ", "wildberries"],
            ))
            .expect("дубликаты не должны ломать сохранение");
            assert_eq!(answers.marketplaces, ["ozon", "wildberries"]);
        }

        #[test]
        fn unknown_marketplace_is_rejected() {
            // Закрытый список Ф0: свободный текст в jsonb не попадает.
            let err = answers_from_request(request(
                Some("marketplace_seller"),
                &["wildberries", "aliexpress"],
            ))
            .expect_err("площадка вне списка — 422");
            assert_eq!(err, VerdictValidationError::UnknownMarketplace);
        }

        #[test]
        fn missing_trade_model_is_invalid() {
            let err = answers_from_request(request(None, &["ozon"]))
                .expect_err("отсутствие trade_model — 422");
            assert_eq!(
                err,
                VerdictValidationError::InvalidTradeModel(String::new())
            );
        }

        #[test]
        fn unknown_trade_model_is_invalid() {
            let err = answers_from_request(request(Some("wholesale"), &[]))
                .expect_err("неизвестная модель — 422");
            assert_eq!(
                err,
                VerdictValidationError::InvalidTradeModel("wholesale".to_owned())
            );
        }

        #[test]
        fn seller_without_marketplaces_is_required_error() {
            let err = answers_from_request(request(Some("marketplace_planned"), &[]))
                .expect_err("селлерская модель без площадок — 422");
            assert_eq!(err, VerdictValidationError::MarketplacesRequired);
        }

        #[test]
        fn blank_marketplace_entry_is_invalid() {
            let err = answers_from_request(request(Some("marketplace_seller"), &["ozon", "  "]))
                .expect_err("пустая метка площадки — 422");
            assert_eq!(err, VerdictValidationError::InvalidMarketplaces);
        }

        #[test]
        fn non_seller_normalizes_marketplaces_away() {
            let answers = answers_from_request(request(Some("services_other"), &["ozon"]))
                .expect("не-селлер валиден без площадок");
            assert!(answers.marketplaces.is_empty());
            assert_eq!(decide_verdict(&answers), Verdict::NonSeller);
        }

        #[test]
        fn non_seller_marketplaces_are_discarded_without_validation() {
            // Для не-селлеров список не собирается: мусор в нём не ошибка —
            // домен нормализует его в пустой (контракт без односторонних 422:
            // ozon и aliexpress дают одинаковый результат — ревью 1.2).
            let answers =
                answers_from_request(request(Some("services_other"), &["aliexpress", ""]))
                    .expect("не-селлер с мусорным списком валиден");
            assert!(answers.marketplaces.is_empty());
            assert_eq!(decide_verdict(&answers), Verdict::NonSeller);
        }
    }

    mod response_shapes {
        use super::*;

        #[test]
        fn onboarding_state_serializes_minimal_shape() {
            let value = serde_json::to_value(OnboardingStateResponse {
                state: "step0_required",
            })
            .expect("сериализуемо");
            assert_eq!(value, serde_json::json!({"state": "step0_required"}));
        }

        #[test]
        fn verdict_serializes_verdict_and_state() {
            let value = serde_json::to_value(VerdictResponse {
                verdict: "non_seller",
                state: "exit_screen",
            })
            .expect("сериализуемо");
            assert_eq!(
                value,
                serde_json::json!({"verdict": "non_seller", "state": "exit_screen"})
            );
        }

        #[test]
        fn metrics_serializes_all_four_counters() {
            let value = serde_json::to_value(PerimeterMetricsResponse {
                installs: 3,
                step0_completed: 2,
                sellers: 1,
                step0_share: 0.6667,
            })
            .expect("сериализуемо");
            assert_eq!(
                value,
                serde_json::json!({
                    "installs": 3,
                    "step0_completed": 2,
                    "sellers": 1,
                    "step0_share": 0.6667
                })
            );
        }

        #[test]
        fn share_rounds_to_four_decimals_and_zero_without_installs() {
            assert_eq!(step0_share(0, 0), 0.0);
            assert_eq!(step0_share(1, 1), 1.0);
            assert_eq!(step0_share(3, 1), 0.3333);
            assert_eq!(step0_share(3, 2), 0.6667);
            // Зашита от отрицательного знаменателя (повреждённые данные).
            assert_eq!(step0_share(-1, 1), 0.0);
        }
    }

    mod error_envelopes {
        use super::*;

        #[test]
        fn db_unavailable_envelope_shape() {
            let (status, Json(envelope)) = db_unavailable();
            assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
            assert_eq!(envelope.error.code, "database_unavailable");
            // request_id проставлен каждому ошибочному ответу (трассировка).
            assert!(envelope.error.request_id.is_some());
        }

        #[test]
        fn validation_envelopes_have_readable_codes() {
            let cases = [
                (
                    VerdictValidationError::InvalidTradeModel("wholesale".to_owned()),
                    "invalid_trade_model",
                ),
                (
                    VerdictValidationError::MarketplacesRequired,
                    "marketplaces_required",
                ),
                (
                    VerdictValidationError::InvalidMarketplaces,
                    "invalid_marketplaces",
                ),
                (
                    VerdictValidationError::UnknownMarketplace,
                    "unknown_marketplace",
                ),
            ];
            for (err, code) in cases {
                let (status, Json(envelope)) = validation_response(err);
                assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "код {code}");
                assert_eq!(envelope.error.code, code);
                assert!(!envelope.error.message.is_empty());
            }
        }

        #[test]
        fn oversized_input_echo_is_truncated() {
            let huge = "x".repeat(300);
            let (_, Json(envelope)) =
                validation_response(VerdictValidationError::InvalidTradeModel(huge));
            // Отражение чужого ввода обрезано — мегабайт не возвращается.
            assert!(envelope.error.message.chars().count() < 200);
        }

        #[test]
        fn echo_strips_control_characters_from_input() {
            // Control-символы не попадают в конверт и warn!-лог: подделать
            // многострочную лог-запись чужим вводом нельзя (ревью 1.2).
            let (_, Json(envelope)) = validation_response(
                VerdictValidationError::InvalidTradeModel("bad\nINJECTED\tLOG".to_owned()),
            );
            assert!(!envelope.error.message.contains('\n'));
            assert!(!envelope.error.message.contains('\t'));
            assert!(envelope.error.message.contains("badINJECTEDLOG"));
        }

        #[tokio::test(start_paused = true)]
        async fn budgeted_store_times_out_to_503_envelope() {
            // Зависший стор (не отказ, а молчание) обязан уйти в бюджет:
            // просрочка → 503-конверт database_unavailable (ревью 1.2).
            let never: std::future::Pending<Result<i32, StoreError>> = std::future::pending();
            let Err((status, Json(envelope))) = budgeted_store(never).await else {
                panic!("зависшая операция обязана завершиться таймаутом бюджета");
            };
            assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
            assert_eq!(envelope.error.code, "database_unavailable");
        }

        #[test]
        fn reserved_domain_envelope_is_422() {
            let (status, Json(envelope)) = reserved_domain_response();
            assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
            assert_eq!(envelope.error.code, "reserved_member_id");
        }

        #[test]
        fn store_errors_map_to_503_and_500() {
            let (db_status, Json(db)) =
                store_error_response(StoreError::Database(sqlx::Error::RowNotFound));
            assert_eq!(db_status, StatusCode::SERVICE_UNAVAILABLE);
            assert_eq!(db.error.code, "database_unavailable");

            let (corrupt_status, Json(corrupt)) =
                store_error_response(StoreError::CorruptVerdict("maybe".to_owned()));
            assert_eq!(corrupt_status, StatusCode::INTERNAL_SERVER_ERROR);
            assert_eq!(corrupt.error.code, "internal_error");
        }
    }
}
