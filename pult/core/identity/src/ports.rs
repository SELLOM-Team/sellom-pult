//! Порты домена `identity` (AR-2: наружу — только через трейты).
//!
//! Story 1.2 — [`PerimeterStore`]: хранение иммутабельных вердиктов шага 0;
//! закрывается sqlx-стором шлюза (AD-1: композиция вне домена).
//!
//! Story 1.3 (token-vault, AD-16) — три порта мастера подключения:
//! - [`MasterKeyProvider`] + [`TokenCipher`]: KMS-конверт Ф0 = env-ключ
//!   (версия 1); расшифровка в проде-пути Ф0 не вызывается (write-only vault,
//!   потребитель decrypt — адаптеры Epic 2);
//! - [`ConnectionStore`]: сохранение/чтение/удаление подключений по
//!   UNIQUE(organization_id, marketplace) (повторный POST — перезапись);
//! - [`RightsProbe`]: пробы прав по API площадки (реализация — шлюз,
//!   reqwest; в тестах — моки). [`IdentityPort`] остаётся заготовкой до
//!   сторис авторизации/ролей (1.6).

use std::future::Future;
use std::pin::Pin;

use uuid::Uuid;

use crate::perimeter::{PerimeterAnswers, Verdict};
use crate::vault::{ConnectionStatus, Credentials, Marketplace, RightsCheck, Scheme};

/// Порт домена `identity`.
///
/// Story 1.1 сознательно не наполняет контракт: сигнатуры методов определят
/// сторис владельца домена под собственные нужды.
pub trait IdentityPort {}

/// Порт хранения вердиктов периметра (шаг 0, Story 1.2).
///
/// Контракт идемпотентности: вердикт иммутабелен per-user. `save_verdict`
/// при уже сохранённом вердикте НЕ перезаписывает его и возвращает именно
/// сохранённый — гонки двух параллельных сохранений дают один результат
/// (`INSERT … ON CONFLICT (user_id) DO NOTHING` + дочитать существующую
/// строку на стороне адаптера).
pub trait PerimeterStore {
    /// Ошибка хранения; конкретный тип определяет адаптер (sqlx и прочее
    /// в домен не протекают).
    type Error;

    /// Возвращает сохранённый вердикт пользователя, если шаг 0 уже пройден.
    fn load_verdict(
        &self,
        user_id: Uuid,
    ) -> impl Future<Output = Result<Option<Verdict>, Self::Error>> + Send;

    /// Сохраняет вердикт пользователя (идемпотентно: существующий вердикт
    /// выигрывает) и возвращает действующий вердикт.
    ///
    /// `organization_id` нужен вместе с `user_id` составным FK
    /// (organization_id, user_id) → users — вердикт нельзя приписать
    /// пользователю другой организации.
    fn save_verdict(
        &self,
        organization_id: Uuid,
        user_id: Uuid,
        answers: &PerimeterAnswers,
        verdict: Verdict,
    ) -> impl Future<Output = Result<Verdict, Self::Error>> + Send;
}

// --- Story 1.3: token-vault (AD-16) ---

/// Шифротекст токена: пара `ciphertext + nonce` (bytea в БД) и версия ключа.
///
/// `key_version` — будущая ротация master-ключа без перешифрования всех строк
/// (re-encrypt job — Ф1+); в Ф0 всегда 1 (env-ключ).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncryptedPayload {
    /// Шифротекст (AES-256-GCM, включая GCM-тег аутентичности).
    pub ciphertext: Vec<u8>,
    /// Уникальный nonce ЭТОЙ записи (12 байт для GCM).
    pub nonce: Vec<u8>,
    /// Версия ключа, которым зашифровано (KMS-конверт Ф0: 1).
    pub key_version: u32,
}

/// Ключ шифрования с версией (KMS-конверт Ф0 — один env-ключ версии 1).
///
/// `Debug` выведен вручную: байты ключа НИКОГДА не попадают в вывод
/// (любой `{:?}` конфига/состояния печатает только версию и маску) —
/// ревью Story 1.3: derive(Debug) протёк бы ключом в логи.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct MasterKey {
    /// Версия ключа (логируется БЕЗ байтов ключа).
    pub version: u32,
    /// Байты ключа (32 байта для AES-256); в логи не попадают.
    pub bytes: [u8; 32],
}

impl std::fmt::Debug for MasterKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MasterKey")
            .field("version", &self.version)
            .field("bytes", &"***32B-redacted***")
            .finish()
    }
}

/// Порт источника ключей шифрования (KMS-конверт).
///
/// Ф0: единственный ключ из окружения (`PULT_VAULT_MASTER_KEY`, версия 1).
/// Будущий YC KMS-адаптер реализует тот же порт (несколько версий —
/// ротация master-ключа без перешифрования).
pub trait MasterKeyProvider: Send + Sync {
    /// Активный ключ (шифрование новых записей).
    fn active_key(&self) -> MasterKey;

