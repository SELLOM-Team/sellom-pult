//! Первый канбан (Story 1.4, FR-16, AD-22/AD-15): витрина заказов WB.
//!
//! Маршрут `/v1/board/orders` (AD-13): 7 колонок канонического словаря
//! (FR-16: новый → к сборке → к отгрузке → в пути → доставлен → возврат →
//! риск), до `BOARD_LIMIT_PER_COLUMN` карточек на колонку (свежие первыми —
//! индекс миграции 0007), карточка с позициями и суммой; `totals` — счётчики
//! ВСЕХ строк колонок (честное «показаны первые N» без фильтров Epic 4).
//!
//! Honest-пустые состояния (I/O матрица спеки): пустая доска ≠ «пусто
//! навсегда» — ответ определяется состоянием цикла:
//! - есть заказы → 200 (доска живёт с первого окна: окна идут от новых);
//! - пусто и цикла нет → 409 `sync_not_started`;
//! - пусто и pending/running/needs_retry → 409 `sync_in_progress`
//!   («данные ещё синкаются»);
//! - пусто и failed → 409 `sync_failed` (диагностика — в прогрессе синка);
//! - пусто и succeeded → 200 с пустыми колонками («заказов пока нет» —
//!   кабинет без истории, честная заглушка, не ошибка).
//!
//! Схемы без PII: контакты FBS не синкаются в 1.4 (спека-деfer), карточка —
//! агрегат позиций и сумм. Ozon на доске честно отсутствует: данные появятся
//! после завершения адаптера (копирайт в SPA, спека 1.4).

use std::collections::HashMap;

use axum::Json;
use axum::Router;
use axum::extract::rejection::QueryRejection;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::routing::get;
use serde::Serialize;
use sqlx::PgPool;
use uuid::Uuid;

use pult_core_sync::orders::{BoardCard, BoardCardItem, BoardColumn};
use pult_core_sync::ports::{
    BoardSnapshot, CycleType, OrdersBoardReader, RunStatus, SyncReadFuture,
};

use crate::AppState;
use crate::onboarding::{ApiResult, error_response};
use crate::sync::{SyncStoreError, budgeted, epoch_to_dt, identity_from};

/// Лимит карточек на колонку Ф0 (спека: «лимит на колонку Ф0=50»; фильтры и
/// пагинация — Epic 4).
const BOARD_LIMIT_PER_COLUMN: u32 = 50;

/// PG-закрытие порта [`OrdersBoardReader`]: карточки окном ROW_NUMBER по
/// колонкам (свежие первыми), позиции пачкой, счётчики GROUP BY — три
/// индексных чтения под бюджетом каждое (паттерн 1.2).
#[derive(Clone)]
pub(crate) struct PgBoardReader {
    db: PgPool,
}

impl PgBoardReader {
    pub(crate) fn new(db: PgPool) -> Self {
        Self { db }
    }
}

/// Строка карточки витрины (прецедент `SyncRunRow` воркеров): id, колонка,
/// внешний id, площадка, схема, статус, склад, копейки, валюта, отмена,
/// дата заказа, последнее изменение (epoch-secs).
type BoardCardRow = (
    Uuid,
    String,
    String,
    String,
    String,
    Option<String>,
    Option<String>,
    i64,
    String,
    bool,
    Option<f64>,
    Option<f64>,
);

/// Строка позиции витрины: заказ, артикул, размер, nmId, количество, копейки.
type BoardItemRow = (Uuid, Option<String>, Option<String>, Option<i64>, i64, i64);

impl OrdersBoardReader for PgBoardReader {
    type Error = SyncStoreError;

