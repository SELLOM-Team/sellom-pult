//! Оркестратор первичного синка (Story 1.4, FR-3 / AD-3 / AD-15 / AD-22).
//!
//! Чистая логика цикла `initial_orders` поверх портов: план окон от
//! старейшей даты истории, прогон окнами, агрегация строк статистики в
//! канонические заказы (домен), публикация событий, чекпоинт ПОСЛЕ
//! подтверждённого окна. Инфраструктура (HTTP, брокер, PG) — за портами;
//! юнит-тесты гоняют цикл целиком на in-memory фейках (спека 1.4).
//!
//! Инварианты:
//! - **Порядок окон — от новых к старым**: свежие данные попадают на доску
//!   за минуты («канбан жив» — принятие ≤ 15 мин активной части), хвост
//!   истории добирается следом. Прогресс честный — только по
//!   подтверждённым окнам (прогресс-домен).
//! - **Граница at-least-once** — [`OrderEventPublisher::flush`] перед
//!   чекпоинтом: чекпоинт окна означает «события окна дошли до брокера»;
//!   повтор окна (сбой до чекпоинта) переиздаёт события — идемпотентность
//!   проекции (AD-15) гасит дубли.
//! - **401 ≠ ретрай**: классификация сбоя источника ([`SourceFailure`])
//!   решает `failed` (человек) vs `needs_retry` (продолжение с чекпоинта,
//!   AD-3). Сбой чекпоинт-сторы/брокера — тоже `needs_retry` (best-effort
//!   сохранение), состояние выводится из БД на следующем прогоне.
//! - **План окон детерминирован**: сетка от `history_since` шагом
//!   `window_days`; граница продолжения — `completed_until` (нижний край
//!   завершённого префикса новых окон). Смена `window_days` между
//!   прогонами одного цикла не поддерживается (конфиг-инвариант цикла).

use chrono::{DateTime, Duration, Utc};
use uuid::Uuid;

use pult_core_sync::orders::aggregate_wb_rows;
use pult_core_sync::ports::{
    CycleType, DateWindow, OrderEvent, OrderEventPublisher, RunStatus, SourceFailure,
    SyncCheckpointStore, SyncRunState, WbOrdersSource,
};

/// Тюнинг цикла — конфиг-постоянные (AR-4), провязка — `main`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncTuning {
    /// Размер окна дат, дней (минимум 1 — клампится в [`window_span`]).
    pub window_days: i64,
    /// Старейшая дата истории (`PULT_SYNC_SINCE`): план окон от неё до now.
    /// WB сам не отдаёт глубже собственного cutoff — пустые окна дёшевы.
    pub history_since: DateTime<Utc>,
}

/// Инфраструктурный сбой цикла (брокер/стора): состояние прогона
/// восстановимо из БД (последний чекпоинт), повтор — следующий тик
/// планировщика. В отличие от [`SourceFailure`] это НЕ вердикт по
/// источнику: окно будет переиздано целиком, дубли гасятся проекцией.
#[derive(Debug, thiserror::Error)]
pub enum OrchestratorError {
    /// Публикация события не прошла (брокер недоступен).
    #[error("публикация событий: {0}")]
    Publish(String),
    /// Брокер не подтвердил опубликованные события (flush/nack).
    #[error("брокер не подтвердил события окна: {0}")]
    Flush(String),
    /// Чекпоинт-стора не сохранила состояние (PG недоступна).
    #[error("чекпоинт-стора: {0}")]
    Checkpoint(String),
}

impl OrchestratorError {
    /// Человекочитаемая подсказка для `sync_runs.error_hint`.
    pub fn hint(&self) -> String {
        format!("конвейер: {self}")
    }
}

fn window_span(tuning: &SyncTuning) -> Duration {
    Duration::days(tuning.window_days.max(1))
}