    /// Ключ конкретной версии (расшифровка старых записей после ротации).
    /// `None` — версия неизвестна (запись чужая/повреждена).
    fn key_for_version(&self, version: u32) -> Option<MasterKey>;
}

/// Порт шифрования токенов (AES-256-GCM, AD-16).
///
/// Инварианты реализации:
/// - уникальный nonce на каждую запись (`encrypt` генерирует его сам);
/// - ключ 256 бит из [`MasterKeyProvider`]; `key_version` пишется рядом
///   с шифротекстом для ротации;
/// - `decrypt` — ТОЛЬКО для будущих адаптеров Epic 2 и юнит-тестов:
///   в проде-пути Ф0 расшифровок нет (write-only vault).
pub trait TokenCipher: Send + Sync {
    /// Ошибка шифратора (адаптер-специфична).
    type Error;

    /// Шифрует байты секрета в [`EncryptedPayload`] (nonce уникален на запись).
    fn encrypt(&self, plaintext: &[u8]) -> Result<EncryptedPayload, Self::Error>;

    /// Расшифровывает запись из БД (см. ограничение использования выше).
    fn decrypt(&self, payload: &EncryptedPayload) -> Result<Vec<u8>, Self::Error>;
}

/// Подключение к площадке для записи (автотест прав уже пройден успешно).
///
/// Сохранение — перезапись по UNIQUE(organization_id, marketplace):
/// ротация токена = повторный POST с новым токеном (шифротекст,
/// verified_at и права перезаписываются; дублей площадок нет — Ф0-квота:
/// один кабинет на площадку в портале).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedConnection {
    /// Площадка.
    pub marketplace: Marketplace,
    /// Подтверждённые схемы (fbo/fbs/…, валидны для площадки).
    pub schemes: Vec<Scheme>,
    /// Client-Id Ozon (НЕ секрет, хранится открыто); у WB — `None`.
    pub client_id: Option<String>,
    /// Подтверждённые права (snapshot проб: имя + ok).
    pub rights: Vec<RightsCheck>,
    /// Шифротекст секрета (token/Api-Key) от [`TokenCipher`].
    pub secret: EncryptedPayload,
}

/// Строка подключения из БД (без секретов: шифротекст наружу не отдаётся).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredConnection {
    /// Площадка.
    pub marketplace: Marketplace,
    /// Схемы.
    pub schemes: Vec<Scheme>,
    /// Статус (Ф0-адаптер пишет только `Verified`).
    pub status: ConnectionStatus,
    /// Client-Id Ozon (открыто); у WB — `None`.
    pub client_id: Option<String>,
    /// Snapshot прав на момент последнего успешного автотеста.
    pub rights: Vec<RightsCheck>,
    /// Момент подтверждения прав (unix-секунды; `None` — никогда).
    /// Время в домене не типизируется chrono-зависимостью — форматирование
    /// RFC3339 выполняет шлюз.
    pub verified_at_unix: Option<i64>,
}

/// Порт хранения подключений (save/load/delete).
pub trait ConnectionStore {
    /// Ошибка хранения; конкретный тип определяет адаптер.
    type Error;

    /// Сохраняет подключение (перезапись по (organization_id, marketplace)).
    fn save_connection(
        &self,
        organization_id: Uuid,
        connection: &VerifiedConnection,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;

    /// Все подключения организации (без секретов).
    fn list_connections(
        &self,
        organization_id: Uuid,
    ) -> impl Future<Output = Result<Vec<StoredConnection>, Self::Error>> + Send;

    /// Удаляет подключение (токен отозван). Отсутствие строки — не ошибка
    /// (DELETE идемпотентен).
    fn delete_connection(
        &self,
        organization_id: Uuid,
        marketplace: Marketplace,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;
}

/// Порт проб прав: по площадке и учётным данным — список проверок
/// (по одной на право-категорию), БЕЗ паник и HTTP-ошибок наружу:
/// любая сетевая проблема → [`crate::vault::ProbeResult::Unreachable`].
///
/// Реализация (шлюз) не логирует токены — только метаданные и маски.
pub trait RightsProbe: Send + Sync {
    /// Запускает все пробы площадки; объектно-безопасная сигнатура
    /// (boxed future) — порт подставляется в состояние шлюза как `Arc<dyn>`,
    /// чтобы тесты подменяли пробу моком (ok/403/таймаут).
    fn probe_rights<'a>(
        &'a self,
        marketplace: Marketplace,
        credentials: &'a Credentials,
    ) -> Pin<Box<dyn Future<Output = Vec<crate::vault::ProbeCheck>> + Send + 'a>>;
}