    fn board_orders<'a>(
        &'a self,
        organization_id: Uuid,
        limit_per_column: u32,
    ) -> SyncReadFuture<'a, BoardSnapshot, Self::Error> {
        Box::pin(async move {
            // 1. Карточки: окно ROW_NUMBER PARTITION BY board_column —
            //    ровно `limit` свежих строк каждой колонки одним запросом
            //    (idx_orders_board: organization_id, board_column,
            //    last_change_at DESC).
            let cards: Vec<BoardCardRow> = sqlx::query_as(
                "SELECT id, board_column, external_order_id, marketplace, scheme, \
                        status_code, warehouse_name, (total_price * 100)::bigint, \
                        currency, is_cancelled, \
                        EXTRACT(EPOCH FROM order_date)::float8, \
                        EXTRACT(EPOCH FROM last_change_at)::float8 \
                 FROM ( \
                     SELECT *, ROW_NUMBER() OVER ( \
                            PARTITION BY board_column \
                            ORDER BY last_change_at DESC NULLS LAST \
                        ) AS rn \
                     FROM orders WHERE organization_id = $1 \
                 ) ranked \
                 WHERE rn <= $2",
            )
            .bind(organization_id)
            .bind(i64::from(limit_per_column))
            .fetch_all(&self.db)
            .await?;

            let mut snapshot = BoardSnapshot::default();
            for row in cards {
                let (
                    id,
                    column_raw,
                    external_order_id,
                    marketplace,
                    scheme,
                    status_code,
                    warehouse_name,
                    total_kopecks,
                    currency,
                    is_cancelled,
                    order_date,
                    last_change_at,
                ) = row;
                let board_column = BoardColumn::from_code(&column_raw)
                    .ok_or(SyncStoreError::Corrupt("board_column вне канон-словаря"))?;
                snapshot
                    .columns
                    .entry(board_column)
                    .or_default()
                    .push(BoardCard {
                        id,
                        external_order_id,
                        marketplace,
                        scheme,
                        board_column,
                        status_code,
                        warehouse_name,
                        total_price_kopecks: total_kopecks,
                        currency,
                        is_cancelled,
                        order_date: epoch_to_dt(order_date),
                        last_change_at: epoch_to_dt(last_change_at),
                        items: Vec::new(),
                    });
            }

            // 2. Позиции пачкой по показанным заказам (составной FK-путь
            //    idx_order_items_order; заказов на витрине ≤ 7 × лимит).
            //    `quantity::bigint` обязателен: колонка INT4, а sqlx декодирует
            //    i64 только из INT8 (runtime-запросы без макросов это не ловят
            //    на компиляции — поймано живым CI e2e 31.10, run #36).
            let shown: Vec<Uuid> = snapshot
                .columns
                .values()
                .flat_map(|cards| cards.iter().map(|card| card.id))
                .collect();
            if !shown.is_empty() {
                let items: Vec<BoardItemRow> = sqlx::query_as(
                    "SELECT order_id, supplier_article, tech_size, nm_id, \
                                quantity::bigint, (total_price * 100)::bigint \
                         FROM order_items \
                         WHERE organization_id = $1 AND order_id = ANY($2)",
                )
                .bind(organization_id)
                .bind(shown)
                .fetch_all(&self.db)
                .await?;
                for (order_id, supplier_article, tech_size, nm_id, quantity, total_kopecks) in items
                {
                    let card = snapshot
                        .columns
                        .values_mut()
                        .flat_map(|cards| cards.iter_mut())
                        .find(|card| card.id == order_id);
                    if let Some(card) = card {
                        card.items.push(BoardCardItem {
                            supplier_article,
                            tech_size,
                            nm_id,
                            quantity,
                            total_price_kopecks: total_kopecks,
                        });
                    }
                }
            }

            // 3. Счётчики всех строк колонок (totals: «показано N из total»).
            let totals: Vec<(String, i64)> = sqlx::query_as(
                "SELECT board_column, count(*) FROM orders \
                 WHERE organization_id = $1 GROUP BY board_column",
            )
            .bind(organization_id)
            .fetch_all(&self.db)
            .await?;
            for (column_raw, count) in totals {
                let board_column = BoardColumn::from_code(&column_raw)
                    .ok_or(SyncStoreError::Corrupt("board_column вне канон-словаря"))?;
                snapshot.totals.insert(board_column, count.max(0) as u64);
            }
            Ok(snapshot)
        })
    }
}

// ── DTO контракта /v1/board/orders ──────────────────────────────────────────

