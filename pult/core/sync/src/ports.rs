//! Порты домена `sync` (AR-2: наружу — только через трейты).
//!
//! Story 1.4 (первичный синк и первый канбан) — четыре порта вертикали:
//! - [`WbOrdersSource`]: чтение истории заказов WB окнами дат (закрывается
//!   HTTP-клиентом конвейера, conveyor/adapters);
//! - [`OrderEventPublisher`]: публикация канонических событий в RabbitMQ
//!   quorum (закрывается lapin-адаптером конвейера; конверт AD-22);
//! - [`OrderProjection`]: идемпотентная проекция заказа в PG (закрывается
//!   sqlx-сторой conveyor/workers; барьер ревизий + idem UNIQUE, AD-15);
//! - [`SyncCheckpointStore`]: состояние цикла `(organization_id, тип цикла)`
//!   с чекпоинтами (закрывается sqlx-сторой; AD-3).
//!
//! Домен синка НЕ расшифровывает токены: расшифровка — забота адаптера,
//! получившего [`pult_core_identity::ports::TokenCipher`] (write-only vault
//! 1.3 получает первого потребителя decrypt именно здесь, в конвейере).

use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;

use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::orders::{BoardCard, BoardColumn, CanonicalOrder};

/// Окно дат цикла синка (границы включительно; AD-3: единица чекпоинта).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DateWindow {
    /// Начало окна.
    pub from: DateTime<Utc>,
    /// Конец окна.
    pub to: DateTime<Utc>,
}

/// Тип цикла синка (чей-поинт-ключ AD-3 вместе с организацией).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CycleType {
    /// Первичная выгрузка истории заказов (Story 1.4).
    InitialOrders,
}

impl CycleType {
    /// Код цикла — БД CHECK `sync_runs.cycle_type` (миграция 0007).
    pub fn as_str(&self) -> &'static str {
        match self {
            CycleType::InitialOrders => "initial_orders",
        }
    }
}

/// Статус цикла (БД CHECK `sync_runs.status`); маппит шлюз/воркер.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunStatus {
    /// Создан, окна не начаты.
    Pending,
    /// Окна идут.
    Running,
    /// Полный охват окон.
    Succeeded,
    /// Неисправимый сбой (диагностика в `error_hint`).
    Failed,
    /// Сбоя с чекпоинтом — продолжение по retry.
    NeedsRetry,
    /// Цикл заменён (будущие сторис ротации).
    Superseded,
}

impl RunStatus {
    /// Код статуса — БД CHECK `sync_runs.status`.
    pub fn as_str(&self) -> &'static str {
        match self {
            RunStatus::Pending => "pending",
            RunStatus::Running => "running",
            RunStatus::Succeeded => "succeeded",
            RunStatus::Failed => "failed",
            RunStatus::NeedsRetry => "needs_retry",
            RunStatus::Superseded => "superseded",
        }
    }

    /// Обратный [`RunStatus::as_str`] — чтение витрины `sync_runs` сторами
    /// (шлюз и воркеры); `None` для значений вне CHECK-словаря — честная
    /// ошибка «повреждённые данные» вместо молчаливой подмены. Прецедент —
    /// [`crate::orders::BoardColumn::from_code`].
    pub fn from_code(raw: &str) -> Option<Self> {
        match raw {
            "pending" => Some(RunStatus::Pending),
            "running" => Some(RunStatus::Running),
            "succeeded" => Some(RunStatus::Succeeded),
            "failed" => Some(RunStatus::Failed),
            "needs_retry" => Some(RunStatus::NeedsRetry),
            "superseded" => Some(RunStatus::Superseded),
            _ => None,
        }
    }
}

/// Состояние цикла синка — одна строка `sync_runs` (UNIQUE per цикл).
///
/// Чекпоинт AD-3: воркер сохраняет состояние ПОСЛЕ каждого подтверждённого
/// окна; повтор после сбоя продолжается с `windows_done`, а не с нуля.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncRunState {
    /// Организация (партиционный ключ AD-4).
    pub organization_id: Uuid,
    /// Тип цикла.
    pub cycle: CycleType,
    /// Статус.
    pub status: RunStatus,
    /// Последнее подтверждённое окно (граница продолжения).
    pub completed_until: Option<DateTime<Utc>>,
    /// Всего окон в плане.
    pub windows_total: u32,
    /// Подтверждённых окон.
    pub windows_done: u32,
    /// Считанные заказы (сырые строки статистики).
    pub orders_fetched: u64,
    /// Опубликованные события.
    pub events_published: u64,
    /// Человекочитаемая диагностика сбоя (без секретов).
    pub error_hint: Option<String>,
}

