//! `pult-conveyor-adapters` — адаптеры площадок (Wildberries, Ozon, Битрикс24) — единственное место касания внешних API; расшифровка token-vault только здесь — AD-16.
//!
//! Story 1.4: первая вертикаль adapter → очередь → worker → PG:
//! - [`cipher`]: AES-256-GCM расшифровка токена (порт `TokenCipher` ядра
//!   identity; write-only vault 1.3 получает первого потребителя decrypt);
//! - [`wb`]: HTTP-клиент статистики WB — реализация порта `WbOrdersSource`
//!   (окна дат, лимиты, backoff на 429; токен не логируется).

pub mod cipher;
pub mod wb;
