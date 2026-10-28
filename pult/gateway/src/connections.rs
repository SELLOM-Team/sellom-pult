//! Мастер подключения площадок с token-vault (Story 1.3, FR-2, AD-16).
//!
//! Маршруты `/v1` (AD-13; ошибки — конверт `{error:{code,message,request_id,details}}`):
//! - `GET /v1/connections` — статусы подключений обеих площадок (без секретов);
//! - `POST /v1/connections` — автотест прав пробами API площадки и, при успехе,
//!   шифрование AES-256-GCM (`EnvAesGcmCipher`) + сохранение метаданных;
//!   неуспешный автотест токен НЕ сохраняет (rejected / probe_unavailable);
//! - `DELETE /v1/connections/{marketplace}` — удаление подключения (токен
//!   отозван вместе со строкой); идемпотентно.
//!
//! Инварианты (спека 1.3):
//! - токен в БД ТОЛЬКО шифротекстом (AES-256-GCM, уникальный nonce на запись,
//!   key_version); Ozon Client-Id — не секрет, хранится открыто;
//! - валидация (422) выполняется ДО пробы и БД и работает в деградации;
//! - расшифрование в проде-пути Ф0 отсутствует (write-only vault): `decrypt`
//!   существует только на порту [`TokenCipher`] для адаптеров Epic 2;
//! - токен никогда не попадает в логи — только метаданные и маска show_last4;
//! - повторный POST по той же площадке — перезапись (UNIQUE(organization_id,
//!   marketplace), INSERT … ON CONFLICT DO UPDATE), не дубль.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use axum::Json;
use axum::Router;
use axum::extract::Path;
use axum::extract::rejection::{JsonRejection, QueryRejection};
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::routing::{delete, get};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tracing::{debug, info, warn};
use uuid::Uuid;

use aes_gcm::aead::{Aead, OsRng};
use aes_gcm::{AeadCore, Aes256Gcm, KeyInit, Nonce};

use pult_core_identity::ports::{
    ConnectionStore, EncryptedPayload, MasterKey, MasterKeyProvider, RightsProbe, StoredConnection,
    TokenCipher, VerifiedConnection,
};
use pult_core_identity::vault::{
    self, ConnectionStatus, Credentials, Marketplace, ProbeCheck, ProbeResult, RightsCheck, Scheme,
    summarize_probes,
};

use pult_platform::error::ErrorEnvelope;

use crate::AppState;
use crate::onboarding::{
    ApiResult, PlacementIdentity, db_unavailable, echo, error_response, placement_error_response,
};

/// Бюджет ОДНОГО обращения к БД (паттерн Story 1.2: просрочка = 503-конверт;
/// 2 × 2с < 8с клиентского бюджета api.ts).
const STORE_BUDGET: Duration = Duration::from_secs(2);

/// Таймаут ОДНОЙ пробы прав (спека 1.3: «таймаут пробы 10с»).
const PROBE_TIMEOUT: Duration = Duration::from_secs(10);
/// Таймаут установления TCP/TLS соединения с площадкой: отдельный бюджет,
/// чтобы зависший адрес не съел весь 10-секундный таймаут пробы.
const PROBE_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

// --- Пробная матрица: URL/методы — конфиг-константы (Директива 1) ---
//
// Верифицировано веб-пробой без валидных токенов (curl, 2026-10-28):
// - GET https://statistics-api.wildberries.ru/api/v1/supplier/stocks?dateFrom=<дата>
//   → 401 JSON (endpoint жив; токен — заголовок Authorization; категория
//   токена «Статистика» по dev.wildberries.ru);
// - GET https://discounts-prices-api.wildberries.ru/api/v2/list/goods/filter?limit=1
//   → 401 JSON (категория «Цены и скидки»);
// - POST https://api-seller.ozon.ru/v3/product/list (заголовки Client-Id и
//   Api-Key, тело {"limit":1}) → 400 {"code":3,"message":"Invalid Api-Key…"}
//   для неверного ключа (endpoint жив; см. маркер в ProbeSpec).
const WB_STATISTICS_HOST: &str = "https://statistics-api.wildberries.ru";
const WB_PRICES_HOST: &str = "https://discounts-prices-api.wildberries.ru";
const OZON_HOST: &str = "https://api-seller.ozon.ru";

/// Максимальная длина вырезки тела ответа площадки для маркерной диагностики
/// (тела НЕ логируются и не возвращаются пользователю).
const PROBE_BODY_SNIPPET: usize = 512;

/// Одна проба права: конфиг-константа матрицы + подставляемые креденшелы.
/// Токен живёт только в заголовках запроса — ни в URL, ни в логах.
struct ProbeSpec {
    /// Человекочитаемое имя категории прав.
    name: &'static str,
    /// HTTP-метод ("GET" | "POST").
    method: &'static str,
    /// Полный URL пробы (хост из констант матрицы или CI-подмена).
    url: String,
    /// Заголовки авторизации площадки (секрет в значении).
    headers: Vec<(&'static str, String)>,
    /// JSON-тело (для POST).
    body: Option<String>,
    /// Маркер credentials-ошибки в теле ответа (Ozon отвечает 400 с
    /// «Invalid Api-Key» на неверный ключ — это отказ, не недоступность).
    auth_error_marker: Option<&'static str>,
    /// Человекочитаемая подсказка при 401/403 (что сделать владельцу).
    denial_hint: &'static str,
}

/// Классифицирует ответ площадки на пробу (чистая функция — тестируется
/// напрямую): 2xx → подтверждено; 401/403 (и маркерные 400 Ozon) → отказ
/// с подсказкой; всё прочее → недоступность.
fn classify_probe_response(status: u16, body_snippet: &str, spec: &ProbeSpec) -> ProbeResult {
    if (200..300).contains(&status) {
        return ProbeResult::Confirmed;
    }
    if status == 401 || status == 403 {
        return ProbeResult::Denied(spec.denial_hint.to_owned());
    }
    if let Some(marker) = spec.auth_error_marker
        && body_snippet.contains(marker)
    {
        return ProbeResult::Denied(spec.denial_hint.to_owned());
    }
    ProbeResult::Unreachable
}

/// Собирает пробную матрицу для площадки и креденшелов (чистая функция).
///
/// `override_host` — test/CI-подмена хоста ВСЕХ проб (`PULT_PROBE_HOST_OVERRIDE`,
/// например `http://127.0.0.1:9`): детерминированный probe_unavailable без
/// выхода наружу. Путь пробы остаётся константой матрицы.
fn build_probe_specs(
    marketplace: Marketplace,
    credentials: &Credentials,
    today: &str,
    override_host: Option<&str>,
) -> Vec<ProbeSpec> {
    let (wb_statistics_host, wb_prices_host, ozon_host): (String, String, String) =
        match override_host {
            Some(host) => (host.to_owned(), host.to_owned(), host.to_owned()),
            None => (
                WB_STATISTICS_HOST.to_owned(),
                WB_PRICES_HOST.to_owned(),
                OZON_HOST.to_owned(),
            ),
        };
    match (marketplace, credentials) {
        (Marketplace::Wildberries, Credentials::Wb { token }) => vec![
            ProbeSpec {
                name: "Статистика",
                method: "GET",
                url: format!("{wb_statistics_host}/api/v1/supplier/stocks?dateFrom={today}"),
                headers: vec![("Authorization", token.clone())],
                body: None,
                auth_error_marker: None,
                denial_hint: "в кабинете Wildberries пересоздайте токен с категорией «Статистика»",
            },
            ProbeSpec {
                name: "Цены и скидки",
                method: "GET",
                url: format!("{wb_prices_host}/api/v2/list/goods/filter?limit=1"),
                headers: vec![("Authorization", token.clone())],
                body: None,
                auth_error_marker: None,
                denial_hint: "в кабинете Wildberries пересоздайте токен с категорией «Цены и скидки»",
            },
        ],
        (Marketplace::Ozon, Credentials::Ozon { client_id, api_key }) => vec![ProbeSpec {
            name: "Товары",
            method: "POST",
            url: format!("{ozon_host}/v3/product/list"),
            headers: vec![
                ("Client-Id", client_id.clone()),
                ("Api-Key", api_key.clone()),
            ],
            body: Some("{\"limit\":1}".to_owned()),
            auth_error_marker: Some("Invalid Api-Key"),
            denial_hint: "проверьте Client-Id и Api-Key: ключ должен принадлежать кабинету Ozon и иметь право чтения товаров",
        }],
        // Креденшелы всегда собираются вместе с площадкой (connect_request_from_body);
        // рассинхрон — программная ошибка: честная недоступность вместо паники.
        _ => vec![ProbeSpec {
            name: "конфигурация пробы",
            method: "GET",
            url: String::new(),
            headers: Vec::new(),
            body: None,
            auth_error_marker: None,
            denial_hint: "внутренняя ошибка сборки пробы",
        }],
    }
}

/// Дата пробы `dateFrom` в формате YYYY-MM-DD (UTC).
fn today_iso() -> String {
    chrono::Utc::now()
        .date_naive()
        .format("%Y-%m-%d")
        .to_string()
}

/// Реализация проб прав живыми запросами к API площадок (reqwest, rustls).
/// Секреты живут только в заголовках исходящих запросов; в логи — ни токена,
/// ни тела ответа площадки (только имена проб и коды).
pub(crate) struct HttpRightsProbe {
    http: reqwest::Client,
    override_host: Option<String>,
}

impl HttpRightsProbe {
    pub(crate) fn new(override_host: Option<String>) -> Result<Self, reqwest::Error> {
        let http = reqwest::Client::builder()
            .connect_timeout(PROBE_CONNECT_TIMEOUT)
            .timeout(PROBE_TIMEOUT)
            .build()?;
        Ok(Self {
            http,
            override_host,
        })
    }

