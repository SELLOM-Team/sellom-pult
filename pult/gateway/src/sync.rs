//! Прогресс и ретрай первичного синка (Story 1.4, FR-3).
//!
//! Маршруты `/v1` (AD-13; ошибки — конверт `{error:{code,message,request_id}}`):
//! - `GET /v1/sync/progress` — витрина цикла `initial_orders`: статус, фаза,
//!   процент (домен — только по подтверждённым окнам), ETA из наблюдаемой
//!   скорости, счётчики и `error_hint` (честный сбой ≠ «зелёный» прогресс);
//! - `POST /v1/sync/retry` — идемпотентная команда продолжения после сбоя:
//!   сбрасывает ТОЛЬКО терминальный `failed` (оркестратор идемпотентно
//!   возвращает терминальные строки — без сброса цикл не оживёт);
//!   `needs_retry`/`running`/`pending` подхватываются планировщиком сами —
//!   ретрай на них честный NO-OP, завершённые не ретраятся.
//!
//! Идентичность — placement-контекст Б24 как в 1.2/1.3 (query DOMAIN/USER_ID;
//! без параметров — документированный dev-фоллбэк). Прогресс-математика —
//! домен `core::sync::progress`; шлюз — композиция портов: каждый вызов
//! стора под бюджетом 2с (просрочка = 503-конверт, паттерн 1.2), три
//! последовательных обращения ≤ 6с < 8с клиентского бюджета SPA.
//!
//! Порты чтения/ретрая объектно-совместимы (`Arc<dyn …>`): sqlx-адаптеры —
//! здесь же (шлюз не импортирует сторы воркеров, AD-1), тесты хендлеров
//! подменяют их in-memory фейками. Резолвер организации — трейт-обёртка над
//! `ensure_organization` стора периметра (точка подмены тестов, семантика
//! placement→org из 1.2 не меняется).

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use axum::Json;
use axum::Router;
use axum::extract::rejection::QueryRejection;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::PgPool;
use tracing::{info, warn};
use uuid::Uuid;

use pult_core_sync::ports::{
    CycleType, RetryOutcome, RunStatus, SyncProgress, SyncProgressReader, SyncReadFuture,
    SyncRetryMutator, SyncRunState,
};
use pult_core_sync::progress::{
    CycleCounters, SyncPhase, cycle_eta_secs, cycle_percent, cycle_phase,
};
use pult_platform::error::ErrorEnvelope;

use crate::AppState;
use crate::onboarding::{
    ApiResult, PlacementIdentity, db_unavailable, echo, error_response, placement_error_response,
};

/// Бюджет ОДНОГО обращения к БД (паттерн 1.2: зависший запрос — тоже
/// деградация; до трёх обращений на запрос ≤ 6с < 8с бюджета SPA).
const STORE_BUDGET: Duration = Duration::from_secs(2);

/// Ошибка стора синка шлюза (без секретов в сообщениях).
#[derive(Debug, thiserror::Error)]
pub(crate) enum SyncStoreError {
    /// Сбой операции БД (недоступна/отказ запроса) → 503 `database_unavailable`.
    #[error("сбой БД синка: {0}")]
    Db(String),
    /// Данные строки вне словарей CHECK — повреждение, которое схема обязана
    /// исключать → 500 `internal_error` (честная диагностика, не подмена).
    #[error("повреждённые данные синка: {0}")]
    Corrupt(&'static str),
}

impl From<sqlx::Error> for SyncStoreError {
    fn from(err: sqlx::Error) -> Self {
        SyncStoreError::Db(err.to_string())
    }
}

/// Точка подмены организации в тестах хендлеров: member_id → UUID (ключ
/// реестра установок; семантика `ensure_organization` 1.2 — идемпотентный
/// upsert, write-on-GET прецедент до хендшейка).
pub(crate) trait OrgResolver: Send + Sync {
    fn resolve<'a>(
        &'a self,
        member_id: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<Uuid, SyncStoreError>> + Send + 'a>>;
}

impl OrgResolver for crate::onboarding::PgPerimeterStore {
    fn resolve<'a>(
        &'a self,
        member_id: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<Uuid, SyncStoreError>> + Send + 'a>> {
        Box::pin(async move {
            self.ensure_organization(member_id)
                .await
                .map_err(|err| SyncStoreError::Db(err.to_string()))
        })
    }
}

// ── sqlx-адаптеры портов (шлюз) ──────────────────────────────────────────────

