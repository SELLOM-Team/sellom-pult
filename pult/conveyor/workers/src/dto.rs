//! DTO событий конвейера (Story 1.4, AD-22) — serde-отражение канона.
//!
//! Сериализация — компоновочный слой конвейера (прецедент identity: домен
//! без serde). Издатель и консьюмер живут в одном крейте (контракт 1.4
//! самодостаточен); `schema_version` в конверте — совместимость при смене
//! словаря статусов (инкремент при заполнении `WB_STATUS_DICTIONARY`).
//!
//! Инварианты парсинга (консьюмер, at-least-once):
//! - битый JSON/неизвестные коды словарей → ошибка парсинга → консьюмер
//!   честно отбрасывает событие (ack) — навсегда-непарсируемое сообщение
//!   не должно блокировать очередь;
//! - `organization_id` конверта обязан совпадать с полем заказа — защита
//!   от рассинхрона маршрутизации (routing key AD-4) и payload.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use pult_core_identity::vault::{Marketplace, Scheme};
use pult_core_sync::orders::{BoardColumn, CanonicalOrder, OrderItem};
use pult_core_sync::ports::OrderEvent;

/// Ошибка конвертации DTO ↔ домен.
#[derive(Debug, thiserror::Error)]
pub enum DtoError {
    /// JSON не соответствует конверту (битое тело из очереди).
    #[error("событие не распарсено: {0}")]
    BadPayload(String),
    /// Код словаря вне канона (площадка/колонка/схема) — событие для ревью.
    #[error("код словаря вне канона: {0}")]
    UnknownCode(String),
    /// Конверт и payload рассинхронены по организации.
    #[error("organization_id конверта и заказа расходятся")]
    OrgMismatch,
}

/// DTO позиции заказа (AD-22; копейки — целые).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OrderItemDto {
    /// Артикул продавца (supplierArticle в WB).
    pub supplier_article: String,
    /// Размер (techSize).
    pub tech_size: String,
    /// Штрихкод.
    pub barcode: String,
    /// Артикул площадки (nmId).
    pub nm_id: i64,
    /// Количество.
    pub quantity: i64,
    /// Цена до скидки, копейки.
    pub price_kopecks: i64,
    /// Итоговая цена позиции, копейки.
    pub total_price_kopecks: i64,
    /// Скидка, %.
    pub discount_percent: i64,
}

/// DTO канонического заказа.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CanonicalOrderDto {
    pub organization_id: Uuid,
    pub marketplace: String,
    pub external_order_id: String,
    pub scheme: String,
    pub board_column: String,
    /// Сырой статус площадки (диагностика; нормализация уже применена).
    pub status_code: i64,
    pub is_cancelled: bool,
    pub warehouse_name: String,
    pub order_date: DateTime<Utc>,
    pub last_change_at: DateTime<Utc>,
    pub items: Vec<OrderItemDto>,
    pub total_price_kopecks: i64,
    pub currency: String,
}

/// DTO конверта события (AD-22).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OrderEventDto {
    pub event_id: Uuid,
    pub event_type: String,
    pub organization_id: Uuid,
    pub occurred_at: DateTime<Utc>,
    pub schema_version: u16,
    pub order: CanonicalOrderDto,
}

fn marketplace_from_code(raw: &str) -> Option<Marketplace> {
    match raw {
        "wildberries" => Some(Marketplace::Wildberries),
        "ozon" => Some(Marketplace::Ozon),
        _ => None,
    }
}

fn scheme_from_code(raw: &str) -> Option<Scheme> {
    match raw {
        "fbo" => Some(Scheme::Fbo),
        "fbs" => Some(Scheme::Fbs),
        "realfbs" => Some(Scheme::RealFbs),
        "rfbs" => Some(Scheme::Rfbs),
        _ => None,
    }
}

