//! Канонические заказы и первый канбан (Story 1.4, FR-3 / FR-16 / AD-22 / AD-15).
//!
//! Чистый домен (AD-1): без sqlx/axum/lapin. Каноническая модель заказа —
//! единый внутренний контракт (AD-22): нормализация статусов площадок в
//! 7-колоночный словарь канбана происходит ЗДЕСЬ, сырые коды сохраняются
//! рядом (`status_code`) — ничего не теряем. Идемпотентный ключ события —
//! sha256-отпечаток (AD-15), детерминированный и одинаково вычислимый на
//! любой стороне конвейера.

use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use pult_core_identity::vault::Marketplace;
use pult_core_identity::vault::Scheme;

/// Колонка канбана — канонический словарь FR-16: 7 колонок
/// «новый → к сборке → к отгрузке → в пути → доставлен → возврат → риск».
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoardColumn {
    /// новый
    New,
    /// к сборке
    Assembling,
    /// к отгрузке
    Shipping,
    /// в пути
    InTransit,
    /// доставлен
    Delivered,
    /// возврат
    Returned,
    /// риск
    Risk,
}

impl BoardColumn {
    /// Строковый код колонки — контракт БД (`orders.board_column` CHECK)
    /// и API `/v1/board/orders`. Маппинг строк живёт здесь, домен — источник
    /// словаря (AD-22).
    pub const ALL: [BoardColumn; 7] = [
        BoardColumn::New,
        BoardColumn::Assembling,
        BoardColumn::Shipping,
        BoardColumn::InTransit,
        BoardColumn::Delivered,
        BoardColumn::Returned,
        BoardColumn::Risk,
    ];

    /// Код колонки (строковый контракт).
    pub fn as_str(&self) -> &'static str {
        match self {
            BoardColumn::New => "new",
            BoardColumn::Assembling => "assembling",
            BoardColumn::Shipping => "shipping",
            BoardColumn::InTransit => "in_transit",
            BoardColumn::Delivered => "delivered",
            BoardColumn::Returned => "returned",
            BoardColumn::Risk => "risk",
        }
    }
}

/// Позиция заказа (каноническая, AD-22). Сериализация — DTO конвейера
/// (прецедент identity: serde в чистом домене запрещён).
#[derive(Debug, Clone, PartialEq)]
pub struct OrderItem {
    /// Артикул продавца (supplierArticle в WB).
    pub supplier_article: String,
    /// Размер (techSize).
    pub tech_size: String,
    /// Штрихкод (barcode).
    pub barcode: String,
    /// Артикул площадки (nmId в WB).
    pub nm_id: i64,
    /// Количество.
    pub quantity: i64,
    /// Цена до скидки, копейки (price в WB, целые копейки).
    pub price_kopecks: i64,
    /// Итоговая цена позиции, копейки (totalPrice в WB).
    pub total_price_kopecks: i64,
    /// Скидка, %.
    pub discount_percent: i64,
}

/// Канонический заказ — событие-проекция канбана (AD-22). Сериализация —
/// DTO конвейера (прецедент identity: домен без serde).
#[derive(Debug, Clone, PartialEq)]
pub struct CanonicalOrder {
    /// Организация-владелец (партиционный ключ AD-4).
    pub organization_id: Uuid,
    /// Площадка.
    pub marketplace: Marketplace,
    /// Внешний id заказа у площадки (orderId в WB).
    pub external_order_id: String,
    /// Схема работы (эвристика WB-строки: incomeID>0 → FBO, иначе FBS;
    /// [ASSUMPTION] — уточняется живым токеном владельца, Story 1.4).
    pub scheme: Scheme,
    /// Каноническая колонка канбана.
    pub board_column: BoardColumn,
    /// Сырой статус площадки (диагностика и уточнение словаря).
    pub status_code: i64,
    /// Ветка отмены заказа (isCancel в WB) — карточка уходит в «риск».
    pub is_cancelled: bool,
    /// Склад отгрузки (warehouseName).
    pub warehouse_name: String,
    /// Дата заказа (date).
    pub order_date: DateTime<Utc>,
    /// Последнее изменение (lastChangeDate) — источник ревизии.
    pub last_change_at: DateTime<Utc>,
    /// Позиции.
    pub items: Vec<OrderItem>,
    /// Сумма заказа, копейки (сумма totalPrice позиций).
    pub total_price_kopecks: i64,
    /// Валюта (WB — RUB).
    pub currency: String,
}