    /// Выполняет одну пробу матрицы; любая ошибка транспорта/протокола —
    /// [`ProbeResult::Unreachable`] (honest-ошибка, не отказ прав).
    async fn run_probe(&self, spec: ProbeSpec) -> ProbeCheck {
        debug!(
            probe = spec.name,
            method = spec.method,
            "проба прав отправляется"
        );
        let request = match spec.method {
            "POST" => self.http.post(&spec.url),
            _ => self.http.get(&spec.url),
        };
        let mut request = request;
        for (name, value) in &spec.headers {
            request = request.header(*name, value);
        }
        if let Some(body) = &spec.body {
            request = request
                .header("Content-Type", "application/json")
                .body(body.clone());
        }
        match request.send().await {
            Ok(response) => {
                // Потолок чтения тела: непонятный/враждебный ответ площадки не
                // буферизуется целиком (ревью 1.3: нужен только короткий сниппет
                // для маркерной диагностики Ozon).
                const MAX_BODY_BYTES: usize = 64 * 1024;
                if response
                    .content_length()
                    .is_some_and(|len| len > MAX_BODY_BYTES as u64)
                {
                    debug!(
                        probe = spec.name,
                        "проба прав ответила слишком большим телом"
                    );
                    ProbeCheck {
                        name: spec.name.to_owned(),
                        result: ProbeResult::Unreachable,
                    }
                } else {
                    let status = response.status().as_u16();
                    // Тело — только для маркерной диагностики Ozon; обрезается и
                    // НЕ попадает в логи (площадки не любят эхо своих тел).
                    let body = response.text().await.unwrap_or_default();
                    let snippet: String = body.chars().take(PROBE_BODY_SNIPPET).collect();
                    debug!(probe = spec.name, status, "проба прав ответила");
                    let result = classify_probe_response(status, &snippet, &spec);
                    ProbeCheck {
                        name: spec.name.to_owned(),
                        result,
                    }
                }
            }
            // Таймаут/DNS/соединение/5xx-классификация не нужна: сеть не ответила
            // валидной пробой → honest probe_unavailable (не «отказ прав»).
            Err(err) => {
                debug!(probe = spec.name, error = %err, "проба прав не ответила");
                ProbeCheck {
                    name: spec.name.to_owned(),
                    result: ProbeResult::Unreachable,
                }
            }
        }
    }
}

impl RightsProbe for HttpRightsProbe {
    fn probe_rights<'a>(
        &'a self,
        marketplace: Marketplace,
        credentials: &'a Credentials,
    ) -> Pin<Box<dyn Future<Output = Vec<ProbeCheck>> + Send + 'a>> {
        Box::pin(async move {
            let specs = build_probe_specs(
                marketplace,
                credentials,
                &today_iso(),
                self.override_host.as_deref(),
            );
            let mut checks = Vec::with_capacity(specs.len());
            for spec in specs {
                checks.push(self.run_probe(spec).await);
            }
            checks
        })
    }
}

// --- Шифратор (KMS-конверт Ф0 = env-ключ) ---

/// Источник ключей Ф0: единственный ключ из `PULT_VAULT_MASTER_KEY`, версия 1.
/// Ротация master-ключа (несколько версий) — deferred Ф1+ (re-encrypt job).
pub(crate) struct EnvMasterKeyProvider {
    key: [u8; 32],
}

impl EnvMasterKeyProvider {
    pub(crate) fn new(key: [u8; 32]) -> Self {
        Self { key }
    }
}

impl MasterKeyProvider for EnvMasterKeyProvider {
    fn active_key(&self) -> MasterKey {
        MasterKey {
            version: 1,
            bytes: self.key,
        }
    }

    fn key_for_version(&self, version: u32) -> Option<MasterKey> {
        (version == 1).then(|| self.active_key())
    }
}

/// Ошибки AES-256-GCM шифратора (без секретов в сообщениях).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum CipherError {
    #[error("мастер-ключ должен быть 32 байта для AES-256")]
    KeyLength,
    #[error("неизвестная версия ключа: {0}")]
    UnknownKeyVersion(u32),
    #[error("nonce записи должен быть 12 байт (GCM)")]
    NonceLength,
    #[error("шифрование не выполнено")]
    Encrypt,
    #[error("расшифровка не выполнена: запись чужая или повреждена")]
    Decrypt,
}

/// AES-256-GCM шифратор токенов: уникальный nonce на запись, key_version из
/// провайдера ключей. `decrypt` существует ТОЛЬКО для порта Epic 2 и юнит-
/// тестов — в проде-пути Ф0 не вызывается (write-only vault).
pub(crate) struct EnvAesGcmCipher {
    keys: Arc<dyn MasterKeyProvider>,
}

impl EnvAesGcmCipher {
    pub(crate) fn new(keys: Arc<dyn MasterKeyProvider>) -> Self {
        Self { keys }
    }

    fn cipher_for(&self, key: &MasterKey) -> Result<Aes256Gcm, CipherError> {
        Aes256Gcm::new_from_slice(&key.bytes).map_err(|_| CipherError::KeyLength)
    }
}

impl TokenCipher for EnvAesGcmCipher {
    type Error = CipherError;

    fn encrypt(&self, plaintext: &[u8]) -> Result<EncryptedPayload, CipherError> {
        let master = self.keys.active_key();
        let cipher = self.cipher_for(&master)?;
        // Уникальный nonce на запись (CSPRNG, 12 байт) — инвариант AD-16.
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
        let ciphertext = cipher
            .encrypt(&nonce, plaintext)
            .map_err(|_| CipherError::Encrypt)?;
        Ok(EncryptedPayload {
            ciphertext,
            nonce: nonce.to_vec(),
            key_version: master.version,
        })
    }

    fn decrypt(&self, payload: &EncryptedPayload) -> Result<Vec<u8>, CipherError> {
        let master = self
            .keys
            .key_for_version(payload.key_version)
            .ok_or(CipherError::UnknownKeyVersion(payload.key_version))?;
        let cipher = self.cipher_for(&master)?;
        // Строгая длина nonce (12 байт для GCM) до конструирования массива.
        let nonce_bytes: [u8; 12] = payload
            .nonce
            .as_slice()
            .try_into()
            .map_err(|_| CipherError::NonceLength)?;
        let nonce = Nonce::from(nonce_bytes);
        cipher
            .decrypt(&nonce, payload.ciphertext.as_ref())
            .map_err(|_| CipherError::Decrypt)
    }
}

// --- Маскирование токена (в логах — только маски, никогда не секрет) ---

/// Маска секрета для логов: последние 4 символа после многоточия; короткие
/// значения маскируются полностью (нет смысла раскрывать «хвост» мусора).
fn show_last4(secret: &str) -> String {
    let chars: Vec<char> = secret.trim().chars().collect();
    if chars.len() <= 4 {
        "****".to_owned()
    } else {
        let tail: String = chars[chars.len() - 4..].iter().collect();
        format!("…{tail}")
    }
}

// --- sqlx-стор подключений (порт ConnectionStore) ---

/// Ошибки стора подключений.
#[derive(Debug)]
pub(crate) enum ConnectionsStoreError {
    /// Ошибка БД (недоступна/отказ запроса) → 503 `database_unavailable`.
    Database(sqlx::Error),
    /// Повреждённые доменные значения в БД → 500 `internal_error`
    /// (значение в сообщение не выносится — только имя поля).
    CorruptData(&'static str),
}

impl From<sqlx::Error> for ConnectionsStoreError {
    fn from(err: sqlx::Error) -> Self {
        ConnectionsStoreError::Database(err)
    }
}

/// sqlx-стор подключений: реализует порт [`ConnectionStore`] домена identity.
/// SQL — рантайм-запросы (паттерн Story 1.2).
#[derive(Clone)]
pub(crate) struct PgConnectionStore {
    db: PgPool,
}

impl PgConnectionStore {
    pub(crate) fn new(db: PgPool) -> Self {
        Self { db }
    }

