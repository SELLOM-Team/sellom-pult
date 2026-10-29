//! RabbitMQ-слой воркеров (Story 1.4): топология quorum, издатель с
//! publisher confirms, консьюмер с per-consumer prefetch.
//!
//! Топология (спека 1.4: «одна очередь, изоляция — routing key», Ф0):
//! - exchange `pult.orders` (topic, durable) — routing key =
//!   `organization_id` (AD-4: маршрутизация уже в сообщении; per-org
//!   слоты/выборочные консьюмеры — AD-6 в Epic 2 добавляются без смены
//!   топологии);
//! - queue `pult.orders.v1` (quorum, durable, binding `#`) — фиксируем
//!   версию имени: смена контракта = новая очередь (`v2`), не миграция.
//!
//! Приоритеты: quorum-очереди RabbitMQ ≥ 4.3 поддерживают строгие
//! приоритеты 0–31 нативно (x-max-priority игнорируется, сообщения без
//! приоритета = 4; compose — RabbitMQ 4.4, верифицировано 29.10.2026).
//! События заказов публикуются приоритетом 31 («заказы первыми» — когда
//! появится низкоприоритетный трафик, разрыв уже заложен).
//!
//! Гарантии: quorum-репликация + `durable` + publisher confirms; ack
//! консьюмера — только после коммита проекции (at-least-once, AD-15).

use std::sync::Arc;

use futures_util::StreamExt;
use lapin::options::{
    BasicConsumeOptions, BasicPublishOptions, BasicQosOptions, ConfirmSelectOptions,
    ExchangeDeclareOptions, QueueBindOptions, QueueDeclareOptions,
};
use lapin::types::{AMQPValue, FieldTable};
use lapin::{BasicProperties, Channel, ExchangeKind};

use pult_core_sync::ports::{OrderEvent, OrderEventPublisher, OrderProjection};

/// Exchange конвейера заказов (topic, durable).
pub const EXCHANGE: &str = "pult.orders";
/// Quorum-очередь событий заказов (версия контракта в имени).
pub const QUEUE: &str = "pult.orders.v1";
/// Binding: консьюмер 1.4 читает всё (изоляция — routing key в сообщении).
pub const BINDING_ALL: &str = "#";
/// Приоритет публикации событий заказов (максимум quorum-словаря 0–31).
pub const ORDER_PRIORITY: u8 = 31;
/// AMQP delivery_mode: 2 = persistent (quorum хранит всегда — свойство
/// выставляем честно, равно как и durable-декларацию).
const DELIVERY_MODE_PERSISTENT: u8 = 2;
/// Prefetch консьюмера (per-consumer: глобальный QoS quorum не поддерживает).
pub const CONSUMER_PREFETCH: u16 = 16;

/// Ошибка lapin-издателя (публикация/подтверждения).
#[derive(Debug, thiserror::Error)]
pub enum PublisherError {
    /// Брокер отклонил публикацию или связь упала.
    #[error("публикация: {0}")]
    Publish(String),
    /// Брокер НЕ подтвердил событие (nack) — окно не дочекпоинчено.
    #[error("брокер не подтвердил события (nack): {0}")]
    Nack(String),
}

/// Объявляет топологию (идемпотентно: declare — объявления, не миграции).
pub async fn declare_topology(channel: &Channel) -> Result<(), lapin::Error> {
    channel
        .exchange_declare(
            EXCHANGE.into(),
            ExchangeKind::Topic,
            ExchangeDeclareOptions {
                durable: true,
                ..Default::default()
            },
            FieldTable::default(),
        )
        .await?;
    let mut queue_args = FieldTable::default();
    queue_args.insert("x-quorum-queue".into(), AMQPValue::Boolean(true));
    channel
        .queue_declare(
            QUEUE.into(),
            QueueDeclareOptions {
                durable: true,
                ..Default::default()
            },
            queue_args,
        )
        .await?;
    channel
        .queue_bind(
            QUEUE.into(),
            EXCHANGE.into(),
            BINDING_ALL.into(),
            QueueBindOptions::default(),
            FieldTable::default(),
        )
        .await?;
    Ok(())
}

/// lapin-издатель событий заказов (порт `OrderEventPublisher`): канал с
/// включённым confirm-mode; [`OrderEventPublisher::flush`] ждёт
/// подтверждения — барьер долговечности перед чекпоинтом окна.
pub struct LapinOrderPublisher {
    channel: Arc<Channel>,
}

