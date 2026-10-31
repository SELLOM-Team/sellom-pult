//! Композиционный корень воркеров Story 1.4 (`pult-workers`).
//!
//! Роли бинарника (AD-23 — stateless, горизонт — за счёт конкурентных
//! реплик на очереди; Ф0 — один инстанс):
//! - **планировщик синка**: опрос verified-WB организаций → расшифровка
//!   токена (`VaultTokenCipher`, AD-16: окно чтения vault — конвейер) →
//!   `WbHttpOrdersSource` → прогон `initial_orders` (окна, события,
//!   чекпоинты AD-3);
//! - **консьюмер**: quorum-очередь → идемпотентная проекция в PG → ack
//!   после коммита (AD-15).
//!
//! Graceful shutdown: SIGINT/SIGTERM → каналы задач получают сигнал →
//! in-flight доставка возвращается в очередь (at-least-once), чекпоинт
//! последнего окна уже зафиксирован (AD-3) — рестарт продолжит с него.
//!
//! Конфигурация: общая — через [`pult_platform::config::ServiceConfig`]
//! (`DATABASE_URL`, `PULT_VAULT_MASTER_KEY`); воркерская —
//! [`pult_platform::config::WorkerConfig`] (полная таблица переменных —
//! в его доке: `PULT_RABBITMQ_URL`, `PULT_WB_STAT_BASE`, тюнинг WB-запросов
//! и циклов синка; AR-4: значения — константы окружения, не код).
//!
//! Секреты: мастер-ключ — из окружения; расшифрованный токен существует
//! только внутри `process_org` (не логируется, не хранится).

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use lapin::{Connection, ConnectionProperties};
use sqlx::postgres::PgPoolOptions;
use tokio::sync::watch;

use pult_conveyor_adapters::cipher::VaultTokenCipher;
use pult_conveyor_adapters::wb::WbHttpOrdersSource;
use pult_core_identity::ports::{EncryptedPayload, MasterKey, TokenCipher};
use pult_platform::config::{ServiceConfig, WorkerConfig};

use pult_conveyor_workers::amqp;
use pult_conveyor_workers::orchestrator::{self, SyncTuning};
use pult_conveyor_workers::store::{self, PgOrderProjection, PgSyncCheckpointStore};

async fn wait_for_postgres(db: &sqlx::PgPool) -> Result<(), String> {
    for attempt in 1..=30 {
        match sqlx::query("SELECT 1").execute(db).await {
            Ok(_) => return Ok(()),
            Err(err) => {
                tracing::warn!(attempt, hint = %err, "PostgreSQL ещё не готов — ждём");
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        }
    }
    Err("PostgreSQL недоступна после 30 попыток подключения".into())
}

async fn wait_for_rabbitmq(
    url: &str,
    mut shutdown: watch::Receiver<bool>,
) -> Result<Connection, String> {
    for attempt in 1..=30 {
        if *shutdown.borrow() {
            return Err("shutdown до подключения брокера".into());
        }
        match Connection::connect(url, ConnectionProperties::default()).await {
            Ok(conn) => return Ok(conn),
            Err(err) => {
                tracing::warn!(attempt, hint = %err, "RabbitMQ ещё не готов — ждём");
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_secs(2)) => {}
                    _ = shutdown.changed() => return Err("shutdown до подключения брокера".into()),
                }
            }
        }
    }
    Err("RabbitMQ недоступен после 30 попыток подключения".into())
}

/// Прогон синка одной организации: расшифровка → источник → цикл.
///
/// Токен живёт только здесь: расшифрованные байты → `String` источника;
/// ошибки расшифровки — данные (не транзиент): честный error-лог, орг
/// пропускается до чиники данных (ротация ключей/повреждённый ciphertext).
async fn process_org(
    org: store::DueWbOrg,
    master_key: &MasterKey,
    cfg: &WorkerConfig,
    publisher: &amqp::LapinOrderPublisher,
    checkpoints: &PgSyncCheckpointStore,
) {
    if org.key_version != 1 {
        tracing::error!(
            organization_id = %org.organization_id,
            key_version = org.key_version,
            "версия мастер-ключа не поддерживается (ротация — deferred): организация пропущена"
        );
        return;
    }
    let payload = EncryptedPayload {
        ciphertext: org.token_ciphertext,
        nonce: org.token_nonce,
        key_version: u32::try_from(org.key_version).unwrap_or(0),
    };
    let cipher = VaultTokenCipher::new(master_key);
    let token = match cipher.decrypt(&payload) {
        Ok(bytes) => match String::from_utf8(bytes) {
            Ok(token) => token,
            Err(_) => {
                tracing::error!(
                    organization_id = %org.organization_id,
                    "расшифрованный токен не UTF-8: данные vault повреждены — организация пропущена"
                );
                return;
            }
        },
        Err(err) => {
            tracing::error!(
                organization_id = %org.organization_id,
                hint = %err,
                "расшифровка токена не удалась (ключ/nonce): организация пропущена"
            );
            return;
        }
    };

    let source = WbHttpOrdersSource::new(
        &cfg.wb_stat_base,
        token,
        cfg.wb_timeout,
        cfg.wb_body_limit_bytes,
        cfg.wb_max_attempts,
    );
    let tuning = SyncTuning {
        window_days: cfg.window_days,
        history_since: cfg.history_since,
    };
    match orchestrator::run_initial_orders(
        org.organization_id,
        &source,
        publisher,
        checkpoints,
        &tuning,
        Utc::now(),
    )
    .await
    {
        Ok(state) => tracing::info!(
            organization_id = %org.organization_id,
            status = state.status.as_str(),
            windows_done = state.windows_done,
            windows_total = state.windows_total,
            orders = state.orders_fetched,
            events = state.events_published,
            hint = state.error_hint.as_deref().unwrap_or(""),
            "цикл синка завершён"
        ),
        Err(err) => tracing::error!(
            organization_id = %org.organization_id,
            hint = %err,
            "инфраструктурный сбой цикла (повтор — следующий тик планировщика)"
        ),
    }
}