    /// Read-only разрешение организации по ключу реестра установок.
    /// GET/DELETE создания организации НЕ делают (write-on-GET не плодим —
    /// см. deferred-work 1.2): нет портала — пустой список, не новая строка.
    pub(crate) async fn resolve_organization(
        &self,
        member_id: &str,
    ) -> Result<Option<Uuid>, ConnectionsStoreError> {
        let row: Option<(Uuid,)> =
            sqlx::query_as("SELECT id FROM organizations WHERE b24_member_id = $1")
                .bind(member_id)
                .fetch_optional(&self.db)
                .await?;
        Ok(row.map(|(id,)| id))
    }
}

/// Строка БД подключений (сырой sqlx-кортеж запроса списка).
type ConnectionRow = (
    String,
    serde_json::Value,
    String,
    Option<String>,
    serde_json::Value,
    Option<i64>,
);

impl ConnectionStore for PgConnectionStore {
    type Error = ConnectionsStoreError;

    /// Перезапись по UNIQUE(organization_id, marketplace) — ротация токена =
    /// повторный POST (шифротекст, verified_at, права перезаписываются).
    async fn save_connection(
        &self,
        organization_id: Uuid,
        connection: &VerifiedConnection,
    ) -> Result<(), ConnectionsStoreError> {
        let schemes_json = serde_json::json!(
            connection
                .schemes
                .iter()
                .map(scheme_as_str)
                .collect::<Vec<_>>()
        );
        let rights_json = serde_json::json!(
            connection
                .rights
                .iter()
                .map(|check| serde_json::json!({
                    "name": check.name,
                    "ok": check.ok,
                    "hint": check.hint,
                }))
                .collect::<Vec<_>>()
        );
        sqlx::query(
            "INSERT INTO marketplace_connections \
                (id, organization_id, marketplace, schemes, status, client_id, \
                 token_ciphertext, token_nonce, key_version, rights_snapshot, verified_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, now()) \
             ON CONFLICT (organization_id, marketplace) DO UPDATE SET \
                 schemes = EXCLUDED.schemes, \
                 status = EXCLUDED.status, \
                 client_id = EXCLUDED.client_id, \
                 token_ciphertext = EXCLUDED.token_ciphertext, \
                 token_nonce = EXCLUDED.token_nonce, \
                 key_version = EXCLUDED.key_version, \
                 rights_snapshot = EXCLUDED.rights_snapshot, \
                 verified_at = EXCLUDED.verified_at",
        )
        .bind(Uuid::now_v7())
        .bind(organization_id)
        .bind(marketplace_as_str(&connection.marketplace))
        .bind(schemes_json)
        .bind(status_as_str(&ConnectionStatus::Verified))
        .bind(connection.client_id.as_deref())
        .bind(connection.secret.ciphertext.clone())
        .bind(connection.secret.nonce.clone())
        .bind(i32::try_from(connection.secret.key_version).map_err(|_| {
            // Версия ключа вне i32 — повреждение на этапе композиции, не тихий
            // clamp в i32::MAX (ревью 1.3: ротация по версии обязана быть честной).
            ConnectionsStoreError::CorruptData("key_version")
        })?)
        .bind(rights_json)
        .execute(&self.db)
        .await?;
        Ok(())
    }

    /// Все подключения организации — БЕЗ секретов (шифротекст наружу не идёт).
    async fn list_connections(
        &self,
        organization_id: Uuid,
    ) -> Result<Vec<StoredConnection>, ConnectionsStoreError> {
        // COALESCE прав: NULL rights_snapshot (рукотворная/будущая строка) не
        // должен ронять ВЕСЬ список с 500 — честный пустой snapshot (ревью 1.3).
        let rows: Vec<ConnectionRow> = sqlx::query_as(
            "SELECT marketplace, schemes, status, client_id, \
                    COALESCE(rights_snapshot, '[]'::jsonb), \
                    EXTRACT(EPOCH FROM verified_at)::bigint \
             FROM marketplace_connections \
             WHERE organization_id = $1 \
             ORDER BY marketplace",
        )
        .bind(organization_id)
        .fetch_all(&self.db)
        .await?;
        let mut connections = Vec::with_capacity(rows.len());
        for (marketplace_raw, schemes_json, status_raw, client_id, rights_json, verified_at) in rows
        {
            let marketplace = marketplace_from_str(&marketplace_raw)
                .ok_or(ConnectionsStoreError::CorruptData("marketplace"))?;
            let status =
                status_from_str(&status_raw).ok_or(ConnectionsStoreError::CorruptData("status"))?;
            let schemes_raw = schemes_json
                .as_array()
                .ok_or(ConnectionsStoreError::CorruptData("schemes"))?;
            let mut schemes = Vec::with_capacity(schemes_raw.len());
            for value in schemes_raw {
                let raw = value
                    .as_str()
                    .ok_or(ConnectionsStoreError::CorruptData("schemes"))?;
                schemes.push(
                    scheme_from_str(raw).ok_or(ConnectionsStoreError::CorruptData("schemes"))?,
                );
            }
            let rights_raw = rights_json
                .as_array()
                .ok_or(ConnectionsStoreError::CorruptData("rights"))?;
            let mut rights = Vec::with_capacity(rights_raw.len());
            for value in rights_raw {
                let dto: RightsCheckDto = serde_json::from_value(value.clone())
                    .map_err(|_| ConnectionsStoreError::CorruptData("rights"))?;
                rights.push(RightsCheck {
                    name: dto.name,
                    ok: dto.ok,
                    hint: dto.hint,
                });
            }
            connections.push(StoredConnection {
                marketplace,
                schemes,
                status,
                client_id,
                rights,
                verified_at_unix: verified_at,
            });
        }
        Ok(connections)
    }

    /// Удаление строки (токен отозван вместе с шифротекстом). Отсутствие
    /// строки — не ошибка (DELETE идемпотентен).
    async fn delete_connection(
        &self,
        organization_id: Uuid,
        marketplace: Marketplace,
    ) -> Result<(), ConnectionsStoreError> {
        sqlx::query(
            "DELETE FROM marketplace_connections WHERE organization_id = $1 AND marketplace = $2",
        )
        .bind(organization_id)
        .bind(marketplace_as_str(&marketplace))
        .execute(&self.db)
        .await?;
        Ok(())
    }
}

// --- Явный маппинг строковых литералов контракта /v1 ↔ enum домена (AD-1) ---

fn marketplace_from_str(raw: &str) -> Option<Marketplace> {
    match raw {
        "wildberries" => Some(Marketplace::Wildberries),
        "ozon" => Some(Marketplace::Ozon),
        _ => None,
    }
}

fn marketplace_as_str(marketplace: &Marketplace) -> &'static str {
    match marketplace {
        Marketplace::Wildberries => "wildberries",
        Marketplace::Ozon => "ozon",
    }
}

fn scheme_from_str(raw: &str) -> Option<Scheme> {
    match raw {
        "fbo" => Some(Scheme::Fbo),
        "fbs" => Some(Scheme::Fbs),
        "realfbs" => Some(Scheme::RealFbs),
        "rfbs" => Some(Scheme::Rfbs),
        _ => None,
    }
}

fn scheme_as_str(scheme: &Scheme) -> &'static str {
    match scheme {
        Scheme::Fbo => "fbo",
        Scheme::Fbs => "fbs",
        Scheme::RealFbs => "realfbs",
        Scheme::Rfbs => "rfbs",
    }
}

fn status_from_str(raw: &str) -> Option<ConnectionStatus> {
    match raw {
        "verified" => Some(ConnectionStatus::Verified),
        "rejected" => Some(ConnectionStatus::Rejected),
        "probe_unavailable" => Some(ConnectionStatus::ProbeUnavailable),
        _ => None,
    }
}

fn status_as_str(status: &ConnectionStatus) -> &'static str {
    match status {
        ConnectionStatus::Verified => "verified",
        ConnectionStatus::Rejected => "rejected",
        ConnectionStatus::ProbeUnavailable => "probe_unavailable",
    }
}

// --- DTO контрактов /v1 ---

/// Тело POST /v1/connections. Для Ozon `token` — это Api-Key, `client_id` —
/// Client-Id кабинета (не секрет). Для WB `client_id` не нужен (переданный
/// игнорируется — контракт без односторонних 422).
#[derive(Debug, Deserialize)]
struct ConnectRequestBody {
    marketplace: Option<String>,
    #[serde(default)]
    schemes: Vec<String>,
    token: Option<String>,
    client_id: Option<String>,
}

/// Ответ POST /v1/connections: `verified` → права; `rejected` /
/// `probe_unavailable` → диагностика по каждой пробе.
#[derive(Debug, PartialEq, Serialize)]
struct ConnectResponse {
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    rights: Option<Vec<RightsCheckDto>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    checks: Option<Vec<RightsCheckDto>>,
}

/// Ответ GET /v1/connections — статусы обеих площадок, без секретов.
#[derive(Debug, PartialEq, Serialize)]
struct ConnectionsResponse {
    connections: Vec<ConnectionDto>,
}

