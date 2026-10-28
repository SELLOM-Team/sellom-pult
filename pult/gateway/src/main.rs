//! `pult-gateway` — REST-шлюз «Пульт селлера» (Axum 0.8.9, стек AD-20).
//!
//! Story 1.1 (Structural Seed): только проверяемый жизненный цикл —
//! `/health` (проверка БД через sqlx-пул, 200/503 с версией приложения),
//! `/ready` (готовность: миграции применены и БД отвечает) и forward-only
//! миграции `sqlx::migrate!` из `db/migrations` на старте. Никакой
//! бизнес-логики: адаптеры площадок, канбаны и авторизация Б24 — Stories 1.2+
//! и Epic 2+.

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use tracing::{error, info, warn};

use pult_platform::config::ServiceConfig;

/// Бюджет пробы БД: ответ /health не блокируется долгими попытками пула
/// (I/O matrix: «повторные попытки пула не блокируют поток ответа»).
const DB_PROBE_TIMEOUT: Duration = Duration::from_millis(500);

/// Пауза между повторами применения миграций, пока БД не поднимется.
const MIGRATION_RETRY_DELAY: Duration = Duration::from_secs(5);

/// Общее состояние шлюза.
#[derive(Clone)]
struct AppState {
    /// Пул подключений к PostgreSQL (lazy: шлюз стартует и без живой БД).
    db: PgPool,
    /// Версия приложения (отдаётся в /health).
    version: &'static str,
    /// Флаг успешного применения миграций (виден в /ready).
    migrations_applied: Arc<AtomicBool>,
}

/// Тело ответа /health и /ready.
#[derive(Debug, Serialize, PartialEq, Eq)]
struct HealthResponse {
    /// `ok` | `degraded` (/health) | `ready` | `not_ready` (/ready).
    status: &'static str,
    /// Версия приложения.
    version: &'static str,
    /// Применены ли миграции (только /ready).
    #[serde(skip_serializing_if = "Option::is_none")]
    migrations_applied: Option<bool>,
}

/// Живая проверка: пул должен УСПЕШНО ответить на тривиальный запрос за бюджет
/// (tokio-timeout поверх попытки пула — поток ответа не блокируется).
/// Важно: проверяется и внешний Ok (уложились в бюджет), и внутренний Result
/// самого запроса — быстрая ошибка (auth-отказ, shutdown PG) ≠ «БД жива».
async fn db_reachable(db: &PgPool) -> bool {
    matches!(
        tokio::time::timeout(DB_PROBE_TIMEOUT, sqlx::query("SELECT 1").execute(db)).await,
        Ok(Ok(_))
    )
}

/// Маскирует пароль в строке подключения для логов: схема://user:***@host/…
/// (в логах не должно быть креденшелов даже при невалидном URL).
fn redact_url(url: &str) -> String {
    match url.split_once("://") {
        Some((scheme, rest)) => match rest.split_once('@') {
            Some((userinfo, hostpart)) => {
                let user = userinfo.split(':').next().unwrap_or("");
                format!("{scheme}://{user}:***@{hostpart}")
            }
            None => format!("{scheme}://{rest}"),
        },
        None => "***".to_owned(),
    }
}