/// Карточка на доске (суммы — копейки; SPA форматирует).
#[derive(Debug, Serialize)]
struct BoardCardDto {
    id: String,
    external_order_id: String,
    marketplace: String,
    scheme: String,
    /// Колонка (самодостаточность карточки вне массива колонок).
    board_column: &'static str,
    status_code: Option<String>,
    warehouse: Option<String>,
    total_kopecks: i64,
    currency: String,
    cancelled: bool,
    /// RFC3339.
    order_date: Option<String>,
    /// RFC3339 — статусная метка времени.
    last_change_at: Option<String>,
    items: Vec<BoardItemDto>,
}

/// Позиция карточки.
#[derive(Debug, Serialize)]
struct BoardItemDto {
    article: Option<String>,
    size: Option<String>,
    nm_id: Option<i64>,
    quantity: i64,
    total_kopecks: i64,
}

/// 7 колонок — фиксированные ключи контракта (SPA рендерит по словарю,
/// пустые колонки приходят явными пустыми массивами, не отсутствием ключей).
#[derive(Debug, Serialize)]
struct BoardColumnsDto {
    new: Vec<BoardCardDto>,
    assembling: Vec<BoardCardDto>,
    shipping: Vec<BoardCardDto>,
    in_transit: Vec<BoardCardDto>,
    delivered: Vec<BoardCardDto>,
    returned: Vec<BoardCardDto>,
    risk: Vec<BoardCardDto>,
}

/// Ответ GET /v1/board/orders.
#[derive(Debug, Serialize)]
struct BoardOrdersResponse {
    columns: BoardColumnsDto,
    /// Счётчики всех строк колонок (не только показанные).
    totals: BoardTotalsDto,
}

/// Totals — те же фиксированные ключи колонок.
#[derive(Debug, Serialize)]
struct BoardTotalsDto {
    new: u64,
    assembling: u64,
    shipping: u64,
    in_transit: u64,
    delivered: u64,
    returned: u64,
    risk: u64,
}

impl BoardColumnsDto {
    fn empty() -> Self {
        Self {
            new: Vec::new(),
            assembling: Vec::new(),
            shipping: Vec::new(),
            in_transit: Vec::new(),
            delivered: Vec::new(),
            returned: Vec::new(),
            risk: Vec::new(),
        }
    }

    fn column_mut(&mut self, column: BoardColumn) -> &mut Vec<BoardCardDto> {
        match column {
            BoardColumn::New => &mut self.new,
            BoardColumn::Assembling => &mut self.assembling,
            BoardColumn::Shipping => &mut self.shipping,
            BoardColumn::InTransit => &mut self.in_transit,
            BoardColumn::Delivered => &mut self.delivered,
            BoardColumn::Returned => &mut self.returned,
            BoardColumn::Risk => &mut self.risk,
        }
    }
}

impl BoardTotalsDto {
    fn count_of(&mut self, column: BoardColumn) -> &mut u64 {
        match column {
            BoardColumn::New => &mut self.new,
            BoardColumn::Assembling => &mut self.assembling,
            BoardColumn::Shipping => &mut self.shipping,
            BoardColumn::InTransit => &mut self.in_transit,
            BoardColumn::Delivered => &mut self.delivered,
            BoardColumn::Returned => &mut self.returned,
            BoardColumn::Risk => &mut self.risk,
        }
    }
}

fn card_dto(card: &BoardCard) -> BoardCardDto {
    BoardCardDto {
        id: card.id.to_string(),
        external_order_id: card.external_order_id.clone(),
        marketplace: card.marketplace.clone(),
        scheme: card.scheme.clone(),
        board_column: card.board_column.as_str(),
        status_code: card.status_code.clone(),
        warehouse: card.warehouse_name.clone(),
        total_kopecks: card.total_price_kopecks,
        currency: card.currency.clone(),
        cancelled: card.is_cancelled,
        order_date: card.order_date.map(|ts| ts.to_rfc3339()),
        last_change_at: card.last_change_at.map(|ts| ts.to_rfc3339()),
        items: card
            .items
            .iter()
            .map(|item| BoardItemDto {
                article: item.supplier_article.clone(),
                size: item.tech_size.clone(),
                nm_id: item.nm_id,
                quantity: item.quantity,
                total_kopecks: item.total_price_kopecks,
            })
            .collect(),
    }
}

