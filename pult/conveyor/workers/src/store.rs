//! sqlx-сторы воркеров: закрытие портов `OrderProjection` и
//! `SyncCheckpointStore` (Story 1.4, AD-15 / AD-3) + опрос организаций,
//! которым нужен синк (verified-WB без завершённого цикла).
//!
//! Паттерны 1.3 (AD-16-гигиена и шлюз-прецеденты):
//! - timestamptz в/из SQL — через epoch-bigint (`EXTRACT`/`TO_TIMESTAMP`) —
//!   chrono-фича sqlx не включена (прецедент шлюза);
//! - токены НЕ читаются plaintext-ом в этой сторе: `due_verified_wb`
//!   отдаёт ШИФРОТЕКСТ; расшифровка — `VaultTokenCipher` в `main`
//!   (окно записи — шлюз, окно чтения — конвейер, AD-16);
//! - SQL — runtime `sqlx::query` (без compile-time макросов: без
//!   DATABASE_URL в сборке, прецедент шлюза).
//!
//! Идемпотентная проекция (`orders` UNIQUE (organization_id, idem_key) +
//! UNIQUE (organization_id, marketplace, external_order_id), ревизионный
//! барьер — миграция 0007):
//! - INSERT … ON CONFLICT DO NOTHING: строка вставлена → позиции →
//!   [`UpsertOutcome::Inserted`];
//! - конфликт: SELECT … FOR UPDATE ревизии → новая ревизия = guarded UPDATE
//!   (WHERE revision < новой) + перезапись позиций → [`UpsertOutcome::Updated`];
//!   та же/старая ревизия → [`UpsertOutcome::NoOp`] (at-least-once погашен).

use chrono::{DateTime, Utc};
use uuid::Uuid;

use pult_core_sync::orders::CanonicalOrder;
use pult_core_sync::ports::{
    CycleType, OrderProjection, RunStatus, SyncCheckpointStore, SyncRunState, UpsertOutcome,
};

/// Ошибка sqlx-стор воркера.
#[derive(Debug, thiserror::Error)]
pub enum WorkerStoreError {
    /// Сбой операции БД (сеть/схема).
    #[error("сбой БД воркера: {0}")]
    Db(String),
    /// Данные строки не соответствуют канону (CHECK-словари) — диагностика.
    #[error("повреждённые данные синка: {0}")]
    Corrupt(&'static str),
}

impl From<sqlx::Error> for WorkerStoreError {
    fn from(err: sqlx::Error) -> Self {
        WorkerStoreError::Db(err.to_string())
    }
}

/// SQL-представление границ окна (epoch-ms → double → timestamptz).
fn epoch_secs(ts: DateTime<Utc>) -> f64 {
    ts.timestamp() as f64 + ts.timestamp_subsec_nanos() as f64 / 1e9
}

// ── чекпоинт-стора ──────────────────────────────────────────────────────────

/// Строка `sync_runs` (status, completed_until, окна, счётчики, hint).
type SyncRunRow = (String, Option<String>, i32, i32, i64, i64, Option<String>);

/// PG-закрытие порта [`SyncCheckpointStore`] (строка `sync_runs`).
#[derive(Clone)]
pub struct PgSyncCheckpointStore {
    db: sqlx::PgPool,
}

impl PgSyncCheckpointStore {
    pub fn new(db: sqlx::PgPool) -> Self {
        Self { db }
    }
}

impl SyncCheckpointStore for PgSyncCheckpointStore {
    type Error = WorkerStoreError;

    async fn load(
        &self,
        organization_id: Uuid,
        cycle: CycleType,
    ) -> Result<Option<SyncRunState>, Self::Error> {
        let row: Option<SyncRunRow> = sqlx::query_as(
            "SELECT status, checkpoint->>'completed_until', windows_total, \
                    windows_done, orders_fetched, events_published, error_hint \
             FROM sync_runs \
             WHERE organization_id = $1 AND cycle_type = $2",
        )
        .bind(organization_id)
        .bind(cycle.as_str())
        .fetch_optional(&self.db)
        .await?;

        let Some((status_raw, completed_raw, total, done, fetched, published, hint)) = row else {
            return Ok(None);
        };
        let status = RunStatus::from_code(&status_raw)
            .ok_or(WorkerStoreError::Corrupt("status вне CHECK-словаря"))?;
        let completed_until = match completed_raw {
            Some(raw) if !raw.is_empty() => Some(
                DateTime::parse_from_rfc3339(&raw)
                    .map_err(|_| WorkerStoreError::Corrupt("completed_until не RFC3339"))?
                    .with_timezone(&Utc),
            ),
            _ => None,
        };
        if total < 0 || done < 0 {
            return Err(WorkerStoreError::Corrupt("отрицательные счётчики окон"));
        }
        Ok(Some(SyncRunState {
            organization_id,
            cycle,
            status,
            completed_until,
            windows_total: total as u32,
            windows_done: done as u32,
            orders_fetched: fetched.max(0) as u64,
            events_published: published.max(0) as u64,
            error_hint: hint,
        }))
    }