/// План окон: сетка от `history_since` шагом `window_days` до `now`
/// (включительно), верх плана — узловая точка `history_since + n·W ≥ now`.
///
/// Пустой план (`now <= history_since`) — легален: истории нет, цикл
/// завершается сразу (прогресс-домен честно даёт 100%).
pub fn plan_windows(tuning: &SyncTuning, now: DateTime<Utc>) -> (u32, DateTime<Utc>) {
    let span = window_span(tuning);
    let delta = now.signed_duration_since(tuning.history_since);
    if delta <= Duration::zero() {
        return (0, tuning.history_since);
    }
    // ceil в миллисекундах: секунды теряют суб-секундный хвост и съедают
    // крайнее окно (2 дня + 100 мс обязаны покрыться 3 окнами, не 2).
    let span_ms = span.num_milliseconds().max(1);
    let n = (delta.num_milliseconds() + span_ms - 1) / span_ms;
    (n as u32, tuning.history_since + span * (n as i32))
}

/// Следующее окно прогона: шаг ВНИЗ от границы продолжения.
///
/// `completed_until` — нижний край завершённого префикса новых окон (для
/// нового плана — верх сетки). `None` при ненулевом плане — повреждённый
/// чекпоинт (гарантия цикла нарушена) — см. [`next_window_checked`].
fn next_window(state: &SyncRunState, tuning: &SyncTuning) -> Option<DateWindow> {
    if state.windows_done >= state.windows_total {
        return None;
    }
    let boundary = state.completed_until?;
    let mut from = boundary - window_span(tuning);
    if from < tuning.history_since {
        from = tuning.history_since;
    }
    Some(DateWindow { from, to: boundary })
}

/// Решение о судьбе цикла по классу сбоя источника (спека: «401 ≠ ретрай»).
fn terminal_status(failure: SourceFailure) -> RunStatus {
    match failure {
        SourceFailure::Transient => RunStatus::NeedsRetry,
        SourceFailure::AccessRevoked | SourceFailure::Fatal => RunStatus::Failed,
    }
}

/// Человекочитаемая диагностика для `error_hint` (секретов нет: сообщения
/// адаптера — redact-гигиены).
fn source_hint(failure: SourceFailure, detail: &str) -> String {
    match failure {
        SourceFailure::AccessRevoked => {
            format!("токен WB отклонён площадкой (отозван/недействителен): {detail}")
        }
        SourceFailure::Transient => {
            format!("временный сбой источника WB (лимит/сеть/5xx): {detail}")
        }
        SourceFailure::Fatal => {
            format!("неисправимый сбой источника WB (схема ответа/потолок тела): {detail}")
        }
    }
}