#[derive(Debug, PartialEq, Serialize)]
struct ConnectionDto {
    marketplace: &'static str,
    schemes: Vec<&'static str>,
    status: &'static str,
    /// Момент последнего успешного автотеста (RFC3339, UTC) или null.
    verified_at: Option<String>,
    /// Client-Id Ozon (не секрет); у WB — null.
    client_id: Option<String>,
    /// Snapshot прав последнего успешного автотеста.
    rights: Vec<RightsCheckDto>,
}

/// Проверка права в ответах (hint опускается, когда его нет — форма контракта
/// 200 `{name, ok:true}` для подтверждённых).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct RightsCheckDto {
    name: String,
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    hint: Option<String>,
}

impl RightsCheckDto {
    fn from_domain(check: &RightsCheck) -> Self {
        Self {
            name: check.name.clone(),
            ok: check.ok,
            hint: check.hint.clone(),
        }
    }
}

#[derive(Debug, PartialEq, Serialize)]
struct DeleteResponse {
    status: &'static str,
    marketplace: &'static str,
}

/// Ошибки валидации тела POST → 422-конверт (коды контракта 1.3).
#[derive(Debug, PartialEq, Eq)]
enum ConnectValidationError {
    /// Площадка вне Ф0 (wildberries/ozon).
    UnknownMarketplace(String),
    /// Пустой список схем или схема чужой площадки.
    InvalidSchemes,
    /// Токен пуст/повреждён/сверхдлинный.
    InvalidToken,
    /// Ozon без Client-Id.
    InvalidClientId,
}

/// Валидация тела запроса → домен. Строковая гигиена (trim, lower, dedup,
/// сортировка) — здесь; правила схем — в домене ([`vault::validate_schemes`]).
/// Выполняется ДО пробы и БД (422 работает даже в деградации).
fn connect_request_from_body(
    body: ConnectRequestBody,
) -> Result<(Marketplace, Vec<Scheme>, Credentials), ConnectValidationError> {
    let marketplace_raw = body.marketplace.unwrap_or_default();
    let marketplace = marketplace_from_str(marketplace_raw.trim().to_lowercase().as_str())
        .ok_or_else(|| ConnectValidationError::UnknownMarketplace(marketplace_raw.clone()))?;

    let mut schemes_raw: Vec<String> = body
        .schemes
        .iter()
        .map(|s| s.trim().to_lowercase())
        .collect();
    schemes_raw.sort();
    schemes_raw.dedup();
    if schemes_raw.is_empty() {
        return Err(ConnectValidationError::InvalidSchemes);
    }
    let mut schemes = Vec::with_capacity(schemes_raw.len());
    for raw in &schemes_raw {
        schemes.push(scheme_from_str(raw).ok_or(ConnectValidationError::InvalidSchemes)?);
    }
    vault::validate_schemes(marketplace, &schemes)
        .map_err(|_| ConnectValidationError::InvalidSchemes)?;

    let token = body.token.unwrap_or_default();
    let token = token.trim();
    vault::validate_token(token).map_err(|_| ConnectValidationError::InvalidToken)?;

    let credentials = match marketplace {
        Marketplace::Wildberries => Credentials::Wb {
            token: token.to_owned(),
        },
        Marketplace::Ozon => {
            let client_raw = body.client_id.unwrap_or_default();
            let client_id = client_raw.trim();
            vault::validate_client_id(client_id)
                .map_err(|_| ConnectValidationError::InvalidClientId)?;
            Credentials::Ozon {
                client_id: client_id.to_owned(),
                api_key: token.to_owned(),
            }
        }
    };
    Ok((marketplace, schemes, credentials))
}

/// 422-конверт ошибок валидации (коды: invalid_token / unknown_marketplace /
/// invalid_schemes / invalid_client_id).
fn validation_response(err: ConnectValidationError) -> (StatusCode, Json<ErrorEnvelope>) {
    let (code, message): (&str, String) = match err {
        ConnectValidationError::UnknownMarketplace(raw) => (
            "unknown_marketplace",
            format!(
                "площадка не распознана: {:?}; в Ф0 подключаются wildberries и ozon",
                echo(&raw)
            ),
        ),
        ConnectValidationError::InvalidSchemes => (
            "invalid_schemes",
            "набор схем невалиден: для wildberries — fbo, fbs, realfbs; \
             для ozon — fbo, fbs, rfbs; хотя бы одна схема обязательна"
                .to_owned(),
        ),
        ConnectValidationError::InvalidToken => (
            "invalid_token",
            "токен пуст или повреждён — вставьте API-токен площадки целиком".to_owned(),
        ),
        ConnectValidationError::InvalidClientId => (
            "invalid_client_id",
            "для Ozon нужен Client-Id кабинета вместе с Api-Key".to_owned(),
        ),
    };
    error_response(StatusCode::UNPROCESSABLE_ENTITY, code, message)
}

/// 500-конверт ошибки шифрования (без деталей секрета).
fn cipher_error_response() -> (StatusCode, Json<ErrorEnvelope>) {
    error_response(
        StatusCode::INTERNAL_SERVER_ERROR,
        "internal_error",
        "не удалось зашифровать токен — подключение не сохранено, повторите попытку",
    )
}

/// Маппит ошибку стора в HTTP-ответ: БД — 503, повреждённые данные — 500.
fn store_error_response(err: ConnectionsStoreError) -> (StatusCode, Json<ErrorEnvelope>) {
    match err {
        ConnectionsStoreError::Database(db_err) => {
            warn!(?db_err, "store подключений: ошибка БД");
            db_unavailable()
        }
        ConnectionsStoreError::CorruptData(field) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            format!("данные подключения повреждены: {field}"),
        ),
    }
}

/// Оборачивает sqlx-футуру стора подключений в бюджет {STORE_BUDGET:?}
/// (паттерн /health: зависший запрос — деградация, не зависший ответ).
async fn budgeted<T>(
    fut: impl Future<Output = Result<T, ConnectionsStoreError>>,
) -> Result<T, (StatusCode, Json<ErrorEnvelope>)> {
    match tokio::time::timeout(STORE_BUDGET, fut).await {
        Ok(res) => res.map_err(store_error_response),
        Err(_) => {
            warn!("store подключений: превышен бюджет {STORE_BUDGET:?} — БД не отвечает");
            Err(db_unavailable())
        }
    }
}

/// Читает placement-параметры запроса (IDENTITY — как в шаге 0).
fn params_from(
    query: Result<Query<HashMap<String, String>>, QueryRejection>,
) -> Result<HashMap<String, String>, (StatusCode, Json<ErrorEnvelope>)> {
    match query {
        Ok(Query(params)) => Ok(params),
        Err(rejection) => Err(error_response(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_query",
            format!(
                "query-параметры не читаются: {}",
                echo(rejection.body_text().as_str())
            ),
        )),
    }
}

fn query_rejection_response(rejection: &JsonRejection) -> (StatusCode, Json<ErrorEnvelope>) {
    error_response(
        StatusCode::UNPROCESSABLE_ENTITY,
        "invalid_body",
        format!(
            "тело запроса не читается: {}",
            echo(rejection.body_text().as_str())
        ),
    )
}

// --- Хендлеры ---

/// `GET /v1/connections` — статусы подключений обеих площадок (без секретов).
/// Портал без подключения → пустой список (write-on-GET нет: организация не
/// создаётся чтением).
async fn list_connections(
    State(state): State<AppState>,
    query: Result<Query<HashMap<String, String>>, QueryRejection>,
) -> ApiResult<ConnectionsResponse> {
    let params = params_from(query)?;
    let identity = PlacementIdentity::from_params(&params).map_err(placement_error_response)?;
    let organization_id = budgeted(
        state
            .connections
            .store
            .resolve_organization(&identity.member_id),
    )
    .await?;
    let rows = match organization_id {
        None => Vec::new(),
        Some(id) => budgeted(state.connections.store.list_connections(id)).await?,
    };
    let connections = rows.iter().map(connection_dto).collect();
    Ok((StatusCode::OK, Json(ConnectionsResponse { connections })))
}

