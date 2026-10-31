//! Прогресс первичного синка (Story 1.4, FR-3): честные фаза/проценты/ETA.
//!
//! Чистая математика без I/O: шлюз и воркер скармливают счётчики цикла
//! (окна дат из чекпоинтов AD-3), домен считает процент и оценку завершения.
//! Честность (инвариант спеки): процент считается ТОЛЬКО по подтверждённым
//! чекпоинтами окнам — «на глазок» скорости ничего не приписывается.

use crate::ports::RunStatus;

/// Фаза цикла синка (прогресс-контракт `/v1/sync/progress`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncPhase {
    /// Цикл создан, окна ещё не начаты.
    Pending,
    /// Читаем историю окнами (активная фаза).
    FetchingHistory,
    /// Цикл закрыт успешно.
    Completed,
}

impl SyncPhase {
    /// Код фазы — контракт `/v1/sync/progress` (литерал живёт в домене —
    /// словарь фаз часть прогресс-контракта, не HTTP-детали).
    pub fn as_str(&self) -> &'static str {
        match self {
            SyncPhase::Pending => "pending",
            SyncPhase::FetchingHistory => "fetching_history",
            SyncPhase::Completed => "completed",
        }
    }
}

/// Фаза цикла из статуса и счётчиков (чистая производная витрины):
/// - `succeeded`/`superseded` — цикл закрыт (`completed`; superseded —
///   замена цикла, для витрины это тоже терминальная точка);
/// - `pending` без единого подтверждённого окна — ещё не начинался;
/// - прочее (в том числе `pending` после сброса ретрая с уже съеденными
///   окнами, `running`, `needs_retry`, `failed`) — фаза чтения истории:
///   цикл в ней (активно или с чекпоинтом), исход виден в `status`.
pub fn cycle_phase(status: RunStatus, counters: CycleCounters) -> SyncPhase {
    match status {
        RunStatus::Succeeded | RunStatus::Superseded => SyncPhase::Completed,
        RunStatus::Pending if counters.windows_done == 0 => SyncPhase::Pending,
        _ => SyncPhase::FetchingHistory,
    }
}

/// Счётчики цикла (материал расчёта прогресса).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CycleCounters {
    /// Всего окон в цикле (план).
    pub windows_total: u32,
    /// Подтверждённых чекпоинтом окон (сделано).
    pub windows_done: u32,
}

/// Процент прогресса — только по подтверждённым окнам (0..=100).
///
/// Активная часть онбординга ≤ 15 мин достигается НЕ процентами, а размером
/// окна (первое окно — свежие данные, самые ценные для «канбан жив»).
pub fn cycle_percent(counters: CycleCounters) -> u8 {
    if counters.windows_total == 0 {
        // Пустой план (нет истории/нет окон) — цикл завершается сразу;
        // 100% честно: делать нечего.
        return 100;
    }
    let done = counters.windows_done.min(counters.windows_total) as u64;
    let total = counters.windows_total as u64;
    ((done * 100) / total).min(100) as u8
}

/// Оценка времени завершения, секунды.
///
/// `avg_window_secs` — скользящее среднее по уже пройденным окнам (наблюдаемая
/// скорость, не выдуманная): воркер считает из фактических длительностей
/// чекпоинт-окон. Нет данных (первое окно) → `None` (UI показывает «оцениваем»).
pub fn cycle_eta_secs(avg_window_secs: Option<f64>, counters: CycleCounters) -> Option<u64> {
    let remaining = counters.windows_total.saturating_sub(counters.windows_done);
    if remaining == 0 {
        return Some(0);
    }
    let avg = avg_window_secs?;
    if avg <= 0.0 {
        return None;
    }
    Some((remaining as f64 * avg).round() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_zero_plan_is_honest_full() {
        assert_eq!(
            cycle_percent(CycleCounters {
                windows_total: 0,
                windows_done: 0
            }),
            100
        );
    }

    #[test]
    fn percent_monotonic_and_capped() {
        let c = |done: u32, total: u32| CycleCounters {
            windows_total: total,
            windows_done: done,
        };
        assert_eq!(cycle_percent(c(0, 4)), 0);
        assert_eq!(cycle_percent(c(1, 4)), 25);
        assert_eq!(cycle_percent(c(2, 4)), 50);
        assert_eq!(cycle_percent(c(3, 4)), 75);
        assert_eq!(cycle_percent(c(4, 4)), 100);
        // чекпоинт не может опередить план; если опередил — клип, не паника
        assert_eq!(cycle_percent(c(9, 4)), 100);
    }

    #[test]
    fn eta_needs_observed_speed() {
        let c = CycleCounters {
            windows_total: 10,
            windows_done: 2,
        };
        // нет наблюдений — нет ETA (UI: «оцениваем»)
        assert_eq!(cycle_eta_secs(None, c), None);
        // 8 окон × 30с = 240с
        assert_eq!(cycle_eta_secs(Some(30.0), c), Some(240));
        // всё сделано — ноль, независимо от скорости
        let done = CycleCounters {
            windows_total: 4,
            windows_done: 4,
        };
        assert_eq!(cycle_eta_secs(Some(1_000.0), done), Some(0));
        // нулевая/отрицательная «скорость» — мусорный вход → None
        assert_eq!(cycle_eta_secs(Some(0.0), c), None);
    }

    #[test]
    fn phase_pending_fresh_cycle() {
        // создан, окон не начато
        assert_eq!(
            cycle_phase(
                RunStatus::Pending,
                CycleCounters {
                    windows_total: 5,
                    windows_done: 0,
                },
            ),
            SyncPhase::Pending
        );
    }

    #[test]
    fn phase_running_is_fetching_history() {
        let c = |done: u32| CycleCounters {
            windows_total: 5,
            windows_done: done,
        };
        assert_eq!(
            cycle_phase(RunStatus::Running, c(0)),
            SyncPhase::FetchingHistory
        );
        // сбойные статусы в фазе чтения истории: цикл в ней, исход — в status
        assert_eq!(
            cycle_phase(RunStatus::NeedsRetry, c(2)),
            SyncPhase::FetchingHistory
        );
        assert_eq!(
            cycle_phase(RunStatus::Failed, c(2)),
            SyncPhase::FetchingHistory
        );
        // pending после сброса ретрая: окна уже съедены — это продолжение,
        // не «начало с нуля»
        assert_eq!(
            cycle_phase(RunStatus::Pending, c(2)),
            SyncPhase::FetchingHistory
        );
    }

    #[test]
    fn phase_terminal_statuses_are_completed() {
        let full = CycleCounters {
            windows_total: 5,
            windows_done: 5,
        };
        assert_eq!(
            cycle_phase(RunStatus::Succeeded, full),
            SyncPhase::Completed
        );
        // superseded — терминальная точка витрины (замена цикла — будущие сторис)
        assert_eq!(
            cycle_phase(RunStatus::Superseded, full),
            SyncPhase::Completed
        );
    }

    #[test]
    fn phase_literals_contract() {
        assert_eq!(SyncPhase::Pending.as_str(), "pending");
        assert_eq!(SyncPhase::FetchingHistory.as_str(), "fetching_history");
        assert_eq!(SyncPhase::Completed.as_str(), "completed");
    }
}
