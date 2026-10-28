//! Конверт ошибок — единый формат ответа об ошибке всех сервисов:
//! `{error:{code,message,request_id,details}}`.
//!
//! Правила конвенций spine:
//! `code` — стабильный машинный идентификатор (snake_case);
//! `message` — человекочитаемая диагностика без стек-трейсов;
//! `request_id` — для трассировки сквозь шлюз и конвейер;
//! `details` — структурированные детали (опционально).

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Конверт ошибок API.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ErrorEnvelope {
    /// Тело ошибки (ключ `error` — фиксированная обёртка конверта).
    pub error: ErrorBody,
}

/// Тело конверта ошибок.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ErrorBody {
    /// Стабильный машинный код ошибки (`validation_failed`, `portal_not_found`...).
    pub code: String,
    /// Человекочитаемое сообщение без стек-трейсов.
    pub message: String,
    /// Идентификатор запроса для трассировки (может отсутствовать → `null`).
    pub request_id: Option<Uuid>,
    /// Структурированные детали ошибки (может отсутствовать → `null`).
    pub details: Option<serde_json::Value>,
}

impl ErrorEnvelope {
    /// Минимальный конверт: только код и сообщение (request_id/details = `null`).
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            error: ErrorBody {
                code: code.into(),
                message: message.into(),
                request_id: None,
                details: None,
            },
        }
    }

    /// Присваивает request_id (цепочка вызовов: `... .with_request_id(id)`).
    #[must_use]
    pub fn with_request_id(mut self, request_id: Uuid) -> Self {
        self.error.request_id = Some(request_id);
        self
    }

    /// Добавляет структурированные детали (цепочка вызовов).
    #[must_use]
    pub fn with_details(mut self, details: serde_json::Value) -> Self {
        self.error.details = Some(details);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn canonical_shape_with_all_fields() {
        let envelope = ErrorEnvelope::new("portal_not_found", "Портал не найден")
            .with_request_id(Uuid::now_v7())
            .with_details(json!({"portal": "unknown"}));

        let value = serde_json::to_value(&envelope).expect("сериализуемо");
        let error = value.get("error").expect("обёртка error");
        for key in ["code", "message", "request_id", "details"] {
            assert!(
                error.get(key).is_some(),
                "конверт обязан содержать error.{key}"
            );
        }
        assert_eq!(error["code"], "portal_not_found");
        assert_eq!(error["message"], "Портал не найден");
    }

    #[test]
    fn minimal_envelope_serializes_nulls() {
        let envelope = ErrorEnvelope::new("validation_failed", "Некорректный запрос");
        let value = serde_json::to_value(&envelope).expect("сериализуемо");
        assert_eq!(
            value,
            json!({"error": {"code": "validation_failed", "message": "Некорректный запрос", "request_id": null, "details": null}})
        );
    }

    #[test]
    fn roundtrip_through_json() {
        let envelope = ErrorEnvelope::new("internal_error", "Сбой конвейера")
            .with_details(json!({"queue": "sync"}));
        let json = serde_json::to_string(&envelope).expect("сериализуемо");
        let parsed: ErrorEnvelope = serde_json::from_str(&json).expect("обратно читаемо");
        assert_eq!(parsed, envelope);
    }
}