/// Тип сущности для идемпотентного ключа (AD-15).
pub const ENTITY_TYPE_ORDER: &str = "order";

impl CanonicalOrder {
    /// Ревизия заказа — монотонный маркер изменения (epoch-ms lastChangeDate).
    /// Барьер откатов: проекция применяет только более НОВУЮ ревизию.
    pub fn revision(&self) -> i64 {
        self.last_change_at.timestamp_millis()
    }

    /// Идемпотентный ключ события AD-15:
    /// `hex(sha256(marketplace | тип | внешний id | revision))` —
    /// детерминированный отпечаток: точный дубль события даёт тот же ключ
    /// (UNIQUE в проекции гасит повторную доставку at-least-once).
    pub fn idem_key(&self) -> String {
        let material = format!(
            "{}|{}|{}|{}",
            self.marketplace.as_str(),
            ENTITY_TYPE_ORDER,
            self.external_order_id,
            self.revision(),
        );
        let digest = Sha256::digest(material.as_bytes());
        hex_lower(&digest)
    }
}

/// Низший hex без внешних зависимостей (домен не тащит hex-крейт ради
/// 32 байтов).
fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

/// Вход нормализации статуса WB (то, что статистика даёт в строке заказа).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WbStatusInput {
    /// Код статуса (поле status статистики WB).
    pub code: i64,
    /// Флаг отмены (isCancel).
    pub is_cancel: bool,
    /// Флаг завершённости (finished).
    pub finished: bool,
}

/// Статус WB → колонка канбана (нормализация AD-22, словарь ядра).
///
/// Правила честности (без выдумывания кодов, Директива 1):
/// - отмена (`is_cancel`) → «риск» (отменённый заказ требует внимания;
///   колонки «отменён» в 7-словаре нет);
/// - завершён и не отменён (`finished`) → «доставлен»;
/// - код из таблицы [`WB_STATUS_DICTIONARY`] → колонка таблицы;
/// - неизвестный код → «риск» + сырое значение сохраняется в `status_code`
///   (на карточке видно; ничего не теряем — инвариант спеки 1.4).
pub fn wb_board_column(input: WbStatusInput) -> BoardColumn {
    if input.is_cancel {
        return BoardColumn::Risk;
    }
    if input.finished {
        return BoardColumn::Delivered;
    }
    for (code, column) in WB_STATUS_DICTIONARY {
        if *code == input.code {
            return *column;
        }
    }
    BoardColumn::Risk
}

/// Словарь кодов статусов WB → канон-колонки. ВЕРСИОНИРУЕТСЯ (AD-22).
///
/// [ASSUMPTION] Таблица сознательно ПУСТА на старте Story 1.4: числовые
/// коды статистики WB не верифицированы достоверным источником (поиск и
/// зеркало SDK не дают таблицу; Директива 1 запрещает выдумывать коды).
/// Заполнение — внешняя верификация живым токеном владельца (шаг из спеки
/// 1.4): коды и переходы фиксируются здесь, `schema_version` события
/// инкрементится при смене словаря. Механика конвейера от таблицы не
/// зависит: CI-заглушка подаёт коды, маппер применяется одинаково.
pub const WB_STATUS_DICTIONARY: &[(i64, BoardColumn)] = &[];