impl LapinOrderPublisher {
    /// Включает publisher confirms (обязателен для at-least-once границы).
    pub async fn new(channel: Arc<Channel>) -> Result<Self, lapin::Error> {
        channel
            .confirm_select(ConfirmSelectOptions::default())
            .await?;
        Ok(Self { channel })
    }
}

impl OrderEventPublisher for LapinOrderPublisher {
    type Error = PublisherError;

    async fn publish(&self, event: &OrderEvent) -> Result<(), Self::Error> {
        let payload =
            crate::dto::event_to_json(event).map_err(|e| PublisherError::Publish(e.to_string()))?;
        let properties = BasicProperties::default()
            .with_content_type("application/json".into())
            .with_delivery_mode(DELIVERY_MODE_PERSISTENT)
            .with_priority(ORDER_PRIORITY)
            .with_message_id(event.event_id.to_string().into())
            .with_app_id("pult-workers".into());
        self.channel
            .basic_publish(
                EXCHANGE.into(),
                event.organization_id.to_string().into(),
                BasicPublishOptions::default(),
                &payload,
                properties,
            )
            .await
            .map_err(|e| PublisherError::Publish(e.to_string()))?;
        Ok(())
    }

    async fn flush(&self) -> Result<(), Self::Error> {
        // lapin 4.12: wait_for_confirms ждёт последний незакрытый confirm;
        // Err = nack/канал-сбой, Ok(returns) — вернувшиеся non-mandatory
        // публикации (наша топология биндит всё — returns быть не должно).
        self.channel
            .wait_for_confirms()
            .await
            .map(|_| ())
            .map_err(|e| {
                PublisherError::Nack(format!("брокер не подтвердил события (nack/канал): {e}"))
            })
    }
}

/// Консьюмер quorum-очереди: проекция → ack (после коммита) / requeue
/// (временный сбой проекции). Пауза на requeue — без горячего цикла
/// доставки (короткая очередь перезаберёт сообщение мгновенно).
pub async fn run_consumer<P>(
    channel: Arc<Channel>,
    projection: Arc<P>,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) -> Result<(), lapin::Error>
where
    P: OrderProjection,
    P::Error: std::fmt::Display,
{
    // Глобальный QoS quorum-очереди не поддерживают — только per-consumer.
    channel
        .basic_qos(CONSUMER_PREFETCH, BasicQosOptions { global: false })
        .await?;
    let mut deliveries = channel
        .basic_consume(
            QUEUE.into(),
            "pult-orders-v1-worker".into(),
            BasicConsumeOptions::default(),
            FieldTable::default(),
        )
        .await?;

    loop {
        tokio::select! {
            changed = shutdown.changed() => {
                if changed.is_err() {
                    tracing::warn!("канал shutdown закрыт преждевременно");
                }
                tracing::info!(
                    "graceful shutdown консьюмера: in-flight доставки вернутся в очередь \
                     (at-least-once), проекция идемпотентна (AD-15)"
                );
                return Ok(());
            }
            delivery = deliveries.next() => {
                match delivery {
                    Some(Ok(delivery)) => {
                        match crate::consumer::project_event(&delivery.data, projection.as_ref()).await {
                            crate::consumer::AckDecision::Ack => {
                                if let Err(err) = delivery.ack(lapin::options::BasicAckOptions::default()).await {
                                    tracing::error!(hint = %err, "ack не прошёл — доставка вернётся (at-least-once)");
                                }
                            }
                            crate::consumer::AckDecision::Requeue => {
                                tracing::warn!(
                                    tag = delivery.delivery_tag,
                                    "проекция не готова — requeue с паузой"
                                );
                                if let Err(err) = delivery
                                    .reject(
                                        lapin::options::BasicRejectOptions { requeue: true },
                                    )
                                    .await
                                {
                                    tracing::error!(hint = %err, "requeue не прошёл");
                                }
                                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                            }
                        }
                    }
                    Some(Err(err)) => {
                        tracing::warn!(hint = %err, "сбой потока доставок — пауза и продолжаем");
                        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                    }
                    None => {
                        tracing::warn!("поток доставок закрыт брокером — консьюмер завершён");
                        return Ok(());
                    }
                }
            }
        }
    }
}