impl From<&OrderItem> for OrderItemDto {
    fn from(item: &OrderItem) -> Self {
        Self {
            supplier_article: item.supplier_article.clone(),
            tech_size: item.tech_size.clone(),
            barcode: item.barcode.clone(),
            nm_id: item.nm_id,
            quantity: item.quantity,
            price_kopecks: item.price_kopecks,
            total_price_kopecks: item.total_price_kopecks,
            discount_percent: item.discount_percent,
        }
    }
}

impl From<OrderItemDto> for OrderItem {
    fn from(dto: OrderItemDto) -> Self {
        Self {
            supplier_article: dto.supplier_article,
            tech_size: dto.tech_size,
            barcode: dto.barcode,
            nm_id: dto.nm_id,
            quantity: dto.quantity,
            price_kopecks: dto.price_kopecks,
            total_price_kopecks: dto.total_price_kopecks,
            discount_percent: dto.discount_percent,
        }
    }
}

impl From<&CanonicalOrder> for CanonicalOrderDto {
    fn from(order: &CanonicalOrder) -> Self {
        Self {
            organization_id: order.organization_id,
            marketplace: order.marketplace.as_str().to_owned(),
            external_order_id: order.external_order_id.clone(),
            scheme: order.scheme.as_str().to_owned(),
            board_column: order.board_column.as_str().to_owned(),
            status_code: order.status_code,
            is_cancelled: order.is_cancelled,
            warehouse_name: order.warehouse_name.clone(),
            order_date: order.order_date,
            last_change_at: order.last_change_at,
            items: order.items.iter().map(OrderItemDto::from).collect(),
            total_price_kopecks: order.total_price_kopecks,
            currency: order.currency.clone(),
        }
    }
}

impl TryFrom<CanonicalOrderDto> for CanonicalOrder {
    type Error = DtoError;

    fn try_from(dto: CanonicalOrderDto) -> Result<Self, Self::Error> {
        Ok(Self {
            organization_id: dto.organization_id,
            marketplace: marketplace_from_code(&dto.marketplace).ok_or_else(|| {
                DtoError::UnknownCode(format!("marketplace {0}", dto.marketplace))
            })?,
            external_order_id: dto.external_order_id,
            scheme: scheme_from_code(&dto.scheme)
                .ok_or_else(|| DtoError::UnknownCode(format!("scheme {0}", dto.scheme)))?,
            board_column: BoardColumn::from_code(&dto.board_column).ok_or_else(|| {
                DtoError::UnknownCode(format!("board_column {0}", dto.board_column))
            })?,
            status_code: dto.status_code,
            is_cancelled: dto.is_cancelled,
            warehouse_name: dto.warehouse_name,
            order_date: dto.order_date,
            last_change_at: dto.last_change_at,
            items: dto.items.into_iter().map(OrderItem::from).collect(),
            total_price_kopecks: dto.total_price_kopecks,
            currency: dto.currency,
        })
    }
}

/// Домен → DTO (издатель).
pub fn event_to_dto(event: &OrderEvent) -> OrderEventDto {
    OrderEventDto {
        event_id: event.event_id,
        event_type: event.event_type.to_owned(),
        organization_id: event.organization_id,
        occurred_at: event.occurred_at,
        schema_version: event.schema_version,
        order: CanonicalOrderDto::from(&event.order),
    }
}

/// DTO → домен (консьюмер): словари, тип события и согласованность
/// организации проверены.
///
/// Неизвестный `event_type` — событие для ревью (Forward-совместимость: в
/// 1.4 тип один; будущие консьюмеры расширяют match здесь). `&'static str`
/// берётся из константы канона — без утечки на каждое сообщение.
pub fn dto_to_event(dto: OrderEventDto) -> Result<OrderEvent, DtoError> {
    if dto.event_type != OrderEvent::EVENT_TYPE {
        return Err(DtoError::UnknownCode(format!(
            "event_type {0}",
            dto.event_type
        )));
    }
    let order = CanonicalOrder::try_from(dto.order)?;
    if order.organization_id != dto.organization_id {
        return Err(DtoError::OrgMismatch);
    }
    Ok(OrderEvent {
        event_id: dto.event_id,
        event_type: OrderEvent::EVENT_TYPE,
        organization_id: dto.organization_id,
        occurred_at: dto.occurred_at,
        schema_version: dto.schema_version,
        order,
    })
}

