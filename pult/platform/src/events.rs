//! Событийный конверт AD-22.
//!
//! Все внутренние события системы публикуются только в этом конверте:
//! `{event_id: UUIDv7, event_type: past-tense, organization_id, occurred_at,
//! schema_version, payload}`. Доставка — at-least-once, дедуп по `event_id`
//! на стороне приёмника (конвейер, AD-15).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Канонический событийный конверт (AD-22).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventEnvelope<P> {
    /// UUIDv7 — время-упорядоченный идентификатор события (генерация на слое
    /// приложения: `uuid::Uuid::now_v7()`; в DDL — только fallback).
    pub event_id: Uuid,
    /// Тип события: snake_case-метка (например, `organization_created`);
    /// семантически — past tense, но это конвенция именования (AD-22),
    /// проверяемая ревью, а не этим кодом.
    pub event_type: String,
    /// Организация-владелец данных (изоляция данных по порталу).
    pub organization_id: Uuid,
    /// Момент события, UTC (timestamptz-конвенция).
    pub occurred_at: DateTime<Utc>,
    /// Версия схемы payload — эволюция контрактов без breaking changes.
    pub schema_version: u32,
    /// Полезная нагрузка события.
    pub payload: P,
}

impl<P> EventEnvelope<P> {
    /// Собирает конверт, генерируя `event_id` (UUIDv7) и `occurred_at` (UTC).
    ///
    /// # Errors
    /// [`EventTypeError::InvalidEventType`], если `event_type` пуст или не
    /// snake_case-метка. Past tense кодом не проверяется — конвенция ревью.
    pub fn new(
        event_type: &str,
        organization_id: Uuid,
        schema_version: u32,
        payload: P,
    ) -> Result<Self, EventTypeError> {
        validate_event_type(event_type)?;
        Ok(Self {
            event_id: Uuid::now_v7(),
            event_type: event_type.to_owned(),
            organization_id,
            occurred_at: Utc::now(),
            schema_version,
            payload,
        })
    }
}

/// Ошибки валидации типа события.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EventTypeError {
    /// Тип события должен быть непустой snake_case-меткой; past tense —
    /// конвенция ревью, кодом не enforce.
    #[error(
        "event_type must be a non-empty snake_case label (past tense is a review convention, not enforced), got {0:?}"
    )]
    InvalidEventType(String),
}

fn validate_event_type(event_type: &str) -> Result<(), EventTypeError> {
    // Метка обязана начинаться со строчной латинской буквы: числовые/подчёркивающие
    // старты ("42", "_x") — не имена событий. Остальное — snake_case-алфавит
    // без двойных подчёркиваний и краевых подчёркиваний.
    let valid = event_type
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_lowercase())
        && !event_type.ends_with('_')
        && !event_type.contains("__")
        && event_type
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    if valid {
        Ok(())
    } else {
        Err(EventTypeError::InvalidEventType(event_type.to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn new_generates_uuidv7_and_utc_occurred_at() {
        let org = Uuid::now_v7();
        let envelope =
            EventEnvelope::new("organization_created", org, 1, json!({"name": "Портал"}))
                .expect("валидный тип события");

        assert_eq!(envelope.event_id.get_version_num(), 7);
        assert_eq!(envelope.organization_id, org);
        assert_eq!(envelope.schema_version, 1);
        let delta = Utc::now() - envelope.occurred_at;
        assert!(
            delta.num_seconds() < 5,
            "occurred_at должен быть ~текущее UTC"
        );
    }

    #[test]
    fn serializes_to_canonical_shape() {
        let org = Uuid::now_v7();
        let envelope =
            EventEnvelope::new("portal_settings_updated", org, 2, json!({"thresholds": 3}))
                .expect("валидный тип события");

        let value = serde_json::to_value(&envelope).expect("сериализуемо");
        let fields = value.as_object().expect("объект");
        for key in [
            "event_id",
            "event_type",
            "organization_id",
            "occurred_at",
            "schema_version",
            "payload",
        ] {
            assert!(
                fields.contains_key(key),
                "конверт AD-22 обязан содержать {key}"
            );
        }
        assert_eq!(fields["event_type"], "portal_settings_updated");
        assert_eq!(fields["schema_version"], 2);
    }

    #[test]
    fn roundtrip_through_json() {
        let org = Uuid::now_v7();
        let envelope = EventEnvelope::new("sync_finished", org, 1, json!({"cycles": 12}))
            .expect("валидный тип события");

        let json = serde_json::to_string(&envelope).expect("сериализуемо");
        let parsed: EventEnvelope<serde_json::Value> =
            serde_json::from_str(&json).expect("обратно читаемо");
        assert_eq!(parsed, envelope);
    }

    #[test]
    fn rejects_invalid_event_types() {
        let org = Uuid::now_v7();
        for bad in [
            "",
            "42",
            "4order_created",
            "OrgCreated",
            "org created",
            "org-created",
            "_created",
            "created_",
            "created__ok",
        ] {
            let err =
                EventEnvelope::new(bad, org, 1, json!(null)).expect_err("должен быть отклонён");
            assert_eq!(err, EventTypeError::InvalidEventType(bad.to_owned()));
        }
    }

    #[test]
    fn accepts_labels_with_digits_after_leading_letter() {
        let org = Uuid::now_v7();
        for good in ["order_2_shipped", "sync_finished", "f"] {
            EventEnvelope::new(good, org, 1, json!(null))
                .expect("строчная буква в старте + snake_case — валидно");
        }
    }
}
