//! Консьюмер событий заказов (Story 1.4, AD-15): тело доставки → проекция
//! → решение ack/requeue.
//!
//! Политика at-least-once:
//! - проекция успешна (`Inserted`/`Updated`/`NoOp` — дубль погашен) →
//!   **ack**: полезная работа есть в любом исходе, включая NO-OP;
//! - проекция упала (PG недоступна) → **requeue** — событие вернётся,
//!   идемпотентность гарантирует безопасность повтора; цикл доставки
//!   ставится на короткую паузу (без горячего цикла);
//! - тело не парсится навсегда (битый JSON, код вне словарей) → **ack +
//!   error-лог**: сообщение, которое никогда не станет валидным, не имеет
//!   права блокировать очередь (deadletter-очередь — deferred, Epic 2+).
//!
//! Секретов в событии нет по построению (токен не покидает адаптер), но
//! payload в логи не пишется — только его длина и идентификаторы.

use pult_core_sync::ports::{OrderProjection, UpsertOutcome};

/// Решение по доставке после обработки.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AckDecision {
    /// Подтвердить (проекция завершена или сообщение неисправимо битое).
    Ack,
    /// Вернуть в очередь (временный сбой — PG недоступна).
    Requeue,
}

/// Обрабатывает одно сообщение очереди: парсинг → проекция → решение.
pub async fn project_event<P>(payload: &[u8], projection: &P) -> AckDecision
where
    P: OrderProjection,
    P::Error: std::fmt::Display,
{
    let event = match crate::dto::parse_event(payload) {
        Ok(event) => event,
        Err(err) => {
            // Неисправимо: никогда не спарсится — не блокируем очередь.
            // Событие для ревью (байт-длина достаточна, payload не логируем).
            tracing::error!(
                bytes = payload.len(),
                hint = %err,
                "битое событие отброшено (ack): очередь не блокируем"
            );
            return AckDecision::Ack;
        }
    };
    match projection.upsert(&event.order).await {
        Ok(outcome) => {
            if outcome == UpsertOutcome::NoOp {
                tracing::debug!(
                    order = %event.order.external_order_id,
                    "дубль события погашен проекцией (NO-OP, AD-15)"
                );
            }
            AckDecision::Ack
        }
        Err(err) => {
            tracing::warn!(
                order = %event.order.external_order_id,
                hint = %err,
                "проекция не применена — requeue (at-least-once)"
            );
            AckDecision::Requeue
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::Mutex;

    use chrono::{TimeZone, Utc};
    use pult_core_identity::vault::Marketplace;
    use pult_core_sync::orders::{BoardColumn, CanonicalOrder, OrderItem};
    use pult_core_sync::ports::OrderEvent;

    /// Проекция с управляемым исходом (Ok>NoOp либо Err).
    struct ScriptedProjection {
        error: Option<&'static str>,
        calls: Mutex<usize>,
    }

    impl OrderProjection for ScriptedProjection {
        type Error = String;

        async fn upsert(&self, _order: &CanonicalOrder) -> Result<UpsertOutcome, Self::Error> {
            *self.calls.lock().unwrap() += 1;
            match self.error {
                Some(msg) => Err(msg.to_owned()),
                None => Ok(UpsertOutcome::NoOp),
            }
        }
    }

    fn order_bytes(external: &str) -> Vec<u8> {
        let order = CanonicalOrder {
            organization_id: uuid::Uuid::nil(),
            marketplace: Marketplace::Wildberries,
            external_order_id: external.to_owned(),
            scheme: pult_core_identity::vault::Scheme::Fbo,
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
        };
        let event = OrderEvent::new(order, Utc.timestamp_millis_opt(1_700_001_000_000).unwrap());
        crate::dto::event_to_json(&event).expect("событие сериализуется")
    }

    #[tokio::test]
    async fn ack_on_noop_duplicate() {
        let projection = ScriptedProjection {
            error: None,
            calls: Mutex::new(0),
        };
        assert_eq!(
            project_event(&order_bytes("771"), &projection).await,
            AckDecision::Ack
        );
        // дубль события — тот же payload: проекция идемпотентна → ack
        assert_eq!(
            project_event(&order_bytes("771"), &projection).await,
            AckDecision::Ack
        );
        assert_eq!(*projection.calls.lock().unwrap(), 2);
    }

    #[tokio::test]
    async fn requeue_on_transient_projection_error() {
        let projection = ScriptedProjection {
            error: Some("PG недоступна"),
            calls: Mutex::new(0),
        };
        assert_eq!(
            project_event(&order_bytes("772"), &projection).await,
            AckDecision::Requeue
        );
    }

    #[tokio::test]
    async fn ack_on_unparseable_payload() {
        let projection = ScriptedProjection {
            error: None,
            calls: Mutex::new(0),
        };
        assert_eq!(
            project_event(b"{\"not-an-event\":true}", &projection).await,
            AckDecision::Ack
        );
        assert_eq!(
            project_event(b"\xff\xfe garbage", &projection).await,
            AckDecision::Ack
        );
        assert_eq!(
            *projection.calls.lock().unwrap(),
            0,
            "проекция не вызывалась"
        );
    }
}