/// epoch-float из SQL → DateTime (EXTRACT-паттерн: chrono-фича sqlx не
/// включается, прецедент шлюза/воркеров).
pub(crate) fn epoch_to_dt(secs: Option<f64>) -> Option<DateTime<Utc>> {
    let secs = secs?;
    let whole = secs.trunc() as i64;
    // float-шум суб-секунд не должен выпасть за диапазон наносекунд
    let nanos = (((secs - secs.trunc()) * 1e9).round() as i64).clamp(0, 999_999_999) as u32;
    DateTime::from_timestamp(whole, nanos)
}

/// PG-закрытие порта [`SyncProgressReader`] (строка `sync_runs` + started_at).
#[derive(Clone)]
pub(crate) struct PgSyncProgressReader {
    db: PgPool,
}

impl PgSyncProgressReader {
    pub(crate) fn new(db: PgPool) -> Self {
        Self { db }
    }
}

/// Строка витрины `sync_runs` для прогресса (прецедент `SyncRunRow` воркеров):
/// статус, граница чекпоинта, план/сделано, заказы/события, диагностика,
/// первый старт (epoch-secs).
type SyncProgressRow = (
    String,
    Option<String>,
    i32,
    i32,
    i64,
    i64,
    Option<String>,
    Option<f64>,
);

impl SyncProgressReader for PgSyncProgressReader {
    type Error = SyncStoreError;

    fn load_progress<'a>(
        &'a self,
        organization_id: Uuid,
        cycle: CycleType,
    ) -> SyncReadFuture<'a, Option<SyncProgress>, Self::Error> {
        Box::pin(async move {
            let row: Option<SyncProgressRow> = sqlx::query_as(
                "SELECT status, checkpoint->>'completed_until', windows_total, \
                        windows_done, orders_fetched, events_published, error_hint, \
                        EXTRACT(EPOCH FROM started_at)::float8 \
                 FROM sync_runs \
                 WHERE organization_id = $1 AND cycle_type = $2",
            )
            .bind(organization_id)
            .bind(cycle.as_str())
            .fetch_optional(&self.db)
            .await?;

            let Some((
                status_raw,
                completed_raw,
                total,
                done,
                fetched,
                published,
                hint,
                started_raw,
            )) = row
            else {
                return Ok(None);
            };
            let status = RunStatus::from_code(&status_raw)
                .ok_or(SyncStoreError::Corrupt("status вне CHECK-словаря"))?;
            let completed_until = match completed_raw {
                Some(raw) if !raw.is_empty() => Some(
                    DateTime::parse_from_rfc3339(&raw)
                        .map_err(|_| SyncStoreError::Corrupt("completed_until не RFC3339"))?
                        .with_timezone(&Utc),
                ),
                _ => None,
            };
            if total < 0 || done < 0 {
                return Err(SyncStoreError::Corrupt("отрицательные счётчики окон"));
            }
            Ok(Some(SyncProgress {
                state: SyncRunState {
                    organization_id,
                    cycle,
                    status,
                    completed_until,
                    windows_total: total as u32,
                    windows_done: done as u32,
                    orders_fetched: fetched.max(0) as u64,
                    events_published: published.max(0) as u64,
                    error_hint: hint,
                },
                started_at: epoch_to_dt(started_raw),
            }))
        })
    }
}

/// PG-закрытие порта [`SyncRetryMutator`] (guarded UPDATE + дочитывание).
#[derive(Clone)]
pub(crate) struct PgSyncRetryMutator {
    db: PgPool,
}

impl PgSyncRetryMutator {
    pub(crate) fn new(db: PgPool) -> Self {
        Self { db }
    }
}

impl SyncRetryMutator for PgSyncRetryMutator {
    type Error = SyncStoreError;

    fn request_retry<'a>(
        &'a self,
        organization_id: Uuid,
        cycle: CycleType,
    ) -> SyncReadFuture<'a, Option<RetryOutcome>, Self::Error> {
        Box::pin(async move {
            // Атомарный сброс терминального failed: диагностика израсходована,
            // продолжение — с чекпоинта (AD-3: windows_done/completed_until не
            // трогаем). Гонка с воркером исключена паттерном WHERE: строку под
            // failed воркер идемпотентно не меняет.
            let reset: Option<(String,)> = sqlx::query_as(
                "UPDATE sync_runs SET status = 'pending', error_hint = NULL \
                 WHERE organization_id = $1 AND cycle_type = $2 AND status = 'failed' \
                 RETURNING status",
            )
            .bind(organization_id)
            .bind(cycle.as_str())
            .fetch_optional(&self.db)
            .await?;
            if reset.is_some() {
                return Ok(Some(RetryOutcome::Reset));
            }
            // Не failed — честный NO-OP с текущим статусом (строки нет — None).
            let current: Option<(String,)> = sqlx::query_as(
                "SELECT status FROM sync_runs \
                 WHERE organization_id = $1 AND cycle_type = $2",
            )
            .bind(organization_id)
            .bind(cycle.as_str())
            .fetch_optional(&self.db)
            .await?;
            match current {
                None => Ok(None),
                Some((status_raw,)) => {
                    let status = RunStatus::from_code(&status_raw)
                        .ok_or(SyncStoreError::Corrupt("status вне CHECK-словаря"))?;
                    Ok(Some(RetryOutcome::Unchanged(status)))
                }
            }
        })
    }
}