/// Маппит строку БД (домен) в DTO контракта; verified_at — RFC3339 UTC.
fn connection_dto(row: &StoredConnection) -> ConnectionDto {
    let verified_at = row.verified_at_unix.and_then(|seconds| {
        chrono::DateTime::from_timestamp(seconds, 0)
            .map(|dt| dt.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
    });
    ConnectionDto {
        marketplace: marketplace_as_str(&row.marketplace),
        schemes: row.schemes.iter().map(scheme_as_str).collect(),
        status: status_as_str(&row.status),
        verified_at,
        client_id: row.client_id.clone(),
        rights: row.rights.iter().map(RightsCheckDto::from_domain).collect(),
    }
}

/// `POST /v1/connections` — автотест прав пробами площадки; при успехе —
/// шифрование и перезапись подключения (ротация = повторный POST).
///
/// Порядок честного ответа (спека 1.3): сетевые проблемы площадки ≠ отказ
/// прав — отдельный статус probe_unavailable. Неуспешный автотест токен НЕ
/// сохраняет: rejected/probe_unavailable не касаются БД вовсе (и не создают
/// организации-мусора); проверка до записи — при отказе прав прежний
/// шифротекст остаётся нетронутым.
async fn create_connection(
    State(state): State<AppState>,
    query: Result<Query<HashMap<String, String>>, QueryRejection>,
    body: Result<Json<ConnectRequestBody>, JsonRejection>,
) -> ApiResult<ConnectResponse> {
    let Json(body) = body.map_err(|rejection| query_rejection_response(&rejection))?;
    // Валидация до пробы и БД: 422 без касания БД/площадки.
    let (marketplace, schemes, credentials) =
        connect_request_from_body(body).map_err(validation_response)?;
    let params = params_from(query)?;
    let identity = PlacementIdentity::from_params(&params).map_err(placement_error_response)?;

    // Пробы прав (ДО сохранения и БД): каждая проба — лёгкий запрос площадки
    // с таймаутом 10с; транспортная ошибка — probe_unavailable, не отказ прав.
    let probe_checks = state
        .connections
        .probe
        .probe_rights(marketplace, &credentials)
        .await;
    let summary = summarize_probes(&probe_checks);

    match summary.status {
        ConnectionStatus::Verified => {
            // Шифрование до записи (plaintext дальше в БД не идёт).
            let secret = state
                .connections
                .cipher
                .encrypt(credentials.secret().as_bytes())
                .map_err(|err| {
                    warn!(
                        ?err,
                        marketplace = marketplace_as_str(&marketplace),
                        "vault: шифрование не выполнено"
                    );
                    cipher_error_response()
                })?;
            info!(
                marketplace = marketplace_as_str(&marketplace),
                key_version = secret.key_version,
                token_mask = %show_last4(credentials.secret()),
                "автотест прав подтверждён — записывается шифротекст (plaintext нигде не хранится)"
            );
            // POST — write-путь: организация может создаваться подключением.
            let organization_id = budgeted(async {
                state
                    .perimeter
                    .ensure_organization(&identity.member_id)
                    .await
                    .map_err(ConnectionsStoreError::Database)
            })
            .await?;
            let connection = VerifiedConnection {
                marketplace,
                schemes: schemes.clone(),
                client_id: credentials.client_id().map(str::to_owned),
                rights: summary.checks.clone(),
                secret,
            };
            budgeted(
                state
                    .connections
                    .store
                    .save_connection(organization_id, &connection),
            )
            .await?;
            Ok((
                StatusCode::OK,
                Json(ConnectResponse {
                    status: "verified",
                    rights: Some(
                        summary
                            .checks
                            .iter()
                            .map(RightsCheckDto::from_domain)
                            .collect(),
                    ),
                    checks: None,
                }),
            ))
        }
        ConnectionStatus::Rejected => {
            warn!(
                marketplace = marketplace_as_str(&marketplace),
                member_id = %identity.member_id,
                "автотест прав отклонён — токен НЕ сохранён, прежние данные целы"
            );
            Ok((
                StatusCode::OK,
                Json(ConnectResponse {
                    status: "rejected",
                    rights: None,
                    checks: Some(
                        summary
                            .checks
                            .iter()
                            .map(RightsCheckDto::from_domain)
                            .collect(),
                    ),
                }),
            ))
        }
        ConnectionStatus::ProbeUnavailable => {
            warn!(
                marketplace = marketplace_as_str(&marketplace),
                member_id = %identity.member_id,
                "площадка не ответила на пробы — подключение НЕ сохранено"
            );
            Ok((
                StatusCode::OK,
                Json(ConnectResponse {
                    status: "probe_unavailable",
                    rights: None,
                    checks: Some(
                        summary
                            .checks
                            .iter()
                            .map(RightsCheckDto::from_domain)
                            .collect(),
                    ),
                }),
            ))
        }
    }
}

/// `DELETE /v1/connections/{marketplace}` — удаление подключения (токен
/// отозван вместе со строкой). Идемпотентно: отсутствие строки — 200.
async fn delete_connection(
    State(state): State<AppState>,
    query: Result<Query<HashMap<String, String>>, QueryRejection>,
    Path(marketplace_raw): Path<String>,
) -> ApiResult<DeleteResponse> {
    let marketplace = marketplace_from_str(marketplace_raw.trim().to_lowercase().as_str())
        .ok_or_else(|| {
            validation_response(ConnectValidationError::UnknownMarketplace(
                marketplace_raw.clone(),
            ))
        })?;
    let params = params_from(query)?;
    let identity = PlacementIdentity::from_params(&params).map_err(placement_error_response)?;
    let organization_id = budgeted(
        state
            .connections
            .store
            .resolve_organization(&identity.member_id),
    )
    .await?;
    if let Some(organization_id) = organization_id {
        budgeted(
            state
                .connections
                .store
                .delete_connection(organization_id, marketplace),
        )
        .await?;
        info!(
            marketplace = marketplace_as_str(&marketplace),
            organization_id = %organization_id,
            "подключение удалено — шифротекст токена отозван вместе со строкой"
        );
    }
    Ok((
        StatusCode::OK,
        Json(DeleteResponse {
            status: "deleted",
            marketplace: marketplace_as_str(&marketplace),
        }),
    ))
}

/// Роутер подключений (подключается в main рядом с шагом 0).
pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/v1/connections",
            get(list_connections).post(create_connection),
        )
        .route("/v1/connections/{marketplace}", delete(delete_connection))
}

/// Зависимости маршрутов подключений в состоянии шлюза (композиция портов):
/// sqlx-стор, AES-шифратор и проба прав (в тестах — мок).
#[derive(Clone)]
pub(crate) struct ConnectionsDeps {
    pub(crate) store: PgConnectionStore,
    pub(crate) cipher: Arc<EnvAesGcmCipher>,
    pub(crate) probe: Arc<dyn RightsProbe>,
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_KEY: [u8; 32] = [0x42; 32];

    fn cipher() -> EnvAesGcmCipher {
        EnvAesGcmCipher::new(Arc::new(EnvMasterKeyProvider::new(TEST_KEY)))
    }

    mod cipher {
        use super::*;

        #[test]
        fn roundtrip_preserves_plaintext() {
            let cipher = cipher();
            let payload = cipher
                .encrypt("wb-токен-123 🔐".as_bytes())
                .expect("шифрование выполнимо");
            assert_eq!(payload.key_version, 1);
            assert_eq!(payload.nonce.len(), 12);
            let plaintext = cipher.decrypt(&payload).expect("расшифровка выполнима");
            assert_eq!(plaintext, "wb-токен-123 🔐".as_bytes());
        }

        #[test]
        fn nonce_and_ciphertext_unique_per_record() {
            // Инвариант AD-16: уникальный nonce на запись.
            let cipher = cipher();
            let first = cipher.encrypt(b"same-token").expect("первая запись");
            let second = cipher.encrypt(b"same-token").expect("вторая запись");
            assert_ne!(first.nonce, second.nonce);
            assert_ne!(first.ciphertext, second.ciphertext);
            // Обе записи расшифровываются в тот же plaintext.
            assert_eq!(
                cipher.decrypt(&first).as_deref(),
                Ok(b"same-token".as_ref())
            );
            assert_eq!(
                cipher.decrypt(&second).as_deref(),
                Ok(b"same-token".as_ref())
            );
        }

        #[test]
        fn tampered_ciphertext_is_decrypt_error() {
            let cipher = cipher();
            let mut payload = cipher.encrypt(b"secret-token").expect("шифрование");
            payload.ciphertext[0] ^= 0xFF;
            assert_eq!(cipher.decrypt(&payload), Err(CipherError::Decrypt));
        }

        #[test]
        fn tampered_nonce_is_decrypt_error() {
            let cipher = cipher();
            let mut payload = cipher.encrypt(b"secret-token").expect("шифрование");
            payload.nonce[0] ^= 0xFF;
            assert_eq!(cipher.decrypt(&payload), Err(CipherError::Decrypt));
        }

        #[test]
        fn unknown_key_version_is_rejected() {
            let cipher = cipher();
            let payload = cipher.encrypt(b"secret-token").expect("шифрование");
            let foreign = EncryptedPayload {
                ciphertext: payload.ciphertext,
                nonce: payload.nonce,
                key_version: 2,
            };
            assert_eq!(
                cipher.decrypt(&foreign),
                Err(CipherError::UnknownKeyVersion(2))
            );
        }

        #[test]
        fn broken_nonce_length_is_rejected() {
            let cipher = cipher();
            let payload = cipher.encrypt(b"secret-token").expect("шифрование");
            let broken = EncryptedPayload {
                ciphertext: payload.ciphertext,
                nonce: vec![0u8; 8],
                key_version: 1,
            };
            assert_eq!(cipher.decrypt(&broken), Err(CipherError::NonceLength));
        }