/// `/health` — liveness с проверкой БД: 200 `ok` или 503 `degraded`.
async fn health(State(state): State<AppState>) -> (StatusCode, Json<HealthResponse>) {
    let response = if db_reachable(&state.db).await {
        HealthResponse {
            status: "ok",
            version: state.version,
            migrations_applied: None,
        }
    } else {
        warn!("/health: БД недоступна — degraded");
        HealthResponse {
            status: "degraded",
            version: state.version,
            migrations_applied: None,
        }
    };
    let code = if response.status == "ok" {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    (code, Json(response))
}

/// `/ready` — готовность принимать трафик: миграции применены и БД отвечает.
async fn ready(State(state): State<AppState>) -> (StatusCode, Json<HealthResponse>) {
    let migrated = state.migrations_applied.load(Ordering::Relaxed);
    let ready = migrated && db_reachable(&state.db).await;
    let response = HealthResponse {
        status: if ready { "ready" } else { "not_ready" },
        version: state.version,
        migrations_applied: Some(migrated),
    };
    let code = if ready {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    (code, Json(response))
}

/// Forward-only миграции на старте; при недоступной БД шлюз остаётся живым
/// и повторяет попытки — /health честно отдаёт degraded, /ready — not_ready.
async fn run_migrations(db: PgPool, migrations_applied: Arc<AtomicBool>) {
    // Встроенные в бинарь миграции из pult/db/migrations (sqlx migrate);
    // путь относительно манифеста крейта gateway.
    let migrator = sqlx::migrate!("../db/migrations");
    loop {
        match migrator.run(&db).await {
            Ok(_) => {
                migrations_applied.store(true, Ordering::Relaxed);
                info!(
                    "миграции применены (forward-only, {} версий)",
                    migrator.migrations.len()
                );
                return;
            }
            Err(err) => {
                error!("миграции не применены, повтор через {MIGRATION_RETRY_DELAY:?}: {err}");
                tokio::time::sleep(MIGRATION_RETRY_DELAY).await;
            }
        }
    }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "pult_gateway=info,sqlx=warn,info".into()),
        )
        .init();

    let config = ServiceConfig::from_env().unwrap_or_else(|err| {
        error!("конфиг невалиден: {err}");
        std::process::exit(1);
    });
    let version = env!("CARGO_PKG_VERSION");
    info!(port = config.http_port, env = ?config.environment, "pult-gateway {version} стартует");

    // connect_lazy: шлюз поднимается даже если БД ещё не готова (состязание
    // с compose --wait) — /health покажет degraded, миграции повторяются.
    // min_connections(1): проба /health не меряет холодный TCP+auth-коннект
    // после простоя (idle_timeout закрывает незанятые соединения) — тёплая
    // связь удерживается пулом в фоне, не блокируя старт.
    // URL парсится уже в connect_lazy: синтаксически битая строка — чистый
    // выход с диагностикой, не паника.
    let db = PgPoolOptions::new()
        .max_connections(5)
        .min_connections(1)
        .acquire_timeout(Duration::from_secs(3))
        .connect_lazy(&config.database_url)
        .unwrap_or_else(|err| {
            error!(
                "DATABASE_URL невалидна (connect_lazy): {err} — строка: {}",
                redact_url(&config.database_url)
            );
            std::process::exit(1);
        });

    let migrations_applied = Arc::new(AtomicBool::new(false));
    tokio::spawn(run_migrations(db.clone(), Arc::clone(&migrations_applied)));

    let app = Router::new()
        .route("/health", get(health))
        .route("/ready", get(ready))
        .with_state(AppState {
            db,
            version,
            migrations_applied,
        });

    let addr = SocketAddr::from(([0, 0, 0, 0], config.http_port));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .unwrap_or_else(|err| panic!("порт {addr} занят: {err}"));
    info!(%addr, "pult-gateway слушает");

    axum::serve(listener, app)
        .await
        .expect("HTTP-сервер не должен завершаться с ошибкой");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_payload_shape_ok() {
        let ok = HealthResponse {
            status: "ok",
            version: "0.1.0",
            migrations_applied: None,
        };
        let value = serde_json::to_value(&ok).expect("сериализуемо");
        assert_eq!(
            value,
            serde_json::json!({"status": "ok", "version": "0.1.0"})
        );
    }

    #[test]
    fn health_payload_shape_degraded() {
        let degraded = HealthResponse {
            status: "degraded",
            version: "0.1.0",
            migrations_applied: None,
        };
        let value = serde_json::to_value(&degraded).expect("сериализуемо");
        assert_eq!(value["status"], "degraded");
        assert_eq!(value["version"], "0.1.0");
    }

    #[test]
    fn ready_payload_reports_migrations() {
        let ready = HealthResponse {
            status: "not_ready",
            version: "0.1.0",
            migrations_applied: Some(false),
        };
        let value = serde_json::to_value(&ready).expect("сериализуемо");
        assert_eq!(value["status"], "not_ready");
        assert_eq!(value["migrations_applied"], false);
    }

    #[test]
    fn redact_url_masks_password() {
        assert_eq!(
            redact_url("postgres://pult:secret@localhost:5432/pult"),
            "postgres://pult:***@localhost:5432/pult"
        );
    }

    #[test]
    fn redact_url_handles_passwordless_and_broken() {
        assert_eq!(
            redact_url("postgres://localhost/pult"),
            "postgres://localhost/pult"
        );
        // Битая строка без схемы не раскрывает содержимое в лог.
        assert_eq!(redact_url("not a url at all"), "***");
    }
}