/// Прогоняет цикл `initial_orders` одной организации до терминального
/// состояния (`succeeded`/`failed`/`needs_retry`) либо инфраструктурного
/// сбоя (брокер/стора — [`OrchestratorError`], best-effort `needs_retry`).
///
/// Идемпотентен: циклы `succeeded`/`failed`/`superseded` возвращаются как
/// есть, без единого запроса к источнику (повторный вызов планировщика —
/// NO-OP). `running`-строки — продолжение с чекпоинта (stale после падения
/// воркера; liveness-детект — Epic 2, Ф0 один воркер).
pub async fn run_initial_orders<S, P, C>(
    organization_id: Uuid,
    source: &S,
    publisher: &P,
    checkpoints: &C,
    tuning: &SyncTuning,
    now: DateTime<Utc>,
) -> Result<SyncRunState, OrchestratorError>
where
    S: WbOrdersSource,
    S::Error: std::fmt::Display,
    P: OrderEventPublisher,
    P::Error: std::fmt::Display,
    C: SyncCheckpointStore,
    C::Error: std::fmt::Display,
{
    // 1. Состояние цикла: продолжение или новый план.
    let mut state = match checkpoints
        .load(organization_id, CycleType::InitialOrders)
        .await
        .map_err(|e| OrchestratorError::Checkpoint(e.to_string()))?
    {
        Some(s)
            if matches!(
                s.status,
                RunStatus::Succeeded | RunStatus::Failed | RunStatus::Superseded
            ) =>
        {
            return Ok(s);
        }
        // pending/running/needs_retry — продолжение с чекпоинта (AD-3).
        Some(s) => s,
        None => {
            let (total, plan_end) = plan_windows(tuning, now);
            SyncRunState {
                organization_id,
                cycle: CycleType::InitialOrders,
                status: RunStatus::Pending,
                completed_until: Some(plan_end),
                windows_total: total,
                windows_done: 0,
                orders_fetched: 0,
                events_published: 0,
                error_hint: None,
            }
        }
    };

    // 2. Переход в running (первый сохраняющий UPSERT создаёт строку и
    //    фиксирует started_at в сторе).
    state.status = RunStatus::Running;
    state.error_hint = None;
    save(checkpoints, &state).await?;

    // 3. Прогон окон: от новых к старым, чекпоинт после каждого.
    while state.windows_done < state.windows_total {
        let window = match next_window(&state, tuning) {
            Some(w) => w,
            // Гарантия цикла нарушена: план ненулевой, но границы нет.
            None => {
                return Err(OrchestratorError::Checkpoint(
                    "повреждённый чекпоинт: ненулевой план без границы продолжения".into(),
                ));
            }
        };
        let rows = match source.fetch_window(window).await {
            Ok(rows) => rows,
            Err(error) => {
                let failure = S::classify_failure(&error);
                state.status = terminal_status(failure);
                state.error_hint = Some(source_hint(failure, &error.to_string()));
                save(checkpoints, &state).await?;
                return Ok(state);
            }
        };
        let fetched = rows.len() as u64;
        let orders = aggregate_wb_rows(organization_id, rows);
        let mut published: u64 = 0;
        for order in orders {
            let event = OrderEvent::new(order, now);
            if let Err(e) = publisher.publish(&event).await {
                return Err(
                    infra_needs_retry(publisher, checkpoints, &mut state, e, "publish").await,
                );
            }
            published += 1;
        }
        // Граница at-least-once: чекпоинт только после подтверждения брокера.
        if let Err(e) = publisher.flush().await {
            return Err(infra_needs_retry(publisher, checkpoints, &mut state, e, "flush").await);
        }

        state.orders_fetched += fetched;
        state.events_published += published;
        state.windows_done += 1;
        state.completed_until = Some(window.from);
        save(checkpoints, &state).await?;
    }

    // 4. Полный охват — успех.
    state.status = RunStatus::Succeeded;
    state.error_hint = None;
    save(checkpoints, &state).await?;
    Ok(state)
}

async fn save<C>(checkpoints: &C, state: &SyncRunState) -> Result<(), OrchestratorError>
where
    C: SyncCheckpointStore,
    C::Error: std::fmt::Display,
{
    checkpoints
        .save(state)
        .await
        .map_err(|e| OrchestratorError::Checkpoint(e.to_string()))
}