        #[test]
        fn provider_exposes_only_version_1() {
            let provider = EnvMasterKeyProvider::new(TEST_KEY);
            assert_eq!(provider.active_key().version, 1);
            assert_eq!(provider.active_key().bytes, TEST_KEY);
            assert!(provider.key_for_version(1).is_some());
            assert!(provider.key_for_version(2).is_none());
        }
    }

    mod masking {
        use super::*;
        #[test]
        fn master_key_debug_never_prints_bytes() {
            // Ревью 1.3: {:?} MasterKey (портом гуляет поAppState/логам) не
            // печатает байты ключа — только версию и маску.
            let master = MasterKey {
                version: 1,
                bytes: [0xAB; 32],
            };
            let debug = format!("{master:?}");
            assert!(!debug.contains("abab"), "Debug протёк байтами: {debug}");
            assert!(
                debug.contains("redacted"),
                "Debug обязан маскировать: {debug}"
            );
        }

        #[test]
        fn long_secret_shows_only_last4() {
            assert_eq!(show_last4("eyAbcdefghijk1234"), "…1234");
        }

        #[test]
        fn short_secret_is_fully_masked() {
            assert_eq!(show_last4("abc"), "****");
            assert_eq!(show_last4("abcd"), "****");
        }

        #[test]
        fn surrounding_whitespace_is_trimmed_before_masking() {
            assert_eq!(show_last4("  eyAbcdefghijk1234  "), "…1234");
        }

        #[test]
        fn unicode_secret_masks_last_chars() {
            assert_eq!(show_last4("токен-абвг"), "…абвг");
        }
    }

    mod probe_matrix {
        use super::*;

        #[test]
        fn wb_specs_hit_verified_endpoints_with_token_only_in_headers() {
            let token = "wb-secret-token-ABCD";
            let specs = build_probe_specs(
                Marketplace::Wildberries,
                &Credentials::Wb {
                    token: token.to_owned(),
                },
                "2026-10-28",
                None,
            );
            assert_eq!(specs.len(), 2);
            assert_eq!(specs[0].name, "Статистика");
            assert_eq!(
                specs[0].url,
                format!("{WB_STATISTICS_HOST}/api/v1/supplier/stocks?dateFrom=2026-10-28")
            );
            assert_eq!(specs[0].method, "GET");
            assert_eq!(specs[1].name, "Цены и скидки");
            assert_eq!(
                specs[1].url,
                format!("{WB_PRICES_HOST}/api/v2/list/goods/filter?limit=1")
            );
            for spec in &specs {
                // Токен — в заголовке Authorization, НЕ в URL (URL попадает в
                // debug-логи; заголовки — нет).
                assert_eq!(spec.headers[0].0, "Authorization");
                assert_eq!(spec.headers[0].1, token);
                assert!(
                    !spec.url.contains(token),
                    "токен протёк в URL: {}",
                    spec.url
                );
                assert!(spec.auth_error_marker.is_none());
            }
        }

        #[test]
        fn ozon_spec_posts_product_list_with_client_headers() {
            let specs = build_probe_specs(
                Marketplace::Ozon,
                &Credentials::Ozon {
                    client_id: "42".to_owned(),
                    api_key: "ozon-key-XYZ".to_owned(),
                },
                "2026-10-28",
                None,
            );
            assert_eq!(specs.len(), 1);
            let spec = &specs[0];
            assert_eq!(spec.name, "Товары");
            assert_eq!(spec.url, format!("{OZON_HOST}/v3/product/list"));
            assert_eq!(spec.method, "POST");
            assert_eq!(spec.body.as_deref(), Some("{\"limit\":1}"));
            assert_eq!(spec.headers[0], ("Client-Id", "42".to_owned()));
            assert_eq!(spec.headers[1], ("Api-Key", "ozon-key-XYZ".to_owned()));
            assert!(!spec.url.contains("ozon-key-XYZ"));
            // Маркерcredentials-ошибки: Ozon отвечает 400 «Invalid Api-Key»
            // на неверный ключ (верифицировано curl, 2026-10-28).
            assert_eq!(spec.auth_error_marker, Some("Invalid Api-Key"));
        }

        #[test]
        fn override_host_replaces_all_probe_hosts() {
            let specs = build_probe_specs(
                Marketplace::Wildberries,
                &Credentials::Wb {
                    token: "t".to_owned(),
                },
                "2026-10-28",
                Some("http://127.0.0.1:9"),
            );
            for spec in &specs {
                assert!(spec.url.starts_with("http://127.0.0.1:9/"));
            }
            let ozon = build_probe_specs(
                Marketplace::Ozon,
                &Credentials::Ozon {
                    client_id: "1".to_owned(),
                    api_key: "k".to_owned(),
                },
                "2026-10-28",
                Some("http://127.0.0.1:9"),
            );
            assert!(ozon[0].url.starts_with("http://127.0.0.1:9/"));
        }

        #[test]
        fn classify_two_hundreds_confirm() {
            let spec = wb_spec("Статистика");
            for status in [200, 201, 204, 299] {
                assert_eq!(
                    classify_probe_response(status, "", &spec),
                    ProbeResult::Confirmed,
                    "статус {status}"
                );
            }
        }

        #[test]
        fn classify_auth_denials_with_hint() {
            let spec = wb_spec("Статистика");
            for status in [401, 403] {
                match classify_probe_response(status, "{}", &spec) {
                    ProbeResult::Denied(hint) => {
                        assert!(
                            hint.contains("Статистика"),
                            "подсказка называет право: {hint}"
                        );
                    }
                    other => panic!("ожидался Denied, получен {other:?}"),
                }
            }
        }

        #[test]
        fn classify_ozon_400_marker_is_denied_but_plain_400_is_unreachable() {
            let spec = ozon_spec();
            // Верифицированный ответ Ozon на неверный ключ — 400 + код 3.
            let marker_body =
                r#"{"code":3,"message":"Invalid Api-Key, please check the key and try again"}"#;
            assert!(matches!(
                classify_probe_response(400, marker_body, &spec),
                ProbeResult::Denied(_)
            ));
            // Прочие 400 (наш сломанный запрос) — честная недоступность.
            assert_eq!(
                classify_probe_response(400, r#"{"code":7,"message":"other"}"#, &spec),
                ProbeResult::Unreachable
            );
        }

        #[test]
        fn classify_other_codes_are_unreachable() {
            let spec = wb_spec("Статистика");
            for status in [400, 404, 429, 500, 502, 503] {
                assert_eq!(
                    classify_probe_response(status, "anything", &spec),
                    ProbeResult::Unreachable,
                    "статус {status} — площадка не ответила валидной пробой"
                );
            }
        }

        #[tokio::test]
        async fn mismatched_credentials_get_honest_unreachable() {
            // Креденшелы всегда собираются с площадкой вместе; рассинхрон —
            // программная ошибка: честный Unreachable вместо паники/пустой
            // «подтверждённости».
            let probe = HttpRightsProbe::new(None).expect("клиент собран");
            let checks = probe
                .probe_rights(
                    Marketplace::Wildberries,
                    &Credentials::Ozon {
                        client_id: "1".to_owned(),
                        api_key: "k".to_owned(),
                    },
                )
                .await;
            assert_eq!(checks.len(), 1);
            assert_eq!(checks[0].result, ProbeResult::Unreachable);
        }

        fn wb_spec(name: &'static str) -> ProbeSpec {
            ProbeSpec {
                name,
                method: "GET",
                url: format!("{WB_STATISTICS_HOST}/api/v1/supplier/stocks?dateFrom=2026-10-28"),
                headers: vec![("Authorization", "t".to_owned())],
                body: None,
                auth_error_marker: None,
                denial_hint: "в кабинете Wildberries пересоздайте токен с категорией «Статистика»",
            }
        }

        fn ozon_spec() -> ProbeSpec {
            ProbeSpec {
                name: "Товары",
                method: "POST",
                url: format!("{OZON_HOST}/v3/product/list"),
                headers: vec![("Client-Id", "42".to_owned()), ("Api-Key", "k".to_owned())],
                body: Some("{\"limit\":1}".to_owned()),
                auth_error_marker: Some("Invalid Api-Key"),
                denial_hint: "проверьте Client-Id и Api-Key",
            }
        }
    }

    mod literal_mapping {
        use super::*;

        #[test]
        fn marketplace_literals_roundtrip() {
            for literal in ["wildberries", "ozon"] {
                let mp = marketplace_from_str(literal)
                    .unwrap_or_else(|| panic!("литерал {literal} распознаётся"));
                assert_eq!(marketplace_as_str(&mp), literal);
            }
            for bad in ["", "Wildberries", "yandex_market", "aliexpress"] {
                assert_eq!(marketplace_from_str(bad), None, "литерал {bad:?} не из Ф0");
            }
        }

