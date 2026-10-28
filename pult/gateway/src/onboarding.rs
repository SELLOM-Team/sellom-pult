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

/// member_id dev-фоллбэка: организация-фикстура, исключается из метрик
/// (не «установка», а документированный dev-режим без placement-параметров).
const DEV_MEMBER_ID: &str = "dev-portal";
/// external_ref dev-фоллбэка пользователя.
const DEV_USER_REF: &str = "dev-user";

/// Идентичность из placement-контекста Б24: member_id (DOMAIN) → ключ
/// реестра установок, user_ref (USER_ID) → внешний ref пользователя.
///
/// Частичный контекст (есть DOMAIN, нет USER_ID — или наоборот) не
/// идентифицирует пользователя: честный dev-фоллбэк, как и полное отсутствие
/// placement-параметров. Никакой собственной аутентификации (NFR-6).
#[derive(Debug, Clone, PartialEq, Eq)]
struct PlacementIdentity {
    /// Ключ реестра установок (organizations.b24_member_id).
    pub member_id: String,
    /// Внешний ref пользователя (users.external_ref).
    pub user_ref: String,
}

impl PlacementIdentity {
    /// Читает placement-контекст из query-параметров запроса.
    fn from_params(params: &HashMap<String, String>) -> Self {
        let domain = params.get("DOMAIN").map(|v| v.trim()).unwrap_or_default();
        let user = params.get("USER_ID").map(|v| v.trim()).unwrap_or_default();
        let placement = params
            .get("PLACEMENT")
            .map(|v| v.trim())
            .unwrap_or_default();
        if domain.is_empty() || user.is_empty() {
            debug!(
                placement,
                "placement-контекст неполный или отсутствует — dev-фоллбэк (в метрики не попадёт)"
            );
            Self {
                member_id: DEV_MEMBER_ID.to_owned(),
                user_ref: DEV_USER_REF.to_owned(),
            }
        } else {
            debug!(domain, user, placement, "placement-контекст Б24 распознан");
            Self {
                member_id: domain.to_owned(),
                user_ref: user.to_owned(),
            }
        }
    }

    /// true — идентичность подставлена dev-фоллбэком.
    fn is_dev_fallback(&self) -> bool {
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
    async fn ensure_organization(&self, member_id: &str) -> Result<Uuid, sqlx::Error> {
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
}

/// Валидация тела запроса → ответы домена: строковая гигиена (trim, пустые
/// метки) и литералы — здесь; бизнес-инвариант «селлер ⇒ площадки» —
/// в [`PerimeterAnswers::new`] домена.
fn answers_from_request(
    body: VerdictRequestBody,
) -> Result<PerimeterAnswers, VerdictValidationError> {
    let raw_model = body.trade_model.unwrap_or_default();
    let trade_model = trade_model_from_str(raw_model.trim())
        .ok_or_else(|| VerdictValidationError::InvalidTradeModel(raw_model.clone()))?;
    let marketplaces: Vec<String> = body
        .marketplaces
        .iter()
        .map(|m| m.trim().to_owned())
        .collect();
    if marketplaces.iter().any(|m| m.is_empty()) {
        return Err(VerdictValidationError::InvalidMarketplaces);
    }
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

type ApiResult<T> = Result<(StatusCode, Json<T>), (StatusCode, Json<ErrorEnvelope>)>;

fn error_response(
    status: StatusCode,
    code: &str,
    message: impl Into<String>,
) -> (StatusCode, Json<ErrorEnvelope>) {
    (
        status,
        Json(ErrorEnvelope::new(code, message).with_request_id(Uuid::now_v7())),
    )
}

/// 503-конверт «БД недоступна» (деградация не ломает маршруты /v1).
fn db_unavailable() -> (StatusCode, Json<ErrorEnvelope>) {
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

/// 422-конверт ошибки валидации ответов шага 0 (code человекочитаемый).
fn validation_response(err: VerdictValidationError) -> (StatusCode, Json<ErrorEnvelope>) {
    let (code, message): (&str, String) = match err {
        VerdictValidationError::InvalidTradeModel(raw) => (
            "invalid_trade_model",
            format!(
                "торговая модель не распознана: {raw:?}; допустимы marketplace_seller, \
                 marketplace_planned, own_channels_no_marketplaces, services_other"
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
    };
    error_response(StatusCode::UNPROCESSABLE_ENTITY, code, message)
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
                format!("query-параметры не читаются: {}", rejection.body_text()),
            ));
        }
    };
    let identity = PlacementIdentity::from_params(&params);
    let org_id = state
        .perimeter
        .ensure_organization(&identity.member_id)
        .await
        .map_err(|err| {
            warn!(?err, member_id = %identity.member_id, "upsert организации не выполнен");
            db_unavailable()
        })?;
    let user_id = state
        .perimeter
        .ensure_user(org_id, &identity.user_ref)
        .await
        .map_err(|err| {
            warn!(?err, "upsert пользователя не выполнен");
            db_unavailable()
        })?;
    let verdict = state
        .perimeter
        .load_verdict(user_id)
        .await
        .map_err(store_error_response)?;
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
                format!("query-параметры не читаются: {}", rejection.body_text()),
            ));
        }
    };
    let Json(body) = body.map_err(|rejection| {
        error_response(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_body",
            format!("тело запроса не читается: {}", rejection.body_text()),
        )
    })?;
    // Валидация до каких-либо касаний БД: 422 не создаёт сущностей и
    // работает даже в деградации (маршруты живы).
    let answers = answers_from_request(body).map_err(validation_response)?;
    let identity = PlacementIdentity::from_params(&params);
    let org_id = state
        .perimeter
        .ensure_organization(&identity.member_id)
        .await
        .map_err(|err| {
            warn!(?err, member_id = %identity.member_id, "upsert организации не выполнен");
            db_unavailable()
        })?;
    let user_id = state
        .perimeter
        .ensure_user(org_id, &identity.user_ref)
        .await
        .map_err(|err| {
            warn!(?err, "upsert пользователя не выполнен");
            db_unavailable()
        })?;
    let verdict = decide_verdict(&answers);
    let stored = state
        .perimeter
        .save_verdict(org_id, user_id, &answers, verdict)
        .await
        .map_err(store_error_response)?;
    if identity.is_dev_fallback() {
        info!("вердикт шага 0 зафиксирован в dev-режиме (dev-фоллбэк, в метрики не попадёт)");
    }
    let (verdict_str, state_str) = match stored {
        Verdict::Seller => ("seller", "funnel"),
        Verdict::NonSeller => ("non_seller", "exit_screen"),
    };
    info!(%user_id, verdict = verdict_str, "вердикт шага 0 зафиксирован");
    Ok((
        StatusCode::OK,
        Json(VerdictResponse {
            verdict: verdict_str,
            state: state_str,
        }),
    ))
}