/// Сырая строка заказа WB статистики (транспорт до канонизации).
///
/// Заполняется адаптером из ответа `GET /api/v1/supplier/orders`; поля —
/// только то, что фактически даёт площадка (без выдуманных кодов статусов:
/// словарь — [`crate::orders::wb_board_column`]).
#[derive(Debug, Clone, PartialEq)]
pub struct WbOrderRow {
    /// Дата заказа (date).
    pub date: DateTime<Utc>,
    /// Последнее изменение (lastChangeDate) — источник ревизии.
    pub last_change_date: DateTime<Utc>,
    /// Артикул продавца (supplierArticle).
    pub supplier_article: String,
    /// Размер (techSize).
    pub tech_size: String,
    /// Штрихкод (barcode).
    pub barcode: String,
    /// Скидка, % (discountPercent).
    pub discount_percent: i64,
    /// Отмена (isCancel).
    pub is_cancel: bool,
    /// Номер incoming-поставки (incomeID): >0 → FBO-эвристика схемы.
    pub income_id: i64,
    /// Склад (warehouseName).
    pub warehouse_name: String,
    /// Номер заказа (orderId).
    pub order_id: i64,
    /// Артикул площадки (nmId).
    pub nm_id: i64,
    /// Предмет (subject).
    pub subject: String,
    /// Бренд (brand).
    pub brand: String,
    /// Код статуса статистики (status).
    pub status: i64,
    /// Завершён (finished).
    pub finished: bool,
    /// Цена до скидки, копейки (price).
    pub price: i64,
    /// Итоговая цена позиции, копейки (totalPrice).
    pub total_price: i64,
}

/// Класс сбоя источника заказов — вход в решение о судьбе цикла (спека 1.4:
/// «401 ≠ ретрай»). Классифицирует АДАПТЕР (ему известны HTTP-коды);
/// домен знает только исходы.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceFailure {
    /// Токен отозван/недействителен (401/403): ретрай бессмыслен — цикл
    /// завершается `failed` с человекочитаемой диагностикой.
    AccessRevoked,
    /// Временный сбой (лимит не отпустил, сеть, 5xx): цикл — `needs_retry`,
    /// продолжение с чекпоинта (AD-3).
    Transient,
    /// Неисправимый без человека (площадка сменила схему ответа, потолок
    /// тела): цикл — `failed`, событие для ревью.
    Fatal,
}

/// Порт: чтение истории заказов WB окнами (закрывается HTTP-адаптером).
pub trait WbOrdersSource {
    /// Ошибка источника; конкретика (HTTP/токен/лимит) — адаптера.
    type Error;

    /// Классифицирует сбой источника: решение «failed vs needs_retry» —
    /// политика домена, классификация HTTP-деталей — знание адаптера.
    fn classify_failure(error: &Self::Error) -> SourceFailure;

    /// Заказы одного окна дат (сырые строки статистики WB).
    ///
    /// Контракт вызова: адаптер сам держит расшифрованный токен организации
    /// (получен через `TokenCipher`), соблюдает лимиты площадки (адаптивный
    /// backoff на 429) и потолок размера ответа. Пустой `Vec` — окно без
    /// заказов, это нормальный результат, а не ошибка.
    fn fetch_window(
        &self,
        window: DateWindow,
    ) -> impl Future<Output = Result<Vec<WbOrderRow>, Self::Error>> + Send;
}

/// Конверт события конвейера (AD-22, канонический внутренний контракт).
#[derive(Debug, Clone, PartialEq)]
pub struct OrderEvent {
    /// Идентификатор события (UUIDv7 — сортируем по времени).
    pub event_id: Uuid,
    /// Тип события — past-tense («заказ записан в канал»).
    pub event_type: &'static str,
    /// Организация (маршрутизация AD-4 + партиции).
    pub organization_id: Uuid,
    /// Момент события.
    pub occurred_at: DateTime<Utc>,
    /// Версия канонической схемы (инкремент при смене словаря статусов).
    pub schema_version: u16,
    /// Канонический заказ (payload).
    pub order: CanonicalOrder,
}