    async fn save(&self, state: &SyncRunState) -> Result<(), Self::Error> {
        // Чекпоинт AD-3: jsonb-граница продолжения (сбоя не теряет окна).
        let checkpoint = match state.completed_until {
            Some(ts) => serde_json::json!({ "completed_until": ts.to_rfc3339() }).to_string(),
            None => "{}".to_string(),
        };
        sqlx::query(
            "INSERT INTO sync_runs \
                 (organization_id, cycle_type, status, checkpoint, windows_total, \
                  windows_done, orders_fetched, events_published, error_hint, \
                  started_at, finished_at) \
             VALUES ($1, $2, $3, $4::jsonb, $5, $6, $7, $8, $9, \
                     CASE WHEN $3 = 'running' THEN now() END, \
                     CASE WHEN $3 IN ('succeeded', 'failed') THEN now() END) \
             ON CONFLICT (organization_id, cycle_type) DO UPDATE SET \
                 status = EXCLUDED.status, \
                 checkpoint = EXCLUDED.checkpoint, \
                 windows_total = EXCLUDED.windows_total, \
                 windows_done = EXCLUDED.windows_done, \
                 orders_fetched = EXCLUDED.orders_fetched, \
                 events_published = EXCLUDED.events_published, \
                 error_hint = EXCLUDED.error_hint, \
                 started_at = COALESCE(sync_runs.started_at, EXCLUDED.started_at), \
                 finished_at = EXCLUDED.finished_at",
        )
        .bind(state.organization_id)
        .bind(state.cycle.as_str())
        .bind(state.status.as_str())
        .bind(&checkpoint)
        .bind(state.windows_total as i32)
        .bind(state.windows_done as i32)
        .bind(state.orders_fetched as i64)
        .bind(state.events_published as i64)
        .bind(state.error_hint.as_deref())
        .execute(&self.db)
        .await?;
        Ok(())
    }
}

// ── проекция заказов ────────────────────────────────────────────────────────

/// PG-закрытие порта [`OrderProjection`] (таблицы `orders`/`order_items`).
#[derive(Clone)]
pub struct PgOrderProjection {
    db: sqlx::PgPool,
}

impl PgOrderProjection {
    pub fn new(db: sqlx::PgPool) -> Self {
        Self { db }
    }
}

async fn insert_items(
    tx: &mut sqlx::PgConnection,
    order_id: Uuid,
    order: &CanonicalOrder,
) -> Result<(), WorkerStoreError> {
    for item in &order.items {
        sqlx::query(
            "INSERT INTO order_items \
                 (order_id, organization_id, nm_id, supplier_article, tech_size, \
                  barcode, quantity, price, total_price, discount_percent) \
             VALUES ($1, $2, $3, $4, $5, $6, ($7::bigint)::int, \
                     ($8::bigint)::numeric/100, ($9::bigint)::numeric/100, ($10::bigint)::int)",
        )
        .bind(order_id)
        .bind(order.organization_id)
        .bind(item.nm_id)
        .bind(&item.supplier_article)
        .bind(&item.tech_size)
        .bind(&item.barcode)
        .bind(item.quantity)
        .bind(item.price_kopecks)
        .bind(item.total_price_kopecks)
        .bind(item.discount_percent)
        .execute(&mut *tx)
        .await?;
    }
    Ok(())
}

impl OrderProjection for PgOrderProjection {
    type Error = WorkerStoreError;