/// Домен → байты очереди (издатель).
pub fn event_to_json(event: &OrderEvent) -> Result<Vec<u8>, DtoError> {
    serde_json::to_vec(&event_to_dto(event)).map_err(|e| DtoError::BadPayload(e.to_string()))
}

/// Байты очереди → домен (консьюмер).
pub fn parse_event(payload: &[u8]) -> Result<OrderEvent, DtoError> {
    let dto: OrderEventDto =
        serde_json::from_slice(payload).map_err(|e| DtoError::BadPayload(e.to_string()))?;
    dto_to_event(dto)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use pult_core_identity::vault::Marketplace;
    use pult_core_sync::orders::BoardColumn;

    fn sample_order(external: &str) -> CanonicalOrder {
        CanonicalOrder {
            organization_id: Uuid::nil(),
            marketplace: Marketplace::Wildberries,
            external_order_id: external.to_owned(),
            scheme: Scheme::Fbo,
            board_column: BoardColumn::New,
            status_code: 3,
            is_cancelled: false,
            warehouse_name: "Коледино".into(),
            order_date: Utc.timestamp_millis_opt(1_700_000_000_000).unwrap(),
            last_change_at: Utc.timestamp_millis_opt(1_700_000_500_000).unwrap(),
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

    fn sample_event(external: &str) -> OrderEvent {
        OrderEvent::new(
            sample_order(external),
            Utc.timestamp_millis_opt(1_700_001_000_000).unwrap(),
        )
    }

    #[test]
    fn dto_round_trip_preserves_domain() {
        let event = sample_event("771");
        let bytes = event_to_json(&event).expect("сериализация валидного события");
        let back = parse_event(&bytes).expect("round-trip парсится");
        assert_eq!(back, event);
    }

    #[test]
    fn parse_rejects_garbage() {
        assert!(matches!(
            parse_event(b"{\"error\":true}"),
            Err(DtoError::BadPayload(_))
        ));
        assert!(matches!(
            parse_event(b"not-json"),
            Err(DtoError::BadPayload(_))
        ));
    }

    #[test]
    fn parse_rejects_unknown_dictionary_codes() {
        let event = sample_event("1");
        let mut dto = event_to_dto(&event);
        dto.order.board_column = "super-new".into();
        let bytes = serde_json::to_vec(&dto).expect("dto сериализуется");
        assert!(matches!(parse_event(&bytes), Err(DtoError::UnknownCode(_))));
    }

    #[test]
    fn parse_rejects_unknown_event_type() {
        let event = sample_event("4");
        let mut dto = event_to_dto(&event);
        dto.event_type = "order.deleted".into();
        let bytes = serde_json::to_vec(&dto).expect("dto сериализуется");
        assert!(matches!(parse_event(&bytes), Err(DtoError::UnknownCode(_))));
    }

    #[test]
    fn parse_rejects_org_mismatch() {
        let event = sample_event("2");
        let mut dto = event_to_dto(&event);
        dto.organization_id = Uuid::now_v7();
        let bytes = serde_json::to_vec(&dto).unwrap();
        assert!(matches!(parse_event(&bytes), Err(DtoError::OrgMismatch)));
    }

    #[test]
    fn schema_version_travels_in_envelope() {
        let event = sample_event("3");
        let dto = event_to_dto(&event);
        assert_eq!(dto.schema_version, OrderEvent::SCHEMA_VERSION);
        assert_eq!(dto.event_type, OrderEvent::EVENT_TYPE);
    }
}
