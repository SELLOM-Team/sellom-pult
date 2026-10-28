//! `pult-platform` — общий язык доменов «Пульт селлера» (platform-слой spine).
//!
//! Канонические конверты и конвенции, обязательные для всех слоёв:
//! - событийный конверт AD-22: [`events::EventEnvelope`]
//!   (`event_id` UUIDv7, past-tense тип, `organization_id`, `occurred_at`,
//!   `schema_version`, `payload`);
//! - конверт ошибок: [`error::ErrorEnvelope`]
//!   (`{error:{code,message,request_id,details}}`);
//! - базовый конфиг сервиса: [`config::ServiceConfig`].
//!
//! Крейт сознательно не зависит ни от axum, ни от sqlx — только сериализация
//! и время, чтобы любой слой (core, conveyor, gateway) мог зависеть от него
//! без циклов. Деньги — integer-копейки, время — timestamptz UTC (конвенции
//! spine): здесь для времени используется `chrono::DateTime<Utc>`.

pub mod config;
pub mod error;
pub mod events;