impl OrderEvent {
    /// Тип события конвейера заказов.
    pub const EVENT_TYPE: &'static str = "order.upserted";

    /// Текущая версия схемы событий заказов (первая — 1; инкремент при
    /// заполнении `WB_STATUS_DICTIONARY` или смене полей заказа).
    pub const SCHEMA_VERSION: u16 = 1;

    /// Собирает событие из заказа (идемпотентен по содержимому — event_id
    /// уникален на публикацию, дубль доставки гасится idem-ключом AD-15).
    pub fn new(order: CanonicalOrder, occurred_at: DateTime<Utc>) -> Self {
        Self {
            event_id: Uuid::now_v7(),
            event_type: Self::EVENT_TYPE,
            organization_id: order.organization_id,
            occurred_at,
            schema_version: Self::SCHEMA_VERSION,
            order,
        }
    }
}

/// Порт: публикация событий заказов (закрывается lapin-адаптером RabbitMQ).
pub trait OrderEventPublisher {
    /// Ошибка публикации (брокер/сериализация).
    type Error;

    /// Публикует событие в quorum-очередь (routing key по организации, AD-4).
    /// Доставка at-least-once: потребитель обязан быть идемпотентным (AD-15).
    fn publish(&self, event: &OrderEvent) -> impl Future<Output = Result<(), Self::Error>> + Send;

    /// Барьер долговечности: все ранее опубликованные события подтверждены
    /// брокером (publisher confirms, quorum-репликация). Воркер вызывает
    /// ДО чекпоинта окна — чекпоинт означает «события окна дошли» (граница
    /// at-least-once). Для фейков/заглушек — пустая операция.
    fn flush(&self) -> impl Future<Output = Result<(), Self::Error>> + Send;
}

/// Исход проекции заказа (для метрик и инварианта идемпотентности).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpsertOutcome {
    /// Заказ вставлен (первое событие заказа).
    Inserted,
    /// Заказ обновлён (более новая ревизия).
    Updated,
    /// NO-OP: дубль события (та же/старая ревизия) — at-least-once погашен.
    NoOp,
}

/// Порт: идемпотентная проекция заказа в PG (закрывается sqlx-сторой
/// conveyor/workers; UNIQUE-барьеры миграции 0007).
pub trait OrderProjection {
    /// Ошибка проекции (sqlx и прочее — адаптера).
    type Error;

    /// Вставляет/обновляет заказ ПОЗИЦИЯМИ атомарно (одна транзакция):
    /// - точный дубль (idem-ключ совпал) → [`UpsertOutcome::NoOp`];
    /// - новая ревизия (revision > хранимой) → [`UpsertOutcome::Updated`];
    /// - старая ревизия → [`UpsertOutcome::NoOp`] (барьер откатов).
    fn upsert(
        &self,
        order: &CanonicalOrder,
    ) -> impl Future<Output = Result<UpsertOutcome, Self::Error>> + Send;
}

/// Порт: состояние цикла синка с чекпоинтами (AD-3; sqlx-стора workers).
pub trait SyncCheckpointStore {
    /// Ошибка хранения.
    type Error;

    /// Загружает состояние цикла организации (None — цикл ещё не начинался).
    fn load(
        &self,
        organization_id: Uuid,
        cycle: CycleType,
    ) -> impl Future<Output = Result<Option<SyncRunState>, Self::Error>> + Send;

    /// Сохраняет состояние ПОСЛЕ подтверждённого окна (чекпоинт-инвариант):
    /// запись атомарна; воркер после сбоя продолжает с `windows_done`.
    fn save(&self, state: &SyncRunState) -> impl Future<Output = Result<(), Self::Error>> + Send;
}

// ── витрина синка для шлюза (Story 1.4: /v1/sync/*, /v1/board/*) ─────────────
//
// Порты чтения прогресса/доски и команда ретрая объявлены объектно-совместимыми
// (Pin<Box<dyn Future>> — прецедент `RightsProbe` ядра identity): шлюз держит
// их за `Arc<dyn …>` и подменяет in-memory фейками в тестах хендлеров, а
// sqlx-адаптеры (PG) живут в самом шлюзе — у каждого композиционного корня
// свои адаптеры (AD-1: шлюз не импортирует сторы воркеров).

