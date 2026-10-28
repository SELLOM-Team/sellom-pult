//! Порты домена `identity` (AR-2: наружу — только через трейты).
//!
//! Story 1.2 — первый контракт: [`PerimeterStore`] — хранение иммутабельных
//! вердиктов шага 0; закрывается sqlx-стором шлюза (AD-1: композиция вне
//! домена). [`IdentityPort`] остаётся заготовкой до сторис авторизации/ролей
//! (1.3+, 1.6).

use std::future::Future;

use uuid::Uuid;

use crate::perimeter::{PerimeterAnswers, Verdict};

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