// ── DTO контракта /v1/sync ───────────────────────────────────────────────────

/// Ответ GET /v1/sync/progress и POST /v1/sync/retry (единая форма: после
/// команды UI сразу видит свежее состояние цикла).
#[derive(Debug, Serialize)]
struct SyncProgressResponse {
    /// Статус цикла (словарь CHECK) или `not_started` — витрина без цикла
    /// (синк не начинался: нет verified-подключения либо планировщик не тикнул).
    status: String,
    /// Фаза работы (словарь домена: pending/fetching_history/completed).
    phase: &'static str,
    /// Процент подтверждёнными окнами (0..=100; not_started → 0).
    percent: u8,
    /// Оценка завершения из наблюдаемой скорости (None — UI «оцениваем»).
    eta_seconds: Option<u64>,
    /// Счётчики и диагностика цикла.
    details: SyncDetailsDto,
}

/// Счётчики цикла в ответе прогресса.
#[derive(Debug, Serialize)]
struct SyncDetailsDto {
    /// Тип цикла (initial_orders).
    cycle: &'static str,
    windows_total: u32,
    windows_done: u32,
    orders_fetched: u64,
    events_published: u64,
    /// Граница продолжения AD-3 (RFC3339).
    completed_until: Option<String>,
    /// Человекочитаемая диагностика сбоя (без секретов).
    error_hint: Option<String>,
}

/// ETA из наблюдаемой скорости активного цикла (чистая функция — тестируется
/// напрямую): средняя длительность окна = (now − started_at)/подтверждённые
/// окна. Честность: полный охват — домен отвечает 0 («оставшихся окон нет»);
/// ETA по скорости обещаем ТОЛЬКО активному `running` с наблюдениями — пауза
/// needs_retry (человек ждал) и прерванные статусы оценку не накручивают,
/// `None` честнее выдуманной цифры.
fn eta_from_observation(
    state: &SyncRunState,
    started_at: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> Option<u64> {
    let counters = CycleCounters {
        windows_total: state.windows_total,
        windows_done: state.windows_done,
    };
    if state.windows_done >= state.windows_total {
        // Полный охват (succeeded/у самой границы) — скорость не нужна:
        // домен честно отвечает «оставшихся окон нет → 0», не «оцениваем».
        return cycle_eta_secs(None, counters);
    }
    if state.status != RunStatus::Running || state.windows_done == 0 {
        return None;
    }
    let started = started_at?;
    // now < started_at (часы/транзит) не должны давать отрицательную скорость
    let elapsed_ms = now.signed_duration_since(started).num_milliseconds().max(1);
    let avg_window_secs = elapsed_ms as f64 / 1000.0 / state.windows_done as f64;
    cycle_eta_secs(Some(avg_window_secs), counters)
}

/// Витрина цикла → ответ прогресса (чистая функция: `None` — цикла нет).
fn progress_response(progress: Option<SyncProgress>, now: DateTime<Utc>) -> SyncProgressResponse {
    let Some(progress) = progress else {
        return SyncProgressResponse {
            status: "not_started".to_owned(),
            phase: SyncPhase::Pending.as_str(),
            percent: 0,
            eta_seconds: None,
            details: SyncDetailsDto {
                cycle: CycleType::InitialOrders.as_str(),
                windows_total: 0,
                windows_done: 0,
                orders_fetched: 0,
                events_published: 0,
                completed_until: None,
                error_hint: None,
            },
        };
    };
    let state = &progress.state;
    let counters = CycleCounters {
        windows_total: state.windows_total,
        windows_done: state.windows_done,
    };
    let details = SyncDetailsDto {
        cycle: state.cycle.as_str(),
        windows_total: state.windows_total,
        windows_done: state.windows_done,
        orders_fetched: state.orders_fetched,
        events_published: state.events_published,
        completed_until: state.completed_until.map(|ts| ts.to_rfc3339()),
        error_hint: state.error_hint.clone(),
    };
    SyncProgressResponse {
        status: state.status.as_str().to_owned(),
        phase: cycle_phase(state.status, counters).as_str(),
        percent: cycle_percent(counters),
        eta_seconds: eta_from_observation(state, progress.started_at, now),
        details,
    }
}

// ── бюджет и маппинг ошибок (паттерн 1.2) ────────────────────────────────────

pub(crate) async fn budgeted<T>(
    fut: impl Future<Output = Result<T, SyncStoreError>>,
) -> Result<T, (StatusCode, Json<ErrorEnvelope>)> {
    match tokio::time::timeout(STORE_BUDGET, fut).await {
        Ok(res) => res.map_err(sync_store_error_response),
        Err(_) => {
            warn!("store синка: превышен бюджет {STORE_BUDGET:?} — БД не отвечает");
            Err(db_unavailable())
        }
    }
}

fn sync_store_error_response(err: SyncStoreError) -> (StatusCode, Json<ErrorEnvelope>) {
    match err {
        SyncStoreError::Db(db_err) => {
            warn!(?db_err, "store синка: ошибка БД");
            db_unavailable()
        }
        SyncStoreError::Corrupt(what) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            format!("данные синка повреждены: {what}"),
        ),
    }
}