        #[test]
        fn scheme_literals_roundtrip() {
            for literal in ["fbo", "fbs", "realfbs", "rfbs"] {
                let scheme = scheme_from_str(literal)
                    .unwrap_or_else(|| panic!("литерал {literal} распознаётся"));
                assert_eq!(scheme_as_str(&scheme), literal);
            }
            for bad in ["", "FBO", "dbf", "dropship"] {
                assert_eq!(scheme_from_str(bad), None, "литерал {bad:?} не схема");
            }
        }

        #[test]
        fn status_literals_roundtrip() {
            let cases = [
                ("verified", ConnectionStatus::Verified),
                ("rejected", ConnectionStatus::Rejected),
                ("probe_unavailable", ConnectionStatus::ProbeUnavailable),
            ];
            for (literal, status) in cases {
                assert_eq!(status_from_str(literal), Some(status));
                assert_eq!(status_as_str(&status), literal);
            }
            assert_eq!(status_from_str("connected"), None);
        }
    }

    mod request_validation {
        use super::*;

        fn body(
            marketplace: &str,
            schemes: &[&str],
            token: &str,
            client_id: Option<&str>,
        ) -> ConnectRequestBody {
            ConnectRequestBody {
                marketplace: Some(marketplace.to_owned()),
                schemes: schemes.iter().map(|s| (*s).to_owned()).collect(),
                token: Some(token.to_owned()),
                client_id: client_id.map(str::to_owned),
            }
        }

        #[test]
        fn valid_wb_request_is_normalized() {
            let (marketplace, schemes, credentials) = connect_request_from_body(body(
                " Wildberries ",
                &[" FBS ", "FBO", "fbo", "realfbs"],
                " wb-token ",
                Some("ignored"),
            ))
            .expect("валидный запрос WB");
            assert_eq!(marketplace, Marketplace::Wildberries);
            // Нормализация: trim + lower + sort + dedup.
            assert_eq!(schemes, [Scheme::Fbo, Scheme::Fbs, Scheme::RealFbs]);
            match credentials {
                Credentials::Wb { token } => assert_eq!(token, "wb-token"),
                _ => panic!("WB-креденшелы"),
            }
        }

        #[test]
        fn valid_ozon_request_carries_client_id() {
            let (marketplace, schemes, credentials) = connect_request_from_body(body(
                "ozon",
                &["fbo", "fbs", "rfbs"],
                "api-key",
                Some(" 42 "),
            ))
            .expect("валидный запрос Ozon");
            assert_eq!(marketplace, Marketplace::Ozon);
            assert_eq!(schemes, [Scheme::Fbo, Scheme::Fbs, Scheme::Rfbs]);
            match credentials {
                Credentials::Ozon { client_id, api_key } => {
                    assert_eq!(client_id, "42");
                    assert_eq!(api_key, "api-key");
                }
                _ => panic!("Ozon-креденшелы"),
            }
        }

        #[test]
        fn unknown_marketplace_is_rejected() {
            let err = connect_request_from_body(body("aliexpress", &["fbo"], "t", None))
                .expect_err("площадка вне Ф0 — 422");
            assert_eq!(
                err,
                ConnectValidationError::UnknownMarketplace("aliexpress".to_owned())
            );
            let empty = connect_request_from_body(body("", &["fbo"], "t", None))
                .expect_err("пустая площадка — 422");
            assert_eq!(
                empty,
                ConnectValidationError::UnknownMarketplace(String::new())
            );
        }

        #[test]
        fn empty_schemes_are_invalid() {
            let err = connect_request_from_body(body("wildberries", &[], "t", None))
                .expect_err("пустые схемы — 422");
            assert_eq!(err, ConnectValidationError::InvalidSchemes);
        }

        #[test]
        fn foreign_schemes_are_invalid() {
            // realfbs — только WB; rfbs — только Ozon (I/O-матрица).
            let ozon_realfbs =
                connect_request_from_body(body("ozon", &["realfbs"], "t", Some("1")))
                    .expect_err("realfbs у Ozon — 422");
            assert_eq!(ozon_realfbs, ConnectValidationError::InvalidSchemes);
            let wb_rfbs =
                connect_request_from_body(body("wildberries", &["fbo", "rfbs"], "t", None))
                    .expect_err("rfbs у WB — 422");
            assert_eq!(wb_rfbs, ConnectValidationError::InvalidSchemes);
        }

        #[test]
        fn unknown_scheme_literal_is_invalid() {
            let err = connect_request_from_body(body("wildberries", &["dbf"], "t", None))
                .expect_err("нераспознанная схема — 422");
            assert_eq!(err, ConnectValidationError::InvalidSchemes);
        }

        #[test]
        fn broken_tokens_are_invalid() {
            for token in ["", "   ", "bad\ntoken", &"x".repeat(1025)] {
                let err = connect_request_from_body(body("wildberries", &["fbo"], token, None))
                    .expect_err("токен {token:?} — 422");
                assert_eq!(err, ConnectValidationError::InvalidToken, "токен {token:?}");
            }
        }

        #[test]
        fn ozon_requires_client_id() {
            let err = connect_request_from_body(body("ozon", &["fbo"], "key", None))
                .expect_err("Ozon без client_id — 422");
            assert_eq!(err, ConnectValidationError::InvalidClientId);
            let blank = connect_request_from_body(body("ozon", &["fbo"], "key", Some("   ")))
                .expect_err("Ozon с пустым client_id — 422");
            assert_eq!(blank, ConnectValidationError::InvalidClientId);
        }

        #[test]
        fn wb_ignores_client_id_without_error() {
            // Контракт без односторонних 422: WB-форма не отправляет client_id,
            // а переданное лишнее не должно ломать подключение.
            assert!(
                connect_request_from_body(body("wildberries", &["fbo"], "t", Some("42"))).is_ok()
            );
        }

        #[test]
        fn validation_errors_map_to_contract_codes() {
            let cases = [
                (
                    ConnectValidationError::UnknownMarketplace("ali".to_owned()),
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "unknown_marketplace",
                ),
                (
                    ConnectValidationError::InvalidSchemes,
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "invalid_schemes",
                ),
                (
                    ConnectValidationError::InvalidToken,
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "invalid_token",
                ),
                (
                    ConnectValidationError::InvalidClientId,
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "invalid_client_id",
                ),
            ];
            for (err, status, code) in cases {
                let (got_status, Json(envelope)) = validation_response(err);
                assert_eq!(got_status, status);
                assert_eq!(envelope.error.code, code);
                assert!(!envelope.error.message.is_empty());
                assert!(envelope.error.request_id.is_some());
            }
        }

        #[test]
        fn huge_marketplace_echo_is_truncated() {
            let (_, Json(envelope)) =
                validation_response(ConnectValidationError::UnknownMarketplace("x".repeat(300)));
            assert!(envelope.error.message.chars().count() < 200);
        }
    }

    mod response_shapes {
        use super::*;

        #[test]
        fn verified_response_shape() {
            let value = serde_json::to_value(ConnectResponse {
                status: "verified",
                rights: Some(vec![RightsCheckDto {
                    name: "Статистика".to_owned(),
                    ok: true,
                    hint: None,
                }]),
                checks: None,
            })
            .expect("сериализуемо");
            // Спека: 200 {status:"verified", rights:[{name, ok:true}]} —
            // hint отсутствует, checks не сериализуются.
            assert_eq!(
                value,
                serde_json::json!({
                    "status": "verified",
                    "rights": [{"name": "Статистика", "ok": true}]
                })
            );
        }

        #[test]
        fn rejected_response_shape() {
            let value = serde_json::to_value(ConnectResponse {
                status: "rejected",
                rights: None,
                checks: Some(vec![RightsCheckDto {
                    name: "Цены и скидки".to_owned(),
                    ok: false,
                    hint: Some("в кабинете Wildberries пересоздайте токен".to_owned()),
                }]),
            })
            .expect("сериализуемо");
            assert_eq!(
                value,
                serde_json::json!({
                    "status": "rejected",
                    "checks": [{
                        "name": "Цены и скидки",
                        "ok": false,
                        "hint": "в кабинете Wildberries пересоздайте токен"
                    }]
                })
            );
        }

        #[test]
        fn connection_row_shape_with_client_id_and_verified_at() {
            let row = StoredConnection {
                marketplace: Marketplace::Ozon,
                schemes: vec![Scheme::Fbo, Scheme::Fbs, Scheme::Rfbs],
                status: ConnectionStatus::Verified,
                client_id: Some("42".to_owned()),
                rights: vec![RightsCheck {
                    name: "Товары".to_owned(),
                    ok: true,
                    hint: None,
                }],
                verified_at_unix: Some(0),
            };
            let value = serde_json::to_value(ConnectionsResponse {
                connections: vec![connection_dto(&row)],
            })
            .expect("сериализуемо");
            assert_eq!(
                value,
                serde_json::json!({
                    "connections": [{
                        "marketplace": "ozon",
                        "schemes": ["fbo", "fbs", "rfbs"],
                        "status": "verified",
                        "verified_at": "1970-01-01T00:00:00Z",
                        "client_id": "42",
                        "rights": [{"name": "Товары", "ok": true}]
                    }]
                })
            );
        }