async fn run_scheduler(
    db: sqlx::PgPool,
    master_key: MasterKey,
    cfg: WorkerConfig,
    publisher: Arc<amqp::LapinOrderPublisher>,
    checkpoints: Arc<PgSyncCheckpointStore>,
    mut shutdown: watch::Receiver<bool>,
) {
    loop {
        match store::due_verified_wb(&db).await {
            Ok(orgs) => {
                for org in orgs {
                    if *shutdown.borrow() {
                        break;
                    }
                    process_org(org, &master_key, &cfg, &publisher, &checkpoints).await;
                }
            }
            Err(err) => {
                tracing::warn!(hint = %err, "опрос организаций синка не прошёл");
            }
        }
        tokio::select! {
            _ = tokio::time::sleep(cfg.scheduler_interval) => {}
            _ = shutdown.changed() => {
                tracing::info!("graceful shutdown планировщика: чекпоинты зафиксированы (AD-3)");
                return;
            }
        }
    }
}

async fn wait_for_shutdown_signal(shutdown_tx: watch::Sender<bool>) {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let mut terminate = match signal(SignalKind::terminate()) {
            Ok(stream) => stream,
            Err(err) => {
                tracing::error!(hint = %err, "SIGTERM не подписан — остаёмся только на SIGINT");
                return wait_ctrl_c(shutdown_tx).await;
            }
        };
        tokio::select! {
            _ = wait_ctrl_c(shutdown_tx.clone()) => {}
            _ = terminate.recv() => {}
        }
    }
    #[cfg(not(unix))]
    {
        wait_ctrl_c(shutdown_tx).await;
    }
    let _ = shutdown_tx.send(true);
}

async fn wait_ctrl_c(shutdown_tx: watch::Sender<bool>) {
    if tokio::signal::ctrl_c().await.is_ok() {
        tracing::info!("SIGINT: graceful shutdown");
        let _ = shutdown_tx.send(true);
    }
}

fn exit_with_config_error(context: &str, err: impl std::fmt::Display) -> ! {
    eprintln!("pult-workers: конфигурация ({context}): {err}");
    std::process::exit(1);
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let config = match ServiceConfig::from_env() {
        Ok(config) => config,
        Err(err) => exit_with_config_error("общая", err),
    };
    let worker_config = match WorkerConfig::from_env(config.environment) {
        Ok(config) => config,
        Err(err) => exit_with_config_error("воркерская", err),
    };

    // PG: ленивый пул + ожидание готовности (состязание старта compose),
    // затем миграции (идемпотентны; advisory-lock не даёт гонки с шлюзом).
    let db = match PgPoolOptions::new()
        .max_connections(5)
        .connect_lazy(&config.database_url)
    {
        Ok(pool) => pool,
        Err(err) => exit_with_config_error("DATABASE_URL", err),
    };
    if let Err(err) = wait_for_postgres(&db).await {
        exit_with_config_error("PostgreSQL", err);
    }
    let migrator = sqlx::migrate!("../../db/migrations");
    if let Err(err) = migrator.run(&db).await {
        exit_with_config_error("миграции", err);
    }
    tracing::info!("миграции применены");

    // RabbitMQ: подключение с ожиданием готовности, каналы на роль.
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let connection = match wait_for_rabbitmq(&worker_config.rabbitmq_url, shutdown_rx.clone()).await
    {
        Ok(conn) => conn,
        Err(err) => exit_with_config_error("RabbitMQ", err),
    };
    let publisher_channel = match connection.create_channel().await {
        Ok(channel) => Arc::new(channel),
        Err(err) => exit_with_config_error("канал издателя", err),
    };
    let consumer_channel = match connection.create_channel().await {
        Ok(channel) => Arc::new(channel),
        Err(err) => exit_with_config_error("канал консьюмера", err),
    };
    if let Err(err) = amqp::declare_topology(&consumer_channel).await {
        exit_with_config_error("топология AMQP", err);
    }
    let publisher = match amqp::LapinOrderPublisher::new(publisher_channel).await {
        Ok(publisher) => publisher,
        Err(err) => exit_with_config_error("confirm-select", err),
    };

    // Сторы + роли.
    let checkpoints = Arc::new(PgSyncCheckpointStore::new(db.clone()));
    let projection = Arc::new(PgOrderProjection::new(db.clone()));
    let master_key = MasterKey {
        version: 1,
        bytes: *config.vault_master_key.as_bytes(),
    };

    let scheduler = tokio::spawn(run_scheduler(
        db,
        master_key,
        worker_config,
        Arc::new(publisher),
        checkpoints,
        shutdown_rx.clone(),
    ));
    let consumer = tokio::spawn(amqp::run_consumer(
        consumer_channel,
        projection,
        shutdown_rx.clone(),
    ));
    tokio::spawn(wait_for_shutdown_signal(shutdown_tx));

    let _ = tokio::join!(scheduler, consumer);
    tracing::info!("pult-workers остановлен: очередь живёт, чекпоинты целы (AD-3/AD-15)");
}