// ── хендлеры ──────────────────────────────────────────────────────────────────

/// Placement-параметры из query (форма 1.2/1.3; повреждённый query — 422).
pub(crate) fn identity_from(
    query: Result<Query<HashMap<String, String>>, QueryRejection>,
) -> Result<PlacementIdentity, (StatusCode, Json<ErrorEnvelope>)> {
    match query {
        Ok(Query(params)) => {
            PlacementIdentity::from_params(&params).map_err(placement_error_response)
        }
        Err(rejection) => Err(error_response(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_query",
            format!(
                "query-параметры не читаются: {}",
                echo(rejection.body_text().as_str())
            ),
        )),
    }
}

/// `GET /v1/sync/progress` — витрина цикла синка организации.
async fn sync_progress(
    State(state): State<AppState>,
    query: Result<Query<HashMap<String, String>>, QueryRejection>,
) -> ApiResult<SyncProgressResponse> {
    let identity = identity_from(query)?;
    let org_id = budgeted(state.sync.org.resolve(&identity.member_id)).await?;
    let progress = budgeted(
        state
            .sync
            .progress
            .load_progress(org_id, CycleType::InitialOrders),
    )
    .await?;
    Ok((
        StatusCode::OK,
        Json(progress_response(progress, Utc::now())),
    ))
}

/// `POST /v1/sync/retry` — идемпотечное продолжение прерванного цикла.
async fn sync_retry(
    State(state): State<AppState>,
    query: Result<Query<HashMap<String, String>>, QueryRejection>,
) -> ApiResult<SyncProgressResponse> {
    let identity = identity_from(query)?;
    let org_id = budgeted(state.sync.org.resolve(&identity.member_id)).await?;
    let outcome = budgeted(
        state
            .sync
            .retry
            .request_retry(org_id, CycleType::InitialOrders),
    )
    .await?;
    let Some(outcome) = outcome else {
        // Цикла нет — ретраить нечего; честный 409 (двойной POST сюда же
        // идемпотентен: ответ одинаков).
        return Err(error_response(
            StatusCode::CONFLICT,
            "sync_not_started",
            "цикл синка ещё не начинался — сначала подключите площадку",
        ));
    };
    match outcome {
        RetryOutcome::Reset => {
            info!(%org_id, "ретрай синка: failed → pending, продолжение с чекпоинта (AD-3)");
        }
        RetryOutcome::Unchanged(status) => {
            info!(
                %org_id,
                status = status.as_str(),
                "ретрай синка: статус не требует сброса (NO-OP, идемпотентность)"
            );
        }
    }
    let progress = budgeted(
        state
            .sync
            .progress
            .load_progress(org_id, CycleType::InitialOrders),
    )
    .await?;
    Ok((
        StatusCode::OK,
        Json(progress_response(progress, Utc::now())),
    ))
}

/// Роутер витрины синка (подключается в main).
pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route("/v1/sync/progress", get(sync_progress))
        .route("/v1/sync/retry", post(sync_retry))
}

/// Зависимости синка в состоянии шлюза (композиция портов; в тестах — фейки).
#[derive(Clone)]
pub(crate) struct SyncDeps {
    /// placement → организация (трейт-обёртка стора периметра).
    pub(crate) org: Arc<dyn OrgResolver>,
    /// Чтение витрины прогресса.
    pub(crate) progress: Arc<dyn SyncProgressReader<Error = SyncStoreError>>,
    /// Команда ретрая цикла.
    pub(crate) retry: Arc<dyn SyncRetryMutator<Error = SyncStoreError>>,
    /// Доска канбана (модуль `board`: хендлеру нужен honest-статус пустой
    /// доски; хранится здесь — единая композиция синка шлюза).
    pub(crate) board: Arc<dyn pult_core_sync::ports::OrdersBoardReader<Error = SyncStoreError>>,
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::sync::Mutex;