/// Снимок доски → DTO (все 7 колонок всегда; отсутствующие — пустые/ноль).
fn board_response(snapshot: &BoardSnapshot) -> BoardOrdersResponse {
    let mut columns = BoardColumnsDto::empty();
    for (column, cards) in &snapshot.columns {
        columns
            .column_mut(*column)
            .extend(cards.iter().map(card_dto));
    }
    let mut totals = BoardTotalsDto {
        new: 0,
        assembling: 0,
        shipping: 0,
        in_transit: 0,
        delivered: 0,
        returned: 0,
        risk: 0,
    };
    for (column, count) in &snapshot.totals {
        *totals.count_of(*column) = *count;
    }
    BoardOrdersResponse { columns, totals }
}

// ── хендлер ──────────────────────────────────────────────────────────────────

/// `GET /v1/board/orders` — канбан организации (7 колонок, лимит на колонку).
///
/// Honest-договор пустой доски: заказы есть → 200 (даже при идущем синке —
/// окна идут от новых, доска живёт с первого окна); пусто → код по циклу
/// (см. доку модуля). Деградация БД → 503-конверт.
async fn board_orders(
    State(state): State<AppState>,
    query: Result<Query<HashMap<String, String>>, QueryRejection>,
) -> ApiResult<BoardOrdersResponse> {
    let identity = identity_from(query)?;
    let org_id = budgeted(state.sync.org.resolve(&identity.member_id)).await?;
    let snapshot = budgeted(
        state
            .sync
            .board
            .board_orders(org_id, BOARD_LIMIT_PER_COLUMN),
    )
    .await?;

    if !snapshot.is_empty() {
        return Ok((StatusCode::OK, Json(board_response(&snapshot))));
    }
    // Пустая проекция — состояние цикла решает, что честно показать.
    let progress = budgeted(
        state
            .sync
            .progress
            .load_progress(org_id, CycleType::InitialOrders),
    )
    .await?;
    match progress {
        Some(p) if p.state.status == RunStatus::Succeeded => {
            // Пустая история: цикл закрыт с полным охватом — заказов нет,
            // это не ошибка и не «данные идут» (I/O матрица).
            Ok((StatusCode::OK, Json(board_response(&snapshot))))
        }
        Some(p) if p.state.status == RunStatus::Failed => Err(error_response(
            StatusCode::CONFLICT,
            "sync_failed",
            "синк завершился с ошибкой — подробности в прогрессе синка",
        )),
        Some(p) if p.state.status == RunStatus::Superseded => Err(error_response(
            StatusCode::CONFLICT,
            "sync_superseded",
            "цикл синка заменён — доска будет доступна после нового цикла",
        )),
        Some(_) => Err(error_response(
            StatusCode::CONFLICT,
            "sync_in_progress",
            "первичная синхронизация ещё идёт — заказы появятся на доске \
             по мере обработки истории",
        )),
        None => Err(error_response(
            StatusCode::CONFLICT,
            "sync_not_started",
            "данные ещё не синкались — подключите Wildberries и дождитесь \
             первого цикла",
        )),
    }
}

