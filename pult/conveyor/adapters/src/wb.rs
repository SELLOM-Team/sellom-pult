//! HTTP-клиент статистики WB (Story 1.4, FR-3).
//!
//! Реализация порта [`pult_core_sync::ports::WbOrdersSource`]: чтение истории
//! заказов окнами дат. Тонкий слой: URL/параметры, парсинг wire-формата,
//! фильтрация окна, backoff на 429. Бизнес-агрегация строк — в ядре
//! (`core::sync::orders::aggregate_wb_rows`), адаптер её не дублирует.
//!
//! Инварианты:
//! - токен передаётся ТОЛЬКО в заголовке Authorization; в логах — никогда
//!   (трассируются окно/URL-хост/статус ответа/размер);
//! - 429/Retry-After уважается: экспоненциальная пауза, не превосходит
//!   `max_attempts`; после исчерпания — честная ошибка, не «пустое окно»;
//! - границы окна: API статистики фильтрует СЛЕВА (`dateFrom`); правая
//!   граница — фильтром `last_change_date` по строке (`to` включительно) —
//!   граничные строки могут прийти дважды в соседних окнах, это погашается
//!   идемпотентностью проекции (AD-15).

use chrono::{DateTime, SecondsFormat, Utc};
use pult_core_sync::ports::{DateWindow, WbOrdersSource};

/// Ошибка источника заказов WB.
#[derive(Debug, thiserror::Error)]
pub enum WbSourceError {
    /// Ответ не распарсен (площадка сменила схему — событие для ревью).
    #[error("ответ WB-статистики не распарсен: {0}")]
    BadPayload(String),
    /// История ответила отказом доступа — токен отозван/недействителен.
    #[error("WB-статистика ответила {0}: доступ запрещён (токен отозван?)")]
    AccessDenied(u16),
    /// Ответ площадки с неожиданным кодом.
    #[error("WB-статистика ответила HTTP {0}")]
    UnexpectedStatus(u16),
    /// 429 не исчерпан за отведённые попытки.
    #[error("лимит WB-статистики не отпустил за {0} попыток")]
    RateLimited(u32),
    /// Сетевой сбой/таймаут.
    #[error("сеть WB-статистики: {0}")]
    Transport(String),
    /// Ответ превысил потолок (тяжёлая история в одном окне).
    #[error("ответ WB-статистики превысил потолок {0} байт — сузить окно")]
    TooLarge(u64),
}

/// Wire-строка заказа из `/api/v1/supplier/orders` (serde — компоновочный
/// слой конвейера; домен без serde, преобразование явное).
#[derive(Debug, serde::Deserialize)]
struct WbWireRow {
    date: DateTime<Utc>,
    #[serde(rename = "lastChangeDate")]
    last_change_date: DateTime<Utc>,
    #[serde(rename = "supplierArticle")]
    supplier_article: String,
    #[serde(rename = "techSize")]
    tech_size: String,
    barcode: String,
    #[serde(rename = "discountPercent")]
    discount_percent: i64,
    #[serde(rename = "isCancel")]
    is_cancel: bool,
    #[serde(rename = "incomeID")]
    income_id: i64,
    #[serde(rename = "warehouseName")]
    warehouse_name: String,
    #[serde(rename = "orderId")]
    order_id: i64,
    #[serde(rename = "nmId")]
    nm_id: i64,
    subject: String,
    brand: String,
    status: i64,
    finished: bool,
    price: i64,
    #[serde(rename = "totalPrice")]
    total_price: i64,
}

impl WbWireRow {
    fn into_domain(self) -> pult_core_sync::ports::WbOrderRow {
        pult_core_sync::ports::WbOrderRow {
            date: self.date,
            last_change_date: self.last_change_date,
            supplier_article: self.supplier_article,
            tech_size: self.tech_size,
            barcode: self.barcode,
            discount_percent: self.discount_percent,
            is_cancel: self.is_cancel,
            income_id: self.income_id,
            warehouse_name: self.warehouse_name,
            order_id: self.order_id,
            nm_id: self.nm_id,
            subject: self.subject,
            brand: self.brand,
            status: self.status,
            finished: self.finished,
            price: self.price,
            total_price: self.total_price,
        }
    }
}

/// Парсит тело ответа статистики (массив строк) в доменные строки.
///
/// Пустое тело `[]` — легальный результат (окно без заказов).
pub fn parse_orders_body(
    body: &[u8],
) -> Result<Vec<pult_core_sync::ports::WbOrderRow>, WbSourceError> {
    let rows: Vec<WbWireRow> =
        serde_json::from_slice(body).map_err(|e| WbSourceError::BadPayload(e.to_string()))?;
    Ok(rows.into_iter().map(WbWireRow::into_domain).collect())
}