    use chrono::TimeZone;

    use pult_core_sync::orders::{BoardCard, BoardCardItem, BoardColumn};
    use pult_core_sync::ports::BoardSnapshot;

    // ── фейки портов (in-memory, семантика = SQL стора) ────────────────────
    // Арматура pub(crate): модуль `board` гоняет хендлер доски поверх тех же
    // портов — единая семантика фейков между тестами витрины и доски.

    /// Резолвер: фиксированная организация (или сбой — деградация).
    pub(crate) struct FakeOrg {
        pub(crate) fail: bool,
    }

    impl OrgResolver for FakeOrg {
        fn resolve<'a>(
            &'a self,
            _member_id: &'a str,
        ) -> Pin<Box<dyn Future<Output = Result<Uuid, SyncStoreError>> + Send + 'a>> {
            Box::pin(async move {
                if self.fail {
                    Err(SyncStoreError::Db("тестовая недоступность".into()))
                } else {
                    Ok(Uuid::nil())
                }
            })
        }
    }

    /// Витрина+ретрай in-memory: точная семантика Pg-стора (failed → Reset,
    /// прочее Unchanged, отсутствие строки — None), фиксирует вызовы.
    pub(crate) struct FakeSync {
        progress: Mutex<Option<SyncProgress>>,
        retry_calls: Mutex<u32>,
    }

    impl FakeSync {
        pub(crate) fn new(progress: Option<SyncProgress>) -> Self {
            Self {
                progress: Mutex::new(progress),
                retry_calls: Mutex::new(0),
            }
        }
    }

    impl SyncProgressReader for FakeSync {
        type Error = SyncStoreError;

        fn load_progress<'a>(
            &'a self,
            _organization_id: Uuid,
            _cycle: CycleType,
        ) -> Pin<Box<dyn Future<Output = Result<Option<SyncProgress>, Self::Error>> + Send + 'a>>
        {
            Box::pin(async move { Ok(self.progress.lock().expect("мьют витрины").clone()) })
        }
    }

    impl SyncRetryMutator for FakeSync {
        type Error = SyncStoreError;

        fn request_retry<'a>(
            &'a self,
            _organization_id: Uuid,
            _cycle: CycleType,
        ) -> Pin<Box<dyn Future<Output = Result<Option<RetryOutcome>, Self::Error>> + Send + 'a>>
        {
            Box::pin(async move {
                *self.retry_calls.lock().expect("мьют вызовов") += 1;
                let mut guard = self.progress.lock().expect("мьют витрины");
                match guard.as_mut() {
                    None => Ok(None),
                    Some(p) if p.state.status == RunStatus::Failed => {
                        p.state.status = RunStatus::Pending;
                        p.state.error_hint = None;
                        Ok(Some(RetryOutcome::Reset))
                    }
                    Some(p) => Ok(Some(RetryOutcome::Unchanged(p.state.status))),
                }
            })
        }
    }

    /// Доска in-memory.
    pub(crate) struct FakeBoard {
        pub(crate) snapshot: BoardSnapshot,
    }

    impl pult_core_sync::ports::OrdersBoardReader for FakeBoard {
        type Error = SyncStoreError;

        fn board_orders<'a>(
            &'a self,
            _organization_id: Uuid,
            _limit_per_column: u32,
        ) -> Pin<Box<dyn Future<Output = Result<BoardSnapshot, Self::Error>> + Send + 'a>> {
            Box::pin(async move { Ok(self.snapshot.clone()) })
        }
    }

    /// «Висящий» стор: не отвечает в пределах бюджета (тест 503 без реальных 2с
    /// — виртуальное время tokio auto-advance).
    struct HangingStore;

    impl SyncProgressReader for HangingStore {
        type Error = SyncStoreError;

        fn load_progress<'a>(
            &'a self,
            _organization_id: Uuid,
            _cycle: CycleType,
        ) -> Pin<Box<dyn Future<Output = Result<Option<SyncProgress>, Self::Error>> + Send + 'a>>
        {
            Box::pin(async move {
                tokio::time::sleep(Duration::from_secs(600)).await;
                unreachable!("тест будит стор раньше")
            })
        }
    }

    // ── сборка состояния ────────────────────────────────────────────────────

    pub(crate) fn deps(org: FakeOrg, sync: Arc<FakeSync>, board: BoardSnapshot) -> SyncDeps {
        SyncDeps {
            org: Arc::new(org),
            progress: sync.clone(),
            retry: sync,
            board: Arc::new(FakeBoard { snapshot: board }),
        }
    }

    pub(crate) fn test_state(sync_deps: SyncDeps) -> AppState {
        // Ленивый пул без подключений: /v1-хендлеры синка его не касаются
        // (стор — фейки), состояние собирается ради типовой формы AppState.
        let db = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect_lazy("postgres://pult:pult@127.0.0.1:1/pult")
            .expect("ленивый пул без подключений собирается");
        let perimeter = crate::onboarding::PgPerimeterStore::new(db.clone());
        let conn_store = crate::connections::PgConnectionStore::new(db.clone());
        let cipher = Arc::new(crate::connections::EnvAesGcmCipher::new(Arc::new(
            crate::connections::EnvMasterKeyProvider::new([0x42; 32]),
        )));
        let probe = crate::connections::HttpRightsProbe::new(None)
            .expect("HTTP-клиент проб без override собирается");
        AppState {
            db,
            version: "test",
            migrations_applied: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            perimeter,
            connections: crate::ConnectionsDeps {
                store: conn_store,
                cipher,
                probe: Arc::new(probe),
            },
            sync: sync_deps,
        }
    }

    pub(crate) fn full_params() -> HashMap<String, String> {
        [("DOMAIN", "shop.example"), ("USER_ID", "42")]
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    fn run_state(status: RunStatus, total: u32, done: u32, hint: Option<&str>) -> SyncRunState {
        SyncRunState {
            organization_id: Uuid::nil(),
            cycle: CycleType::InitialOrders,
            status,
            completed_until: Some(Utc.with_ymd_and_hms(2026, 1, 15, 0, 0, 0).unwrap()),
            windows_total: total,
            windows_done: done,
            orders_fetched: 120,
            events_published: 120,
            error_hint: hint.map(str::to_owned),
        }
    }

    fn progress(status: RunStatus, total: u32, done: u32, hint: Option<&str>) -> SyncProgress {
        SyncProgress {
            state: run_state(status, total, done, hint),
            started_at: Some(Utc::now() - chrono::Duration::seconds(100)),
        }
    }

    async fn call_progress(
        state: AppState,
        params: HashMap<String, String>,
    ) -> (StatusCode, serde_json::Value, serde_json::Value) {
        let response = sync_progress(State(state), Ok(Query(params))).await;
        match response {
            Ok((code, Json(body))) => {
                let value = serde_json::to_value(&body).expect("сериализуемо");
                (code, value["status"].clone(), value)
            }
            Err((code, Json(err))) => {
                let value = serde_json::to_value(&err).expect("сериализуемо");
                (code, value["error"]["code"].clone(), value)
            }
        }
    }

    // ── GET /v1/sync/progress ───────────────────────────────────────────────

    #[tokio::test]
    async fn progress_without_cycle_is_not_started() {
        let sync = Arc::new(FakeSync::new(None));
        let state = test_state(deps(
            FakeOrg { fail: false },
            sync,
            BoardSnapshot::default(),
        ));
        let (code, status, body) = call_progress(state, full_params()).await;
        assert_eq!(code, StatusCode::OK);
        assert_eq!(status, "not_started");
        assert_eq!(body["phase"], "pending");
        assert_eq!(body["percent"], 0);
        assert_eq!(body["eta_seconds"], serde_json::Value::Null);
        assert_eq!(body["details"]["cycle"], "initial_orders");
        assert_eq!(body["details"]["windows_total"], 0);
    }

    #[tokio::test]
    async fn progress_running_percent_and_eta_from_observation() {
        // 2 окна из 10 за 100с: percent 20, ETA = 8 × 50с = 400с
        let sync = Arc::new(FakeSync::new(Some(progress(
            RunStatus::Running,
            10,
            2,
            None,
        ))));
        let state = test_state(deps(
            FakeOrg { fail: false },
            sync,
            BoardSnapshot::default(),
        ));
        let (code, status, body) = call_progress(state, full_params()).await;
        assert_eq!(code, StatusCode::OK);
        assert_eq!(status, "running");
        assert_eq!(body["phase"], "fetching_history");
        assert_eq!(body["percent"], 20);
        assert_eq!(body["eta_seconds"], 400);
        assert_eq!(body["details"]["windows_done"], 2);
    }

    #[tokio::test]
    async fn progress_running_first_window_eta_unknown() {
        // Ни одно окно не подтверждено — скорости нет, ETA честно отсутствует
        let sync = Arc::new(FakeSync::new(Some(progress(
            RunStatus::Running,
            10,
            0,
            None,
        ))));
        let state = test_state(deps(
            FakeOrg { fail: false },
            sync,
            BoardSnapshot::default(),
        ));
        let (_, _, body) = call_progress(state, full_params()).await;
        assert_eq!(body["percent"], 0);
        assert_eq!(body["eta_seconds"], serde_json::Value::Null);
    }

    #[tokio::test]
    async fn progress_failed_is_visible_not_green() {
        // Сбой виден целиком: статус failed, диагностика на витрине, ETA нет
        let sync = Arc::new(FakeSync::new(Some(progress(
            RunStatus::Failed,
            10,
            2,
            Some("токен WB отклонён площадкой (отозван/недействителен)"),
        ))));
        let state = test_state(deps(
            FakeOrg { fail: false },
            sync,
            BoardSnapshot::default(),
        ));
        let (code, status, body) = call_progress(state, full_params()).await;
        assert_eq!(code, StatusCode::OK);
        assert_eq!(status, "failed");
        assert_eq!(body["percent"], 20);
        assert_eq!(body["eta_seconds"], serde_json::Value::Null);
        assert!(
            body["details"]["error_hint"]
                .as_str()
                .expect("hint на витрине")
                .contains("токен")
        );
    }

    #[tokio::test]
    async fn progress_succeeded_completed_phase_full_percent() {
        let sync = Arc::new(FakeSync::new(Some(progress(
            RunStatus::Succeeded,
            4,
            4,
            None,
        ))));
        let state = test_state(deps(
            FakeOrg { fail: false },
            sync,
            BoardSnapshot::default(),
        ));
        let (code, status, body) = call_progress(state, full_params()).await;
        assert_eq!(code, StatusCode::OK);
        assert_eq!(status, "succeeded");
        assert_eq!(body["phase"], "completed");
        assert_eq!(body["percent"], 100);
        assert_eq!(body["eta_seconds"], 0);
    }

    #[tokio::test]
    async fn progress_partial_placement_is_422_without_store_calls() {
        // Частичный контекст не идентифицирует: отказ до каких-либо обращений
        let sync = Arc::new(FakeSync::new(None));
        let state = test_state(deps(
            FakeOrg { fail: false },
            sync,
            BoardSnapshot::default(),
        ));
        let mut params = HashMap::new();
        params.insert("DOMAIN".to_owned(), "shop.example".to_owned());
        let (code, err, _) = call_progress(state, params).await;
        assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(err, "invalid_placement");
    }

    #[tokio::test]
    async fn progress_db_failure_is_503_envelope() {
        let sync = Arc::new(FakeSync::new(None));
        let state = test_state(deps(FakeOrg { fail: true }, sync, BoardSnapshot::default()));
        let (code, err, _) = call_progress(state, full_params()).await;
        assert_eq!(code, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(err, "database_unavailable");
    }

    #[tokio::test(start_paused = true)]
    async fn progress_hanging_store_hits_budget_503() {
        // Виртуальное время: таймаут бюджета срабатывает без реальных 2 секунд
        let deps = SyncDeps {
            org: Arc::new(FakeOrg { fail: false }),
            progress: Arc::new(HangingStore),
            retry: Arc::new(FakeSync::new(None)),
            board: Arc::new(FakeBoard {
                snapshot: BoardSnapshot::default(),
            }),
        };
        let state = test_state(deps);
        let (code, err, _) = call_progress(state, full_params()).await;
        assert_eq!(code, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(err, "database_unavailable");
    }

    // ── POST /v1/sync/retry ──────────────────────────────────────────────────

    async fn call_retry(
        state: AppState,
        params: HashMap<String, String>,
    ) -> (StatusCode, serde_json::Value, serde_json::Value) {
        let response = sync_retry(State(state), Ok(Query(params))).await;
        match response {
            Ok((code, Json(body))) => {
                let value = serde_json::to_value(&body).expect("сериализуемо");
                (code, value["status"].clone(), value)
            }
            Err((code, Json(err))) => {
                let value = serde_json::to_value(&err).expect("сериализуемо");
                (code, value["error"]["code"].clone(), value)
            }
        }
    }

    #[tokio::test]
    async fn retry_resets_failed_and_clears_hint() {
        let sync = Arc::new(FakeSync::new(Some(progress(
            RunStatus::Failed,
            10,
            2,
            Some("временный сбой источника WB"),
        ))));
        let state = test_state(deps(
            FakeOrg { fail: false },
            sync.clone(),
            BoardSnapshot::default(),
        ));
        let (code, status, body) = call_retry(state, full_params()).await;
        assert_eq!(code, StatusCode::OK);
        // Витрина после сброса: pending, продолжение с чекпоинта (2 окна целы)
        assert_eq!(status, "pending");
        assert_eq!(body["details"]["windows_done"], 2);
        assert_eq!(body["details"]["error_hint"], serde_json::Value::Null);
        assert_eq!(*sync.retry_calls.lock().expect("мьют"), 1);
    }

    #[tokio::test]
    async fn retry_on_active_cycle_is_idempotent_noop() {
        // running — уже подхвачен; повторный ретрай ничего не сбрасывает
        let sync = Arc::new(FakeSync::new(Some(progress(
            RunStatus::Running,
            10,
            2,
            None,
        ))));
        let state = test_state(deps(
            FakeOrg { fail: false },
            sync.clone(),
            BoardSnapshot::default(),
        ));
        let (code, status, _) = call_retry(state, full_params()).await;
        assert_eq!(code, StatusCode::OK);
        assert_eq!(status, "running");
        assert_eq!(*sync.retry_calls.lock().expect("мьют"), 1);
    }

    #[tokio::test]
    async fn retry_without_cycle_is_409() {
        let sync = Arc::new(FakeSync::new(None));
        let state = test_state(deps(
            FakeOrg { fail: false },
            sync.clone(),
            BoardSnapshot::default(),
        ));
        let (code, err, _) = call_retry(state, full_params()).await;
        assert_eq!(code, StatusCode::CONFLICT);
        assert_eq!(err, "sync_not_started");
        // идемпотентность: вторая команда даёт тот же ответ
        let state = test_state(deps(
            FakeOrg { fail: false },
            sync,
            BoardSnapshot::default(),
        ));
        let (code, err, _) = call_retry(state, full_params()).await;
        assert_eq!(code, StatusCode::CONFLICT);
        assert_eq!(err, "sync_not_started");
    }

    #[tokio::test]
    async fn retry_on_needs_retry_stays_queued() {
        // needs_retry подхватится планировщиком сам — сброс не нужен
        let sync = Arc::new(FakeSync::new(Some(progress(
            RunStatus::NeedsRetry,
            10,
            5,
            Some("временный сбой источника WB"),
        ))));
        let state = test_state(deps(
            FakeOrg { fail: false },
            sync,
            BoardSnapshot::default(),
        ));
        let (code, status, _) = call_retry(state, full_params()).await;
        assert_eq!(code, StatusCode::OK);
        assert_eq!(status, "needs_retry");
    }

    #[tokio::test]
    async fn retry_on_succeeded_is_noop() {
        // Завершённый цикл ретраить нечего (полный охват окон)
        let sync = Arc::new(FakeSync::new(Some(progress(
            RunStatus::Succeeded,
            4,
            4,
            None,
        ))));
        let state = test_state(deps(
            FakeOrg { fail: false },
            sync,
            BoardSnapshot::default(),
        ));
        let (code, status, _) = call_retry(state, full_params()).await;
        assert_eq!(code, StatusCode::OK);
        assert_eq!(status, "succeeded");
    }

    // ── витрина → DTO (чистые функции) ───────────────────────────────────────

    #[test]
    fn eta_clock_rollback_guard() {
        // started_at «в будущем» (часы/транзит) не даёт отрицательной скорости:
        // elapsed клампится к 1мс — ETA считается, не уходит в минус
        let now = Utc::now();
        let state = run_state(RunStatus::Running, 10, 2, None);
        let eta = eta_from_observation(&state, Some(now + chrono::Duration::seconds(60)), now);
        assert!(eta.is_some());
    }

    #[test]
    fn board_card_types_are_usable_in_snapshot() {
        // Контракт витрины: карточка с позициями группируется по колонкам
        let mut snapshot = BoardSnapshot::default();
        snapshot.columns.insert(
            BoardColumn::New,
            vec![BoardCard {
                id: Uuid::nil(),
                external_order_id: "1001".into(),
                marketplace: "wildberries".into(),
                scheme: "fbo".into(),
                board_column: BoardColumn::New,
                status_code: Some("3".into()),
                warehouse_name: Some("Коледино".into()),
                total_price_kopecks: 9_000,
                currency: "RUB".into(),
                is_cancelled: false,
                order_date: None,
                last_change_at: None,
                items: vec![BoardCardItem {
                    supplier_article: Some("ART-1".into()),
                    tech_size: Some("M".into()),
                    nm_id: Some(123),
                    quantity: 1,
                    total_price_kopecks: 9_000,
                }],
            }],
        );
        snapshot.totals.insert(BoardColumn::New, 1);
        assert!(!snapshot.is_empty());
        assert_eq!(
            snapshot.columns[&BoardColumn::New][0].items.len(),
            1,
            "позиции карточки на витрине"
        );
    }
}