/// Инфраструктурный сбой: best-effort фиксация `needs_retry` (состояние —
/// последний чекпоинт; окно будет переиздано, дубли гасятся AD-15), затем
/// честная ошибка наверх.
async fn infra_needs_retry<P, C, E>(
    publisher: &P,
    checkpoints: &C,
    state: &mut SyncRunState,
    error: E,
    stage: &str,
) -> OrchestratorError
where
    P: OrderEventPublisher,
    C: SyncCheckpointStore,
    C::Error: std::fmt::Display,
    E: std::fmt::Display,
{
    let orchestrator_error = match stage {
        "flush" => OrchestratorError::Flush(error.to_string()),
        _ => OrchestratorError::Publish(error.to_string()),
    };
    // Окно не дочекпоинчено: счётчики состояния — на уровне последнего
    // чекпоинта; помечаем цикл needs_retry (повтор с чекпоинта).
    let mut retry = state.clone();
    retry.status = RunStatus::NeedsRetry;
    retry.error_hint = Some(orchestrator_error.hint());
    if let Err(save_error) = checkpoints.save(&retry).await {
        tracing::error!(
            hint = %save_error,
            "needs_retry не зафиксирован (стора недоступна?) — состояние восстановит следующий прогон"
        );
        let _ = publisher.flush().await;
    }
    tracing::warn!(stage, hint = %orchestrator_error, "цикл переведён в needs_retry");
    orchestrator_error
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    use chrono::TimeZone;
    use pult_core_identity::vault::Marketplace;
    use pult_core_sync::orders::BoardColumn;
    use pult_core_sync::ports::{SourceFailure, WbOrderRow};

    // ── фейки портов (in-memory; спека: lapin/PG — CI e2e) ────────────────

    /// Ошибка фейкового источника — только класс (детали не важны домену).
    #[derive(Debug, Clone, PartialEq)]
    struct FakeSourceError {
        kind: SourceFailure,
    }

    impl std::fmt::Display for FakeSourceError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "fake-source-error ({:?})", self.kind)
        }
    }

    impl std::error::Error for FakeSourceError {}

    /// Скриптовый источник: очередь результатов по вызовам (окна идут от
    /// новых к старым — порядок фиксируется).
    struct FakeSource {
        script: Mutex<std::collections::VecDeque<Result<Vec<WbOrderRow>, FakeSourceError>>>,
        fetched: Mutex<Vec<DateWindow>>,
    }

    impl FakeSource {
        fn new(script: Vec<Result<Vec<WbOrderRow>, FakeSourceError>>) -> Self {
            Self {
                script: Mutex::new(script.into()),
                fetched: Mutex::new(Vec::new()),
            }
        }
    }
    impl WbOrdersSource for FakeSource {
        type Error = FakeSourceError;

        fn classify_failure(error: &Self::Error) -> SourceFailure {
            error.kind
        }

        async fn fetch_window(&self, window: DateWindow) -> Result<Vec<WbOrderRow>, Self::Error> {
            self.fetched.lock().unwrap().push(window);
            self.script
                .lock()
                .unwrap()
                .pop_front()
                .expect("скрипт фейка короче плана окон")
        }
    }

    struct FakePublisher {
        events: Mutex<Vec<OrderEvent>>,
    }

    impl FakePublisher {
        fn new() -> Self {
            Self {
                events: Mutex::new(Vec::new()),
            }
        }
    }

    impl OrderEventPublisher for FakePublisher {
        type Error = std::convert::Infallible;

        async fn publish(&self, event: &OrderEvent) -> Result<(), Self::Error> {
            self.events.lock().unwrap().push(event.clone());
            Ok(())
        }

        async fn flush(&self) -> Result<(), Self::Error> {
            Ok(())
        }
    }

    /// Издатель с инжекцией сбоя (тип ошибки — не Infallible).
    struct FakeFailPublisher {
        fail_publish: bool,
    }

    impl OrderEventPublisher for FakeFailPublisher {
        type Error = FakeSourceError;

        async fn publish(&self, _event: &OrderEvent) -> Result<(), Self::Error> {
            if self.fail_publish {
                return Err(FakeSourceError {
                    kind: SourceFailure::Transient,
                });
            }
            Ok(())
        }

        async fn flush(&self) -> Result<(), Self::Error> {
            Ok(())
        }
    }

    /// In-memory чекпоинт-стора с журналом сохранений.
    struct FakeCheckpoints {
        state: Mutex<Option<SyncRunState>>,
        saves: Mutex<Vec<SyncRunState>>,
    }

    impl FakeCheckpoints {
        fn new() -> Self {
            Self {
                state: Mutex::new(None),
                saves: Mutex::new(Vec::new()),
            }
        }
    }

    impl SyncCheckpointStore for FakeCheckpoints {
        type Error = FakeSourceError;

        async fn load(
            &self,
            organization_id: Uuid,
            cycle: CycleType,
        ) -> Result<Option<SyncRunState>, Self::Error> {
            let _ = (organization_id, cycle);
            Ok(self.state.lock().unwrap().clone())
        }

        async fn save(&self, state: &SyncRunState) -> Result<(), Self::Error> {
            *self.state.lock().unwrap() = Some(state.clone());
            self.saves.lock().unwrap().push(state.clone());
            Ok(())
        }
    }

    fn row(order_id: i64, change_ms: i64) -> WbOrderRow {
        WbOrderRow {
            date: Utc.timestamp_millis_opt(change_ms - 60_000).unwrap(),
            last_change_date: Utc.timestamp_millis_opt(change_ms).unwrap(),
            supplier_article: format!("ART-{order_id}"),
            tech_size: "M".into(),
            barcode: "2000000000000".into(),
            discount_percent: 10,
            is_cancel: false,
            income_id: 555,
            warehouse_name: "Коледино".into(),
            order_id,
            nm_id: 100 + order_id,
            subject: "Платья".into(),
            brand: "Бренд".into(),
            status: 3,
            finished: false,
            price: 10_000,
            total_price: 9_000,
        }
    }

    /// Однострочный заказ в момент `change_ms` (ревизия = lastChangeDate).
    fn rows_for(change_ms: i64) -> Vec<WbOrderRow> {
        vec![row(change_ms / 1000, change_ms)]
    }

    fn tuning(days: i64, since_ms: i64) -> SyncTuning {
        SyncTuning {
            window_days: days,
            history_since: Utc.timestamp_millis_opt(since_ms).unwrap(),
        }
    }

    const MS_DAY: i64 = 86_400_000;
    const SINCE_MS: i64 = 1_700_000_000_000; // 2023-11-14, фиксировано

    // ── план окон ──────────────────────────────────────────────────────────

    #[test]
    fn plan_is_grid_from_oldest_covering_now() {
        let t = tuning(1, SINCE_MS);
        let now = Utc
            .timestamp_millis_opt(SINCE_MS + 2 * MS_DAY + 100)
            .unwrap();
        let (n, plan_end) = plan_windows(&t, now);
        assert_eq!(n, 3);
        assert_eq!(
            plan_end,
            Utc.timestamp_millis_opt(SINCE_MS + 3 * MS_DAY).unwrap()
        );
        // точная кратность — не +1 окно
        let (n_exact, end_exact) =
            plan_windows(&t, Utc.timestamp_millis_opt(SINCE_MS + 3 * MS_DAY).unwrap());
        assert_eq!(n_exact, 3);
        assert_eq!(
            end_exact,
            Utc.timestamp_millis_opt(SINCE_MS + 3 * MS_DAY).unwrap()
        );
    }

    #[test]
    fn plan_empty_when_no_history() {
        let t = tuning(1, SINCE_MS);
        let (n, plan_end) = plan_windows(&t, Utc.timestamp_millis_opt(SINCE_MS).unwrap());
        assert_eq!(n, 0);
        assert_eq!(plan_end, t.history_since);
        let (n_past, _) = plan_windows(&t, Utc.timestamp_millis_opt(SINCE_MS - 1).unwrap());
        assert_eq!(n_past, 0);
        // и кламп отрицательного окна
        let t_zero = tuning(0, SINCE_MS);
        let (n_clamped, _) = plan_windows(
            &t_zero,
            Utc.timestamp_millis_opt(SINCE_MS + MS_DAY).unwrap(),
        );
        assert_eq!(n_clamped, 1);
    }

    // ── цикл: счастливый путь ─────────────────────────────────────────────

    #[tokio::test]
    async fn happy_path_windows_newest_first_with_checkpoints() {
        let t = tuning(1, SINCE_MS);
        let now = Utc.timestamp_millis_opt(SINCE_MS + 3 * MS_DAY).unwrap();
        // 3 окна, в каждом 1 заказ (момент внутри окна)
        let mid = |k: i64| SINCE_MS + k * MS_DAY + MS_DAY / 2;
        let source = FakeSource::new(vec![
            Ok(rows_for(mid(2))), // окно [since+2d, since+3d) — САМОЕ НОВОЕ
            Ok(rows_for(mid(1))),
            Ok(rows_for(mid(0))),
        ]);
        let publisher = FakePublisher::new();
        let checkpoints = FakeCheckpoints::new();

        let state = run_initial_orders(Uuid::nil(), &source, &publisher, &checkpoints, &t, now)
            .await
            .expect("цикл проходит");

        // окна от новых к старым
        let fetched = source.fetched.lock().unwrap();
        assert_eq!(fetched.len(), 3);
        assert_eq!(
            fetched[0].to,
            Utc.timestamp_millis_opt(SINCE_MS + 3 * MS_DAY).unwrap()
        );
        assert_eq!(fetched[2].from, Utc.timestamp_millis_opt(SINCE_MS).unwrap());
        assert!(fetched[0].to > fetched[1].to && fetched[1].to > fetched[2].to);

        assert_eq!(state.status, RunStatus::Succeeded);
        assert_eq!(state.windows_done, 3);
        assert_eq!(state.orders_fetched, 3);
        assert_eq!(state.events_published, 3);
        assert_eq!(state.completed_until, Some(t.history_since));

        // чекпоинт-инвариант: последний сохранённый — успех
        let saves = checkpoints.saves.lock().unwrap();
        assert_eq!(saves.last().unwrap().status, RunStatus::Succeeded);
        // переход в running зафиксирован ДО первого окна
        assert_eq!(saves[0].status, RunStatus::Running);
        assert_eq!(saves[0].windows_done, 0);

        // события — по одному на заказ, канон агрегации домена
        let events = publisher.events.lock().unwrap();
        assert_eq!(events.len(), 3);
        let order = &events[0].order;
        assert_eq!(order.marketplace, Marketplace::Wildberries);
        assert_eq!(order.board_column, BoardColumn::Risk); // status 3 вне словаря → risk
        assert_eq!(order.total_price_kopecks, 9_000);
        assert_eq!(order.items.len(), 1);
    }

    #[tokio::test]
    async fn empty_history_completes_with_honest_hundred_percent() {
        let t = tuning(1, SINCE_MS);
        let now = Utc.timestamp_millis_opt(SINCE_MS).unwrap();
        let source = FakeSource::new(vec![]); // план пуст — фейк не вызовется
        let publisher = FakePublisher::new();
        let checkpoints = FakeCheckpoints::new();

        let state = run_initial_orders(Uuid::nil(), &source, &publisher, &checkpoints, &t, now)
            .await
            .expect("пустая история — успех");

        assert_eq!(state.status, RunStatus::Succeeded);
        assert_eq!(state.windows_total, 0);
        assert!(source.fetched.lock().unwrap().is_empty());
        // прогресс-домен: пустой план = честные 100%
        use pult_core_sync::progress::{CycleCounters, cycle_percent};
        assert_eq!(
            cycle_percent(CycleCounters {
                windows_total: state.windows_total,
                windows_done: state.windows_done,
            }),
            100
        );
    }

    // ── чекпоинт-продолжение (AD-3) ───────────────────────────────────────

    #[tokio::test]
    async fn transient_failure_resumes_from_checkpoint_without_refetch() {
        let t = tuning(1, SINCE_MS);
        let now = Utc.timestamp_millis_opt(SINCE_MS + 3 * MS_DAY).unwrap();
        let mid = |k: i64| SINCE_MS + k * MS_DAY + MS_DAY / 2;

        // прогон 1: самое новое окно ок, второе — временный сбой
        let source = FakeSource::new(vec![
            Ok(rows_for(mid(2))),
            Err(FakeSourceError {
                kind: SourceFailure::Transient,
            }),
        ]);
        let publisher = FakePublisher::new();
        let checkpoints = FakeCheckpoints::new();
        let state = run_initial_orders(Uuid::nil(), &source, &publisher, &checkpoints, &t, now)
            .await
            .expect("терминальное состояние — не инфра-ошибка");
        assert_eq!(state.status, RunStatus::NeedsRetry);
        assert_eq!(state.windows_done, 1);
        assert_eq!(
            state.completed_until,
            Some(Utc.timestamp_millis_opt(SINCE_MS + 2 * MS_DAY).unwrap())
        );
        assert!(state.error_hint.as_deref().unwrap().contains("временный"));

        // прогон 2 (рестарт воркера): продолжает со СБОЙНОГО окна, не с нуля
        let source2 = FakeSource::new(vec![
            Ok(rows_for(mid(1))), // окно 1 — то, на котором упали
            Ok(rows_for(mid(0))), // окно 0 — хвост
        ]);
        let state2 = run_initial_orders(Uuid::nil(), &source2, &publisher, &checkpoints, &t, now)
            .await
            .expect("продолжение завершается");
        assert_eq!(state2.status, RunStatus::Succeeded);
        assert_eq!(state2.windows_done, 3);
        assert_eq!(state2.events_published, 3); // 1 из прогона 1 + 2 из прогона 2

        // окно 2 повторно НЕ запрашивалось (AD-3: съеденные окна не трогаем)
        let fetched2 = source2.fetched.lock().unwrap();
        assert_eq!(fetched2.len(), 2);
        assert_eq!(
            fetched2[0].to,
            Utc.timestamp_millis_opt(SINCE_MS + 2 * MS_DAY).unwrap()
        );
    }

    #[tokio::test]
    async fn access_revoked_is_failed_not_retry() {
        let t = tuning(1, SINCE_MS);
        let now = Utc.timestamp_millis_opt(SINCE_MS + MS_DAY).unwrap();
        let source = FakeSource::new(vec![Err(FakeSourceError {
            kind: SourceFailure::AccessRevoked,
        })]);
        let publisher = FakePublisher::new();
        let checkpoints = FakeCheckpoints::new();

        let state = run_initial_orders(Uuid::nil(), &source, &publisher, &checkpoints, &t, now)
            .await
            .expect("терминальное состояние");
        assert_eq!(state.status, RunStatus::Failed);
        assert_eq!(state.windows_done, 0);
        assert!(state.error_hint.as_deref().unwrap().contains("токен"));

        // повторный прогон НЕ ретраит failed (человек решает) — ни одного fetch
        let source2 = FakeSource::new(vec![]);
        let state2 = run_initial_orders(Uuid::nil(), &source2, &publisher, &checkpoints, &t, now)
            .await
            .expect("идемпотентный повтор");
        assert_eq!(state2.status, RunStatus::Failed);
        assert!(source2.fetched.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn terminal_states_short_circuit_without_fetch() {
        let t = tuning(1, SINCE_MS);
        let now = Utc.timestamp_millis_opt(SINCE_MS + MS_DAY).unwrap();
        let checkpoints = FakeCheckpoints::new();
        let done = SyncRunState {
            organization_id: Uuid::nil(),
            cycle: CycleType::InitialOrders,
            status: RunStatus::Succeeded,
            completed_until: Some(t.history_since),
            windows_total: 1,
            windows_done: 1,
            orders_fetched: 1,
            events_published: 1,
            error_hint: None,
        };
        checkpoints.save(&done).await.expect("фиксируем успех");

        let source = FakeSource::new(vec![]);
        let publisher = FakePublisher::new();
        let state = run_initial_orders(Uuid::nil(), &source, &publisher, &checkpoints, &t, now)
            .await
            .expect("успешный цикл не перезапускается");
        assert_eq!(state, done);
        assert!(source.fetched.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn infra_publish_failure_marks_needs_retry_best_effort() {
        let t = tuning(1, SINCE_MS);
        let now = Utc.timestamp_millis_opt(SINCE_MS + MS_DAY).unwrap();
        let source = FakeSource::new(vec![Ok(rows_for(SINCE_MS + 100))]); // не важно
        let publisher = FakeFailPublisher { fail_publish: true };
        let checkpoints = FakeCheckpoints::new();

        let err = run_initial_orders(Uuid::nil(), &source, &publisher, &checkpoints, &t, now)
            .await
            .expect_err("сбой публикации — ошибка наверх");
        assert!(matches!(err, OrchestratorError::Publish(_)));

        // best-effort: цикл зафиксирован needs_retry, окно не дочекпоинчено
        let saved = checkpoints
            .state
            .lock()
            .unwrap()
            .clone()
            .expect("состояние сохранено");
        assert_eq!(saved.status, RunStatus::NeedsRetry);
        assert_eq!(saved.windows_done, 0);
        assert!(saved.error_hint.is_some());
    }
}