    async fn upsert(&self, order: &CanonicalOrder) -> Result<UpsertOutcome, Self::Error> {
        let mut tx = self.db.begin().await?;

        // 1. Вставка заказа; конфликт (idem-дубль ИЛИ существующий заказ) —
        //    далее ревизионный барьер. platform_payload — NULL: сырой ответ
        //    не входит в конверт AD-22 (сырой статус хранится в status_code;
        //    перенос сырой строки в события — ревью при заполнении словаря).
        let inserted: Option<Uuid> = sqlx::query_scalar(
            "INSERT INTO orders \
                 (organization_id, marketplace, external_order_id, idem_key, revision, \
                  scheme, board_column, status_code, warehouse_name, total_price, \
                  currency, is_cancelled, order_date, last_change_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, ($10::bigint)::numeric/100, \
                     $11, $12, TO_TIMESTAMP($13), TO_TIMESTAMP($14)) \
             ON CONFLICT DO NOTHING \
             RETURNING id",
        )
        .bind(order.organization_id)
        .bind(order.marketplace.as_str())
        .bind(&order.external_order_id)
        .bind(order.idem_key())
        .bind(order.revision())
        .bind(order.scheme.as_str())
        .bind(order.board_column.as_str())
        .bind(order.status_code.to_string())
        .bind(&order.warehouse_name)
        .bind(order.total_price_kopecks)
        .bind(&order.currency)
        .bind(order.is_cancelled)
        .bind(epoch_secs(order.order_date))
        .bind(epoch_secs(order.last_change_at))
        .fetch_optional(&mut *tx)
        .await?;

        if let Some(order_id) = inserted {
            insert_items(&mut tx, order_id, order).await?;
            tx.commit().await?;
            return Ok(UpsertOutcome::Inserted);
        }

        // 2. Конфликт: существующая строка (FOR UPDATE — сериализация
        //    гонки двух проекций одного заказа).
        let existing: Option<(Uuid, i64)> = sqlx::query_as(
            "SELECT id, revision FROM orders \
             WHERE organization_id = $1 AND marketplace = $2 AND external_order_id = $3 \
             FOR UPDATE",
        )
        .bind(order.organization_id)
        .bind(order.marketplace.as_str())
        .bind(&order.external_order_id)
        .fetch_optional(&mut *tx)
        .await?;

        let Some((order_id, stored_revision)) = existing else {
            // idem-ключ чужого заказа не может совпасть по построению
            // (материал включает внешний id); сюда попадаем только при
            // нарушении инварианта БД — честная ошибка, ретрай бессмыслен.
            tx.rollback().await.ok();
            return Err(WorkerStoreError::Corrupt(
                "конфликт idem-ключа без строки заказа (инвариант AD-15 нарушен)",
            ));
        };
        if stored_revision >= order.revision() {
            // Дубль доставки (та же ревизия) или откат (старая) — NO-OP.
            tx.commit().await?;
            return Ok(UpsertOutcome::NoOp);
        }

        // 3. Новая ревизия: guarded UPDATE (барьер откатов + гонки).
        let updated = sqlx::query(
            "UPDATE orders SET \
                 idem_key = $4, revision = $5, scheme = $6, board_column = $7, \
                 status_code = $8, warehouse_name = $9, total_price = ($10::bigint)::numeric/100, \
                 currency = $11, is_cancelled = $12, order_date = TO_TIMESTAMP($13), \
                 last_change_at = TO_TIMESTAMP($14) \
             WHERE organization_id = $1 AND marketplace = $2 AND external_order_id = $3 \
               AND revision < $5",
        )
        .bind(order.organization_id)
        .bind(order.marketplace.as_str())
        .bind(&order.external_order_id)
        .bind(order.idem_key())
        .bind(order.revision())
        .bind(order.scheme.as_str())
        .bind(order.board_column.as_str())
        .bind(order.status_code.to_string())
        .bind(&order.warehouse_name)
        .bind(order.total_price_kopecks)
        .bind(&order.currency)
        .bind(order.is_cancelled)
        .bind(epoch_secs(order.order_date))
        .bind(epoch_secs(order.last_change_at))
        .execute(&mut *tx)
        .await?;

        if updated.rows_affected() == 0 {
            // Гонка: конкурент применил не-меньшую ревизию между SELECT и
            // UPDATE — барьер отработал, откат запрещён.
            tx.commit().await?;
            return Ok(UpsertOutcome::NoOp);
        }

        // 4. Позиции заказа = функции ревизии: перезапись (мелкий батч,
        //    составный FK держит целостность).
        sqlx::query(
            "DELETE FROM order_items \
             WHERE order_id = $1 AND organization_id = $2",
        )
        .bind(order_id)
        .bind(order.organization_id)
        .execute(&mut *tx)
        .await?;
        insert_items(&mut tx, order_id, order).await?;

        tx.commit().await?;
        Ok(UpsertOutcome::Updated)
    }
}

// ── планировщик: организации, которым нужен прогон ──────────────────────────

/// Организация с verified-WB, ожидающая синка: токен — ШИФРОТЕКСТ.
pub struct DueWbOrg {
    pub organization_id: Uuid,
    /// Зашифрованный токен (bytea `marketplace_connections.token_ciphertext`).
    pub token_ciphertext: Vec<u8>,
    /// Nonce AES-GCM (bytea `token_nonce`).
    pub token_nonce: Vec<u8>,
    /// Версия мастер-ключа (`key_version`; 1 — Ф0, ротация deferred).
    pub key_version: i32,
}

/// Verified-WB организации без завершённого/терминально-здорового цикла:
/// нет строки `sync_runs`, либо pending/running (stale после падения
/// воркера; liveness-детект — Epic 2, Ф0 один воркер)/needs_retry.
pub async fn due_verified_wb(db: &sqlx::PgPool) -> Result<Vec<DueWbOrg>, WorkerStoreError> {
    let rows: Vec<(Uuid, Vec<u8>, Vec<u8>, i32)> = sqlx::query_as(
        "SELECT c.organization_id, c.token_ciphertext, c.token_nonce, c.key_version \
         FROM marketplace_connections c \
         LEFT JOIN sync_runs r \
           ON r.organization_id = c.organization_id AND r.cycle_type = 'initial_orders' \
         WHERE c.marketplace = 'wildberries' AND c.status = 'verified' \
           AND (r.id IS NULL OR r.status IN ('pending', 'running', 'needs_retry'))",
    )
    .fetch_all(db)
    .await?;
    Ok(rows
        .into_iter()
        .map(
            |(organization_id, token_ciphertext, token_nonce, key_version)| DueWbOrg {
                organization_id,
                token_ciphertext,
                token_nonce,
                key_version,
            },
        )
        .collect())
}