/// Оставляет строки окна: `from <= last_change_date <= to`.
///
/// API фильтрует только слева (`dateFrom`): правую границу наводит клиент.
/// Граничные строки могут повториться в следующем окне — проекция
/// идемпотентна (AD-15), дубль = NO-OP.
pub fn rows_within_window(
    rows: Vec<pult_core_sync::ports::WbOrderRow>,
    window: DateWindow,
) -> Vec<pult_core_sync::ports::WbOrderRow> {
    rows.into_iter()
        .filter(|r| r.last_change_date >= window.from && r.last_change_date <= window.to)
        .collect()
}

/// HTTP-источник заказов WB (порт [`WbOrdersSource`]).
pub struct WbHttpOrdersSource {
    http: reqwest::Client,
    /// База statistics-API (prod или CI-заглушка), без хвостового слэша.
    base_url: String,
    /// Расшифрованный токен (в заголовок Authorization; не логируется).
    token: String,
    /// Таймаут одного запроса.
    timeout: std::time::Duration,
    /// Потолок тела ответа (байт).
    body_limit: u64,
    /// Попыток на один вызов (429/сеть), считая первую.
    max_attempts: u32,
}

impl WbHttpOrdersSource {
    /// Собирает источник. `token` — уже расшифрованные байты (расшифровка —
    /// [`crate::cipher::VaultTokenCipher`], в конвейере).
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        base_url: impl Into<String>,
        token: String,
        timeout: std::time::Duration,
        body_limit: u64,
        max_attempts: u32,
    ) -> Self {
        Self {
            http: reqwest::Client::builder()
                .timeout(timeout)
                .build()
                .expect("reqwest-клиент с rustls собирается без конфигурации"),
            base_url: base_url.into(),
            token,
            timeout,
            body_limit,
            max_attempts: max_attempts.max(1),
        }
    }

    /// URL окна: `?dateFrom=...&flag=0` (полная выгрузка с границы окна;
    /// `flag=1` — только изменения — будущий циклический синк).
    fn window_url(&self, window: DateWindow) -> String {
        // Naive-формат из примеров WB (без таймзоны); интерпретация
        // граничных строк корректируется клиентским фильтром окна.
        let from = window.from.to_rfc3339_opts(SecondsFormat::Secs, true);
        format!(
            "{}/api/v1/supplier/orders?dateFrom={from}&flag=0",
            self.base_url.trim_end_matches('/')
        )
    }
}

impl WbOrdersSource for WbHttpOrdersSource {
    type Error = WbSourceError;

    fn classify_failure(error: &WbSourceError) -> pult_core_sync::ports::SourceFailure {
        match error {
            // 401/403: токен отозван/недействителен — ретрай бессмыслен.
            WbSourceError::AccessDenied(_) => pult_core_sync::ports::SourceFailure::AccessRevoked,
            // Лимит не отпустил за отведённые попытки, сеть, неожиданный 5xx —
            // временный класс: продолжение с чекпоинта (AD-3).
            WbSourceError::RateLimited(_) | WbSourceError::Transport(_) => {
                pult_core_sync::ports::SourceFailure::Transient
            }
            WbSourceError::UnexpectedStatus(_) => pult_core_sync::ports::SourceFailure::Transient,
            // Схема ответа сменилась (событие для ревью) или потолок тела —
            // без человека не чинится.
            WbSourceError::BadPayload(_) | WbSourceError::TooLarge(_) => {
                pult_core_sync::ports::SourceFailure::Fatal
            }
        }
    }

    async fn fetch_window(
        &self,
        window: DateWindow,
    ) -> Result<Vec<pult_core_sync::ports::WbOrderRow>, Self::Error> {
        let url = self.window_url(window);
        let mut attempt: u32 = 0;
        loop {
            attempt += 1;
            let resp = self
                .http
                .get(&url)
                .header("Authorization", &self.token)
                .send()
                .await
                .map_err(|e| WbSourceError::Transport(redact(&e.to_string())))?;

            let status = resp.status();
            if status.as_u16() == 429 {
                if attempt >= self.max_attempts {
                    return Err(WbSourceError::RateLimited(attempt));
                }
                // Retry-After уважаем (секунды); иначе — экспоненциальная пауза.
                let pause = resp
                    .headers()
                    .get("Retry-After")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|s| s.trim().parse::<u64>().ok())
                    .map(std::time::Duration::from_secs)
                    .unwrap_or_else(|| {
                        std::time::Duration::from_millis(500 * (1_u64 << attempt.min(6)))
                    });
                tracing::warn!(
                    attempt,
                    pause_ms = pause.as_millis() as u64,
                    "WB 429: пауза и повтор"
                );
                tokio::time::sleep(pause).await;
                continue;
            }
            if status.as_u16() == 401 || status.as_u16() == 403 {
                return Err(WbSourceError::AccessDenied(status.as_u16()));
            }
            if !status.is_success() {
                return Err(WbSourceError::UnexpectedStatus(status.as_u16()));
            }

            let content_length = resp.content_length().unwrap_or(0);
            let body = resp
                .bytes()
                .await
                .map_err(|e| WbSourceError::Transport(redact(&e.to_string())))?;
            if content_length > self.body_limit || body.len() as u64 > self.body_limit {
                return Err(WbSourceError::TooLarge(self.body_limit));
            }
            let rows = parse_orders_body(&body)?;
            let in_window = rows_within_window(rows, window);
            tracing::info!(
                window_from = %window.from,
                window_to = %window.to,
                rows_total = body.len(),
                rows_window = in_window.len(),
                timeout_secs = self.timeout.as_secs(),
                "WB-окно прочитано"
            );
            return Ok(in_window);
        }
    }
}