        #[test]
        fn wb_row_has_null_client_id() {
            let row = StoredConnection {
                marketplace: Marketplace::Wildberries,
                schemes: vec![Scheme::Fbo],
                status: ConnectionStatus::Verified,
                client_id: None,
                rights: vec![],
                verified_at_unix: None,
            };
            let value = serde_json::to_value(connection_dto(&row)).expect("сериализуемо");
            assert_eq!(value["client_id"], serde_json::Value::Null);
            assert_eq!(value["verified_at"], serde_json::Value::Null);
        }

        #[test]
        fn delete_response_shape() {
            let value = serde_json::to_value(DeleteResponse {
                status: "deleted",
                marketplace: "wildberries",
            })
            .expect("сериализуемо");
            assert_eq!(
                value,
                serde_json::json!({"status": "deleted", "marketplace": "wildberries"})
            );
        }

        #[test]
        fn cipher_error_response_is_internal_500() {
            let (status, Json(envelope)) = cipher_error_response();
            assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
            assert_eq!(envelope.error.code, "internal_error");
        }

        #[test]
        fn corrupt_data_maps_to_500_and_db_to_503() {
            let (corrupt_status, Json(corrupt)) =
                store_error_response(ConnectionsStoreError::CorruptData("status"));
            assert_eq!(corrupt_status, StatusCode::INTERNAL_SERVER_ERROR);
            assert_eq!(corrupt.error.code, "internal_error");

            let (db_status, Json(db)) =
                store_error_response(ConnectionsStoreError::Database(sqlx::Error::RowNotFound));
            assert_eq!(db_status, StatusCode::SERVICE_UNAVAILABLE);
            assert_eq!(db.error.code, "database_unavailable");
        }
    }

    // --- Хендлеры: мок-проба (ok/403/таймаут) + lazy-пул без БД ---

    /// Мок пробы: возвращает заготовленные проверки (инфалибельный порт).
    struct MockProbe(Vec<ProbeCheck>);

    impl RightsProbe for MockProbe {
        fn probe_rights<'a>(
            &'a self,
            _marketplace: Marketplace,
            _credentials: &'a Credentials,
        ) -> Pin<Box<dyn Future<Output = Vec<ProbeCheck>> + Send + 'a>> {
            Box::pin(async move { self.0.clone() })
        }
    }

    /// Состояние шлюза для хендлер-тестов: lazy-пул на закрытый порт —
    /// любое касание БД даёт 503 (и ломает тест, если ветка не должна
    /// касаться БД).
    fn state_with(probe: Arc<dyn RightsProbe>) -> AppState {
        let db = sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgres://vault-test:stub@127.0.0.1:1/vault-test")
            .expect("lazy-пул собирается без подключения");
        AppState {
            db: db.clone(),
            version: "test",
            migrations_applied: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            perimeter: crate::onboarding::PgPerimeterStore::new(db.clone()),
            connections: crate::ConnectionsDeps {
                store: PgConnectionStore::new(db),
                cipher: Arc::new(cipher()),
                probe,
            },
        }
    }

    fn params(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    fn confirmed_checks() -> Vec<ProbeCheck> {
        vec![
            ProbeCheck {
                name: "Статистика".to_owned(),
                result: ProbeResult::Confirmed,
            },
            ProbeCheck {
                name: "Цены и скидки".to_owned(),
                result: ProbeResult::Confirmed,
            },
        ]
    }

    async fn post_connection(
        state: AppState,
        body: ConnectRequestBody,
    ) -> Result<(StatusCode, Json<ConnectResponse>), (StatusCode, Json<ErrorEnvelope>)> {
        create_connection(State(state), Ok(Query(HashMap::new())), Ok(Json(body))).await
    }

    fn wb_body() -> ConnectRequestBody {
        ConnectRequestBody {
            marketplace: Some("wildberries".to_owned()),
            schemes: vec!["fbo".to_owned(), "fbs".to_owned(), "realfbs".to_owned()],
            token: Some("wb-ci-token-ABCD".to_owned()),
            client_id: None,
        }
    }

    mod handlers {
        use super::*;

        #[tokio::test]
        async fn rejected_probe_returns_200_and_never_touches_db() {
            // Токен жив, право не подтвердилось: 200 rejected + диагностика;
            // ни одной операции БД (lazy-пул на закрытый порт ответил бы 503).
            let state = state_with(Arc::new(MockProbe(vec![ProbeCheck {
                name: "Статистика".to_owned(),
                result: ProbeResult::Denied("в кабинете WB пересоздайте токен".to_owned()),
            }])));
            let (status, Json(response)) =
                post_connection(state, wb_body()).await.expect("ответ 200");
            assert_eq!(status, StatusCode::OK);
            assert_eq!(response.status, "rejected");
            assert!(response.rights.is_none());
            let checks = response.checks.expect("диагностика по пробам");
            assert_eq!(checks.len(), 1);
            assert!(!checks[0].ok);
            assert!(checks[0].hint.as_deref().unwrap().contains("WB"));
        }

        #[tokio::test]
        async fn probe_unavailable_returns_200_and_never_touches_db() {
            // Таймаут/недоступность площадки — честная ошибка, не «подключено».
            let state = state_with(Arc::new(MockProbe(vec![ProbeCheck {
                name: "Цены и скидки".to_owned(),
                result: ProbeResult::Unreachable,
            }])));
            let (status, Json(response)) =
                post_connection(state, wb_body()).await.expect("ответ 200");
            assert_eq!(status, StatusCode::OK);
            assert_eq!(response.status, "probe_unavailable");
            let checks = response.checks.expect("диагностика по пробам");
            assert_eq!(checks[0].hint.as_deref(), Some(vault::UNREACHABLE_HINT));
        }

        #[tokio::test]
        async fn verified_probe_attempts_save_and_reports_db_outage_honestly() {
            // Без БД verified-путь честно падает 503 на записи: подтверждённая
            // проба ДОХОДИТ до шифратора и стора (в отличие от rejected).
            let state = state_with(Arc::new(MockProbe(confirmed_checks())));
            let Err((status, Json(envelope))) = post_connection(state, wb_body()).await else {
                panic!("без БД сохранение обязано уйти в 503");
            };
            assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
            assert_eq!(envelope.error.code, "database_unavailable");
        }

        #[tokio::test]
        async fn invalid_body_is_422_without_probe_or_db() {
            // Валидация до пробы и БД: unknown marketplace на мок-пробе,
            // который бы ответил «confirmed» — всё равно 422.
            let state = state_with(Arc::new(MockProbe(confirmed_checks())));
            let mut body = wb_body();
            body.marketplace = Some("aliexpress".to_owned());
            let Err((status, Json(envelope))) = post_connection(state, body).await else {
                panic!("валидация обязана отклонить запрос");
            };
            assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
            assert_eq!(envelope.error.code, "unknown_marketplace");
        }

        #[tokio::test]
        async fn partial_placement_is_422_before_probe() {
            let state = state_with(Arc::new(MockProbe(confirmed_checks())));
            let Err((status, Json(envelope))) = create_connection(
                State(state),
                Ok(Query(params(&[("DOMAIN", "shop.example")]))),
                Ok(Json(wb_body())),
            )
            .await
            else {
                panic!("частичный placement обязан быть отклонён");
            };
            assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
            assert_eq!(envelope.error.code, "invalid_placement");
        }

        #[tokio::test]
        async fn get_without_db_is_503_envelope() {
            // Живой шлюз без БД: GET /v1/connections → 503-конверт.
            let state = state_with(Arc::new(MockProbe(Vec::new())));
            let Err((status, Json(envelope))) =
                list_connections(State(state), Ok(Query(HashMap::new()))).await
            else {
                panic!("чтение без БД обязано уйти в 503");
            };
            assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
            assert_eq!(envelope.error.code, "database_unavailable");
        }

        #[tokio::test]
        async fn delete_unknown_marketplace_is_422_before_db() {
            let state = state_with(Arc::new(MockProbe(Vec::new())));
            let Err((status, Json(envelope))) = delete_connection(
                State(state),
                Ok(Query(HashMap::new())),
                Path("aliexpress".to_owned()),
            )
            .await
            else {
                panic!("неизвестная площадка в пути — 422");
            };
            assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
            assert_eq!(envelope.error.code, "unknown_marketplace");
        }

        #[tokio::test]
        async fn delete_canonicalizes_marketplace_case() {
            // Канонизация литерала пути: регистр не ломает DELETE; БД-касание
            // без живой БД — 503 (после валидации и identity).
            let state = state_with(Arc::new(MockProbe(Vec::new())));
            let Err((status, Json(envelope))) = delete_connection(
                State(state),
                Ok(Query(HashMap::new())),
                Path("Wildberries".to_owned()),
            )
            .await
            else {
                panic!("валидный путь обязан пройти валидацию");
            };
            assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
            assert_eq!(envelope.error.code, "database_unavailable");
        }
    }
}