/// Агрегирует сырые строки статистики WB в канонические заказы.
///
/// Строка статистики = позиция; заказ = строки одного `order_id`. Порядок
/// строк сохраняется в позициях; сумма заказа — сумма `totalPrice` позиций;
/// ревизия заказа — максимум `lastChangeDate` строк (любая позиция меняет
/// ревизию всего заказа). Схема — эвристика incomeID ([ASSUMPTION] спеки:
/// >0 → FBO, 0 → FBS; realFBS в статистике не отличим — уточняется живым
/// токеном владельца).
pub fn aggregate_wb_rows(
    organization_id: Uuid,
    rows: Vec<crate::ports::WbOrderRow>,
) -> Vec<CanonicalOrder> {
    use std::collections::BTreeMap;

    // BTreeMap: детерминированный порядок (id заказа) — порядок событий
    // стабилен при повторной публикации окна (тесты и идемпотентность).
    let mut grouped: BTreeMap<i64, Vec<crate::ports::WbOrderRow>> = BTreeMap::new();
    for row in rows {
        grouped.entry(row.order_id).or_default().push(row);
    }

    grouped
        .into_iter()
        .map(|(order_id, mut lines)| {
            // ревизия заказа: максимум lastChangeDate по строкам
            let last_change_at = lines
                .iter()
                .map(|r| r.last_change_date)
                .max()
                .expect("агрегат непустой");
            let order_date = lines
                .iter()
                .map(|r| r.date)
                .min()
                .expect("агрегат непустой");
            let any_cancel = lines.iter().any(|r| r.is_cancel);
            let any_finished = lines.iter().any(|r| r.finished);
            // статус: последняя по времени строка — источник кода/флагов
            lines.sort_by_key(|r| r.last_change_date);
            let last = lines.last().expect("агрегат непустой");
            let board_column = wb_board_column(crate::orders::WbStatusInput {
                code: last.status,
                is_cancel: any_cancel,
                finished: any_finished,
            });
            let scheme = if last.income_id > 0 {
                Scheme::Fbo
            } else {
                Scheme::Fbs
            };
            let items: Vec<OrderItem> = lines
                .iter()
                .map(|r| OrderItem {
                    supplier_article: r.supplier_article.clone(),
                    tech_size: r.tech_size.clone(),
                    barcode: r.barcode.clone(),
                    nm_id: r.nm_id,
                    quantity: 1, // строка статистики = 1 позиция единицы товара
                    price_kopecks: r.price,
                    total_price_kopecks: r.total_price,
                    discount_percent: r.discount_percent,
                })
                .collect();
            let total: i64 = items.iter().map(|i| i.total_price_kopecks).sum();
            CanonicalOrder {
                organization_id,
                marketplace: Marketplace::Wildberries,
                external_order_id: order_id.to_string(),
                scheme,
                board_column,
                status_code: last.status,
                is_cancelled: any_cancel,
                warehouse_name: last.warehouse_name.clone(),
                order_date,
                last_change_at,
                items,
                total_price_kopecks: total,
                currency: "RUB".into(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn order(rev_ms: i64, external: &str) -> CanonicalOrder {
        CanonicalOrder {
            organization_id: Uuid::nil(),
            marketplace: Marketplace::Wildberries,
            external_order_id: external.to_string(),
            scheme: Scheme::Fbo,
            board_column: BoardColumn::New,
            status_code: 0,
            is_cancelled: false,
            warehouse_name: "Коледино".into(),
            order_date: DateTime::from_timestamp_millis(1_700_000_000_000).unwrap(),
            last_change_at: DateTime::from_timestamp_millis(rev_ms).unwrap(),
            items: vec![OrderItem {
                supplier_article: "ART-1".into(),
                tech_size: "M".into(),
                barcode: "2000000000000".into(),
                nm_id: 123,
                quantity: 1,
                price_kopecks: 10_000,
                total_price_kopecks: 9_000,
                discount_percent: 10,
            }],
            total_price_kopecks: 9_000,
            currency: "RUB".into(),
        }
    }

    #[test]
    fn board_column_str_contract() {
        // 7 колонок, строки = CHECK словарю миграции 0007 и API.
        let all: Vec<&str> = BoardColumn::ALL.iter().map(|c| c.as_str()).collect();
        assert_eq!(
            all,
            vec![
                "new",
                "assembling",
                "shipping",
                "in_transit",
                "delivered",
                "returned",
                "risk"
            ]
        );
    }

    #[test]
    fn idem_key_is_deterministic_and_revision_sensitive() {
        let a = order(1_735_000_000_000, "1001");
        let b = order(1_735_000_000_000, "1001");
        assert_eq!(a.idem_key(), b.idem_key());
        // другая ревизия → другой ключ (событие не путается с дублем)
        let c = order(1_735_000_000_500, "1001");
        assert_ne!(a.idem_key(), c.idem_key());
        // другой внешний id → другой ключ
        let d = order(1_735_000_000_000, "1002");
        assert_ne!(a.idem_key(), d.idem_key());
        // форма ключа: 64 hex-символа
        assert_eq!(a.idem_key().len(), 64);
        assert!(a.idem_key().bytes().all(|b| b.is_ascii_hexdigit()));
    }

    #[test]
    fn revision_is_monotonic_marker() {
        let a = order(1_735_000_000_000, "1");
        let newer = order(1_735_000_000_999, "1");
        assert!(newer.revision() > a.revision());
    }

    #[test]
    fn wb_mapping_structural_branches() {
        // отмена → риск (приоритетнее finished)
        assert_eq!(
            wb_board_column(WbStatusInput {
                code: 0,
                is_cancel: true,
                finished: true
            }),
            BoardColumn::Risk
        );
        // завершён → доставлен
        assert_eq!(
            wb_board_column(WbStatusInput {
                code: 0,
                is_cancel: false,
                finished: true
            }),
            BoardColumn::Delivered
        );
        // незнакомый код (таблица пуста, [ASSUMPTION]) → риск, не теряем
        assert_eq!(
            wb_board_column(WbStatusInput {
                code: 42,
                is_cancel: false,
                finished: false
            }),
            BoardColumn::Risk
        );
    }

    #[test]
    fn wb_mapping_dictionary_wins_when_filled() {
        // Механика словаря проверяется на временно заполненной копии
        // (const-таблица в проде пуста — [ASSUMPTION] до живого токена):
        // тот же алгоритм, что в wb_board_column, поверх непустого словаря.
        let dict: &[(i64, BoardColumn)] = &[(5, BoardColumn::InTransit)];
        let input = WbStatusInput {
            code: 5,
            is_cancel: false,
            finished: false,
        };
        let mapped = if input.is_cancel {
            BoardColumn::Risk
        } else if input.finished {
            BoardColumn::Delivered
        } else {
            dict.iter()
                .find(|(c, _)| *c == input.code)
                .map(|(_, col)| *col)
                .unwrap_or(BoardColumn::Risk)
        };
        assert_eq!(mapped, BoardColumn::InTransit);
        // код вне словаря → риск
        let input2 = WbStatusInput {
            code: 77,
            is_cancel: false,
            finished: false,
        };
        let mapped2 = dict
            .iter()
            .find(|(c, _)| *c == input2.code)
            .map(|(_, col)| *col)
            .unwrap_or(BoardColumn::Risk);
        assert_eq!(mapped2, BoardColumn::Risk);
    }

    fn row(order_id: i64, date_ms: i64, change_ms: i64, income: i64) -> crate::ports::WbOrderRow {
        crate::ports::WbOrderRow {
            date: DateTime::from_timestamp_millis(date_ms).unwrap(),
            last_change_date: DateTime::from_timestamp_millis(change_ms).unwrap(),
            supplier_article: format!("ART-{order_id}"),
            tech_size: "M".into(),
            barcode: "2000000000000".into(),
            discount_percent: 10,
            is_cancel: false,
            income_id: income,
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

    #[test]
    fn aggregation_groups_lines_and_sums() {
        let rows = vec![
            row(2, 1_000, 1_500, 0),
            row(1, 1_000, 1_200, 555),
            row(2, 1_000, 1_700, 0),
            row(1, 1_000, 1_300, 555),
        ];
        let orders = aggregate_wb_rows(Uuid::nil(), rows);
        assert_eq!(orders.len(), 2);
        // BTreeMap: детерминированный порядок по order_id
        assert_eq!(orders[0].external_order_id, "1");
        assert_eq!(orders[1].external_order_id, "2");
        // схема: income_id>0 → FBO, 0 → FBS ([ASSUMPTION] эвристика)
        assert_eq!(orders[0].scheme, Scheme::Fbo);
        assert_eq!(orders[1].scheme, Scheme::Fbs);
        // две позиции в заказе 1, сумма totalPrice
        assert_eq!(orders[0].items.len(), 2);
        assert_eq!(orders[0].total_price_kopecks, 18_000);
        // ревизия заказа — максимум lastChangeDate его строк
        assert_eq!(orders[1].revision(), 1_700);
        // код статуса — от последней по времени строки
        assert_eq!(orders[1].status_code, 3);
    }

    #[test]
    fn aggregation_cancel_and_finished_flags() {
        // одна строка отменена — весь заказ уходит в риск
        let mut cancelled = row(7, 1_000, 1_200, 42);
        cancelled.is_cancel = true;
        let orders = aggregate_wb_rows(Uuid::nil(), vec![cancelled]);
        assert_eq!(orders[0].board_column, BoardColumn::Risk);
        assert!(orders[0].is_cancelled);

        // завершён — доставлен (не отменён)
        let mut finished = row(8, 1_000, 1_200, 0);
        finished.finished = true;
        let orders = aggregate_wb_rows(Uuid::nil(), vec![finished]);
        assert_eq!(orders[0].board_column, BoardColumn::Delivered);

        // пустой вход — пустой выход (окно без заказов — норма)
        assert!(aggregate_wb_rows(Uuid::nil(), vec![]).is_empty());
    }
}