/// Зачистка транспортного сообщения от случайной утечки секрета: сообщения
/// reqwest не содержат заголовков, но защита дешёвая и граница явная.
fn redact(msg: &str) -> String {
    let token_mark = "Authorization";
    match msg.find(token_mark) {
        Some(pos) => {
            let mut cleaned = String::new();
            cleaned.push_str(&msg[..pos]);
            cleaned.push_str("Authorization=***redacted***");
            cleaned
        }
        None => msg.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn win(from_ms: i64, to_ms: i64) -> DateWindow {
        DateWindow {
            from: Utc.timestamp_millis_opt(from_ms).unwrap(),
            to: Utc.timestamp_millis_opt(to_ms).unwrap(),
        }
    }

    #[test]
    fn parse_orders_body_full_and_empty() {
        // Строковый литерал + as_bytes: byte-string-литералы (br#"…"#)
        // запрещают не-ASCII, а fixture честный — с кириллицей площадок.
        let body = r#"[{
            "date":"2026-10-20T10:00:00+03:00",
            "lastChangeDate":"2026-10-20T10:05:00+03:00",
            "supplierArticle":"ART-1",
            "techSize":"M",
            "barcode":"2000000000000",
            "total":10,
            "discountPercent":10,
            "isCancel":false,
            "incomeID":555,
            "warehouseName":"Коледино",
            "orderId":771,
            "nmId":123,
            "subject":"Платья",
            "category":"Женщинам",
            "brand":"Бренд",
            "status":3,
            "finished":false,
            "price":10000,
            "totalPrice":9000
        }]"#
        .as_bytes();
        let rows = parse_orders_body(body).expect("wire-схема парсится");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].order_id, 771);
        assert_eq!(rows[0].income_id, 555);
        assert_eq!(rows[0].total_price, 9000);

        // пустая история — легальный ответ
        assert!(parse_orders_body(b"[]").unwrap().is_empty());
    }

    #[test]
    fn parse_orders_body_rejects_garbage() {
        assert!(matches!(
            parse_orders_body(b"{\"error\":true}"),
            Err(WbSourceError::BadPayload(_))
        ));
    }

    #[test]
    fn window_filter_left_and_right_bounds() {
        let row = |ms: i64| pult_core_sync::ports::WbOrderRow {
            date: Utc.timestamp_millis_opt(ms).unwrap(),
            last_change_date: Utc.timestamp_millis_opt(ms).unwrap(),
            supplier_article: "A".into(),
            tech_size: "M".into(),
            barcode: "B".into(),
            discount_percent: 0,
            is_cancel: false,
            income_id: 0,
            warehouse_name: "W".into(),
            order_id: 1,
            nm_id: 1,
            subject: "S".into(),
            brand: "BR".into(),
            status: 0,
            finished: false,
            price: 1,
            total_price: 1,
        };
        // строки: 1000 (до окна), 2000 (левая граница), 3000 (внутри),
        // 4000 (правая граница), 5000 (после окна)
        let rows = vec![row(1000), row(2000), row(3000), row(4000), row(5000)];
        let kept = rows_within_window(rows, win(2000, 4000));
        let changes: Vec<i64> = kept
            .iter()
            .map(|r| r.last_change_date.timestamp_millis())
            .collect();
        // границы включительно
        assert_eq!(changes, vec![2000, 3000, 4000]);
    }

    #[test]
    fn window_url_shape() {
        let src = WbHttpOrdersSource::new(
            "https://statistics-api.wildberries.ru/",
            "tok".into(),
            std::time::Duration::from_secs(10),
            1024,
            3,
        );
        let url = src.window_url(win(1_700_000_000_000, 1_700_086_400_000));
        assert_eq!(
            url,
            "https://statistics-api.wildberries.ru/api/v1/supplier/orders?dateFrom=2023-11-14T22:13:20Z&flag=0"
        );
    }

    #[test]
    fn redact_scrubs_authorization() {
        let dirty = "err sending request Authorization: abc123 header";
        assert_eq!(
            redact(dirty),
            "err sending request Authorization=***redacted***"
        );
        assert_eq!(redact("чистое сообщение"), "чистое сообщение");
    }
}