/// Роутер доски (подключается в main).
pub(crate) fn router() -> Router<AppState> {
    Router::new().route("/v1/board/orders", get(board_orders))
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::Arc;

    use chrono::{TimeZone, Utc};

    use pult_core_sync::ports::{SyncProgress, SyncRunState};

    // Тестовая арматура синка (фейки) живёт в `sync::tests` — здесь только
    // хендлер доски поверх тех же портов: переиспользуем его хелперы сборки
    // состояния через публичный API модуля.
    use crate::sync::tests::{
        FakeBoard, FakeOrg, FakeSync, deps as sync_deps, full_params, test_state,
    };

    fn snapshot_with(card_column: BoardColumn, totals: u64) -> BoardSnapshot {
        let mut snapshot = BoardSnapshot::default();
        let mut card = BoardCard {
            id: Uuid::now_v7(),
            external_order_id: "1001".to_owned(),
            marketplace: "wildberries".to_owned(),
            scheme: "fbo".to_owned(),
            board_column: card_column,
            status_code: Some("3".to_owned()),
            warehouse_name: Some("Коледино".to_owned()),
            total_price_kopecks: 9_000,
            currency: "RUB".to_owned(),
            is_cancelled: false,
            order_date: Some(Utc.with_ymd_and_hms(2026, 1, 10, 8, 0, 0).unwrap()),
            last_change_at: Some(Utc.with_ymd_and_hms(2026, 1, 10, 9, 30, 0).unwrap()),
            items: vec![BoardCardItem {
                supplier_article: Some("ART-1".to_owned()),
                tech_size: Some("M".to_owned()),
                nm_id: Some(123),
                quantity: 1,
                total_price_kopecks: 9_000,
            }],
        };
        card.board_column = card_column;
        snapshot.columns.insert(card_column, vec![card]);
        snapshot.totals.insert(card_column, totals);
        snapshot
    }

    fn sync_with_status(status: Option<RunStatus>) -> Arc<FakeSync> {
        let progress = status.map(|status| SyncProgress {
            state: SyncRunState {
                organization_id: Uuid::nil(),
                cycle: CycleType::InitialOrders,
                status,
                completed_until: None,
                windows_total: 5,
                windows_done: 2,
                orders_fetched: 40,
                events_published: 40,
                error_hint: None,
            },
            started_at: None,
        });
        Arc::new(FakeSync::new(progress))
    }

    async fn call_board(
        state: AppState,
        params: HashMap<String, String>,
    ) -> (StatusCode, serde_json::Value, serde_json::Value) {
        let response = board_orders(State(state), Ok(Query(params))).await;
        match response {
            Ok((code, Json(body))) => {
                let value = serde_json::to_value(&body).expect("сериализуемо");
                (code, value["columns"].clone(), value)
            }
            Err((code, Json(err))) => {
                let value = serde_json::to_value(&err).expect("сериализуемо");
                (code, value["error"]["code"].clone(), value)
            }
        }
    }

    #[tokio::test]
    async fn board_with_orders_is_200_even_mid_sync() {
        // Доска живёт с первого окна: заказы есть → 200, состояние цикла
        // вторично (I/O матрица: «заказы на доске ≤ 15 мин»)
        let sync = sync_with_status(Some(RunStatus::Running));
        let state = test_state(sync_deps(
            FakeOrg { fail: false },
            sync,
            snapshot_with(BoardColumn::New, 1),
        ));
        let (code, columns, body) = call_board(state, full_params()).await;
        assert_eq!(code, StatusCode::OK);
        let new_cards = columns["new"].as_array().expect("колонка new — массив");
        assert_eq!(new_cards.len(), 1);
        assert_eq!(new_cards[0]["external_order_id"], "1001");
        assert_eq!(new_cards[0]["total_kopecks"], 9000);
        assert_eq!(new_cards[0]["items"][0]["nm_id"], 123);
        assert_eq!(
            columns["risk"]
                .as_array()
                .expect("пустые колонки — массивы")
                .len(),
            0
        );
        assert_eq!(body["totals"]["new"], 1);
        // карточка самодостаточна: колонка и в DTO
        assert_eq!(new_cards[0]["board_column"], "new");
    }

    #[tokio::test]
    async fn board_empty_mid_sync_is_409_in_progress() {
        let sync = sync_with_status(Some(RunStatus::Running));
        let state = test_state(sync_deps(
            FakeOrg { fail: false },
            sync,
            BoardSnapshot::default(),
        ));
        let (code, err, _) = call_board(state, full_params()).await;
        assert_eq!(code, StatusCode::CONFLICT);
        assert_eq!(err, "sync_in_progress");
    }

    #[tokio::test]
    async fn board_empty_pending_is_409_in_progress() {
        // pending без единого окна — это тоже «данные ещё синкаются»
        let sync = sync_with_status(Some(RunStatus::Pending));
        let state = test_state(sync_deps(
            FakeOrg { fail: false },
            sync,
            BoardSnapshot::default(),
        ));
        let (_, err, _) = call_board(state, full_params()).await;
        assert_eq!(err, "sync_in_progress");
    }

    #[tokio::test]
    async fn board_empty_succeeded_is_200_honest_placeholder() {
        // Пустая история — не ошибка: цикл закрыт с полным охватом
        let sync = sync_with_status(Some(RunStatus::Succeeded));
        let state = test_state(sync_deps(
            FakeOrg { fail: false },
            sync,
            BoardSnapshot::default(),
        ));
        let (code, columns, totals) = call_board(state, full_params()).await;
        assert_eq!(code, StatusCode::OK);
        for column in [
            "new",
            "assembling",
            "shipping",
            "in_transit",
            "delivered",
            "returned",
            "risk",
        ] {
            assert!(
                columns[column].is_array(),
                "колонка {column} обязана присутствовать пустым массивом"
            );
            assert_eq!(totals["totals"][column], 0);
        }
    }

    #[tokio::test]
    async fn board_empty_failed_is_409_with_hint_direction() {
        let sync = sync_with_status(Some(RunStatus::Failed));
        let state = test_state(sync_deps(
            FakeOrg { fail: false },
            sync,
            BoardSnapshot::default(),
        ));
        let (code, err, _) = call_board(state, full_params()).await;
        assert_eq!(code, StatusCode::CONFLICT);
        assert_eq!(err, "sync_failed");
    }

    #[tokio::test]
    async fn board_empty_needs_retry_is_409_in_progress() {
        // needs_retry — продолжение с чекпоинта: честно «ещё идёт»
        let sync = sync_with_status(Some(RunStatus::NeedsRetry));
        let state = test_state(sync_deps(
            FakeOrg { fail: false },
            sync,
            BoardSnapshot::default(),
        ));
        let (_, err, _) = call_board(state, full_params()).await;
        assert_eq!(err, "sync_in_progress");
    }

    #[tokio::test]
    async fn board_no_cycle_is_409_not_started() {
        let sync = sync_with_status(None);
        let state = test_state(sync_deps(
            FakeOrg { fail: false },
            sync,
            BoardSnapshot::default(),
        ));
        let (code, err, _) = call_board(state, full_params()).await;
        assert_eq!(code, StatusCode::CONFLICT);
        assert_eq!(err, "sync_not_started");
    }

    #[tokio::test]
    async fn board_partial_placement_is_422() {
        let sync = sync_with_status(None);
        let state = test_state(sync_deps(
            FakeOrg { fail: false },
            sync,
            BoardSnapshot::default(),
        ));
        let mut params = HashMap::new();
        params.insert("USER_ID".to_owned(), "42".to_owned());
        let (code, err, _) = call_board(state, params).await;
        assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(err, "invalid_placement");
    }

    #[test]
    fn dto_totals_all_seven_columns_present() {
        // Контракт SPA: все 7 ключей в columns и totals всегда
        let response = board_response(&BoardSnapshot::default());
        let value = serde_json::to_value(&response).expect("сериализуемо");
        for column in [
            "new",
            "assembling",
            "shipping",
            "in_transit",
            "delivered",
            "returned",
            "risk",
        ] {
            assert!(
                value["columns"][column].is_array(),
                "columns.{column} отсутствует"
            );
            assert!(
                value["totals"][column].is_u64(),
                "totals.{column} отсутствует"
            );
        }
    }

    #[test]
    fn fake_board_reads_are_configurable() {
        // самопроверка арматуры: FakeBoard отдаёт предустановленный снимок
        let snapshot = snapshot_with(BoardColumn::Risk, 3);
        let fake = FakeBoard {
            snapshot: snapshot.clone(),
        };
        assert_eq!(fake.snapshot.totals[&BoardColumn::Risk], 3);
        assert!(!fake.snapshot.is_empty());
    }
}