/// Прогресс цикла для витрины `/v1/sync/progress`: состояние домена + момент
/// первого старта (наблюдаемая скорость ETA: `started_at` COALESCE-ится стором
/// с первого перехода в running — паузы needs_retry в оценку активного цикла
/// не попадают, неактивный цикл ETA не обещает).
#[derive(Debug, Clone, PartialEq)]
pub struct SyncProgress {
    /// Состояние цикла (строка `sync_runs`).
    pub state: SyncRunState,
    /// Первый старт цикла (NULL — окна не начинались).
    pub started_at: Option<DateTime<Utc>>,
}

/// Исход команды повтора цикла (POST /v1/sync/retry).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryOutcome {
    /// `failed → pending`: цикл продолжится с чекпоинта планировщиком. Сброс —
    /// единственный способ оживить терминальный `failed`: оркестратор
    /// идемпотентно возвращает терминальные строки как есть, без запросов.
    Reset,
    /// Без изменений: текущий статус подхвата сам (pending/running/needs_retry
    /// — планировщик) либо завершён (succeeded/superseded — ретраить нечего).
    Unchanged(RunStatus),
}

/// Боксированная футура витрины (объектная совместимость портов чтения —
/// clippy type_complexity: сигнатура скрыта за осмысленным именем).
pub type SyncReadFuture<'a, T, E> = Pin<Box<dyn Future<Output = Result<T, E>> + Send + 'a>>;

/// Порт: чтение прогресса цикла (шлюз; закрывается sqlx-стором шлюза).
pub trait SyncProgressReader: Send + Sync {
    /// Ошибка чтения.
    type Error;

    /// Прогресс цикла организации (None — цикл ещё не начинался).
    fn load_progress<'a>(
        &'a self,
        organization_id: Uuid,
        cycle: CycleType,
    ) -> SyncReadFuture<'a, Option<SyncProgress>, Self::Error>;
}

/// Порт: команда повторения цикла после исправимого сбоя (шлюз).
pub trait SyncRetryMutator: Send + Sync {
    /// Ошибка мутации.
    type Error;

    /// Идемпотентный ретрай: сбрасывает ТОЛЬКО `failed` (адаптер атомарно
    /// UPDATE … WHERE status='failed'); прочие статусы — [`RetryOutcome::Unchanged`];
    /// `Ok(None)` — строки цикла нет (синк не стартовал — нечего ретраить).
    fn request_retry<'a>(
        &'a self,
        organization_id: Uuid,
        cycle: CycleType,
    ) -> SyncReadFuture<'a, Option<RetryOutcome>, Self::Error>;
}

/// Снимок доски канбана (витрина `/v1/board/orders`): карточки по колонкам
/// (лимит на колонку, свежие первыми — сортировка стора) + счётчики ВСЕХ строк
/// колонок (показаны первые N, всего — `totals`; честность «показано из»).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct BoardSnapshot {
    /// Карточки на колонку (пустая колонка может отсутствовать ключом).
    pub columns: BTreeMap<BoardColumn, Vec<BoardCard>>,
    /// Счётчики всех строк колонок (GROUP BY стора; отсутствует = 0).
    pub totals: BTreeMap<BoardColumn, u64>,
}

impl BoardSnapshot {
    /// Доска пуста — ни одного заказа в проекции (шлюз решает honest-ответ
    /// по состоянию цикла: пусто+`succeeded` — честная заглушка «заказов
    /// пока нет», пусто+идёт — «данные ещё синкаются», спека 1.4).
    pub fn is_empty(&self) -> bool {
        self.totals.values().all(|count| *count == 0)
    }
}

/// Порт: чтение доски канбана организации (шлюз; sqlx-стор шлюза).
pub trait OrdersBoardReader: Send + Sync {
    /// Ошибка чтения.
    type Error;

    /// Снимок доски: до `limit_per_column` карточек на колонку + счётчики.
    fn board_orders<'a>(
        &'a self,
        organization_id: Uuid,
        limit_per_column: u32,
    ) -> Pin<Box<dyn Future<Output = Result<BoardSnapshot, Self::Error>> + Send + 'a>>;
}