/// Метрика шага 0 — детерминированный SQL-агрегат по таблице вердиктов
/// (событийный конвейер AD-22 появится в 1.4+). Исключаются: dev-портал
/// (`b24_member_id = 'dev-portal'` — фикстура, не установка) и строки без
/// member_id (не созданы placement-потоком). Установка «прошла шаг 0» =
/// в организации есть хотя бы один вердикт (шаг запоминается per-user;
/// знаменатель активации UJ-1 — org-уровень).
async fn perimeter_metrics(State(state): State<AppState>) -> ApiResult<PerimeterMetricsResponse> {
    let (installs, step0_completed, sellers): (i64, i64, i64) = sqlx::query_as(
        "SELECT \
            (SELECT count(*) FROM organizations o \
             WHERE o.b24_member_id IS NOT NULL AND o.b24_member_id <> 'dev-portal') AS installs, \
            (SELECT count(DISTINCT v.organization_id) FROM perimeter_verdicts v \
             JOIN organizations o ON o.id = v.organization_id \
             WHERE o.b24_member_id IS NOT NULL AND o.b24_member_id <> 'dev-portal') AS step0_completed, \
            (SELECT count(DISTINCT v.organization_id) FROM perimeter_verdicts v \
             JOIN organizations o ON o.id = v.organization_id \
             WHERE o.b24_member_id IS NOT NULL AND o.b24_member_id <> 'dev-portal' \
               AND v.verdict = 'seller') AS sellers",
    )
    .fetch_one(&state.perimeter.db)
    .await
    .map_err(|err| {
        warn!(?err, "метрика шага 0 не вычислена");
        db_unavailable()
    })?;
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
            ]));
            assert_eq!(identity.member_id, "shop.example");
            assert_eq!(identity.user_ref, "42");
            assert!(!identity.is_dev_fallback());
        }

        #[test]
        fn empty_context_falls_back_to_dev() {
            let identity = PlacementIdentity::from_params(&HashMap::new());
            assert_eq!(identity.member_id, "dev-portal");
            assert_eq!(identity.user_ref, "dev-user");
            assert!(identity.is_dev_fallback());
        }

        #[test]
        fn partial_context_falls_back_to_dev() {
            // Только DOMAIN или только USER_ID не идентифицируют пользователя.
            let only_domain =
                PlacementIdentity::from_params(&params(&[("DOMAIN", "shop.example")]));
            assert!(only_domain.is_dev_fallback());
            let only_user = PlacementIdentity::from_params(&params(&[("USER_ID", "42")]));
            assert!(only_user.is_dev_fallback());
        }

        #[test]
        fn whitespace_values_are_trimmed_or_fall_back() {
            let identity = PlacementIdentity::from_params(&params(&[
                ("DOMAIN", "  shop.example  "),
                ("USER_ID", " 42 "),
            ]));
            assert_eq!(identity.member_id, "shop.example");
            assert_eq!(identity.user_ref, "42");

            let blank =
                PlacementIdentity::from_params(&params(&[("DOMAIN", "   "), ("USER_ID", "42")]));
            assert!(blank.is_dev_fallback());
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
                &[" wildberries ", "ozon"],
            ))
            .expect("валидные ответы селлера");
            assert_eq!(answers.trade_model, TradeModel::MarketplaceSeller);
            assert_eq!(answers.marketplaces, ["wildberries", "ozon"]);
            assert_eq!(decide_verdict(&answers), Verdict::Seller);
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
            ];
            for (err, code) in cases {
                let (status, Json(envelope)) = validation_response(err);
                assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "код {code}");
                assert_eq!(envelope.error.code, code);
                assert!(!envelope.error.message.is_empty());
            }
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
