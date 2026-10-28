/**
 * Чистая логика мастера подключения площадок (Story 1.3, шаги 1–3) —
 * гейтинг и валидация форм, вынесенные из компонента для прямого
 * тестирования (паттерн step0.ts Story 1.2).
 *
 * Серверная половина правил — в core/identity::vault + gateway /v1/connections:
 * клиентская валидация лишь предотвращает заведомые 422, контракт — за шлюзом.
 */

import type { MarketplaceId } from '../../api'

/** Схема продажи для чекбоксов шага 2 (матрица сервера: WB fbo/fbs/realfbs, Ozon fbo/fbs/rfbs). */
export const SCHEMES: Record<MarketplaceId, { id: string; label: string }[]> = {
  wildberries: [
    { id: 'fbo', label: 'FBO — со склада площадки' },
    { id: 'fbs', label: 'FBS — со склада селлера' },
    { id: 'realfbs', label: 'realFBS' },
  ],
  ozon: [
    { id: 'fbo', label: 'FBO — со склада площадки' },
    { id: 'fbs', label: 'FBS — со склада селлера' },
    { id: 'rfbs', label: 'rFBS' },
  ],
}

/** Человекочитаемое имя площадки для UI мастера. */
export const MARKETPLACE_LABELS: Record<MarketplaceId, string> = {
  wildberries: 'Wildberries',
  ozon: 'Ozon',
}

/** Шаг мастера: 1 — выбор площадки, 2 — токен и схемы, 3 — итог автотеста. */
export type WizardStep = 1 | 2 | 3

/**
 * Активный шаг мастера: итог автотеста открывает шаг 3; выбранная площадка —
 * шаг 2; иначе шаг 1. Чистая функция — покрыта тестами напрямую.
 */
export function wizardStep(marketplace: MarketplaceId | null, outcome: unknown): WizardStep {
  if (outcome !== null && outcome !== undefined) return 3
  if (marketplace !== null) return 2
  return 1
}

/** Ошибки формы шага 2 (человекочитаемые; пустая строка — нет ошибки). */
export interface ConnectFormErrors {
  token: string
  clientId: string
  schemes: string
}

/**
 * Валидация формы шага 2 ДО отправки (клиентская половина контракта 422):
 * токен непустой и без переносов; для Ozon обязателен Client-Id; хотя бы
 * одна схема, и все — из списка площадки.
 */
export function validateConnectForm(
  marketplace: MarketplaceId,
  token: string,
  clientId: string,
  schemes: readonly string[],
): ConnectFormErrors {
  const trimmedToken = token.trim()
  let tokenError = ''
  if (!trimmedToken) {
    tokenError = 'Вставьте API-токен площадки'
  } else if (/[\n\r\t]/.test(trimmedToken)) {
    tokenError = 'Токен не должен содержать переносов строк'
  }

  let clientIdError = ''
  if (marketplace === 'ozon' && !clientId.trim()) {
    clientIdError = 'Для Ozon нужен Client-Id кабинета вместе с Api-Key'
  }

  let schemesError = ''
  const allowed = SCHEMES[marketplace]
  if (schemes.length === 0) {
    schemesError = 'Выберите хотя бы одну схему продаж'
  } else if (schemes.some((s) => !allowed.some((a) => a.id === s))) {
    schemesError = 'Схема не существует у выбранной площадки'
  }

  return { token: tokenError, clientId: clientIdError, schemes: schemesError }
}

/** Готова ли форма к отправке (нет ни одной ошибки). */
export function canSubmitConnect(errors: ConnectFormErrors, submitting: boolean): boolean {
  return !submitting && errors.token === '' && errors.clientId === '' && errors.schemes === ''
}

/**
 * Подсказка о зоне ответственности Владельца: токены дальше видит и меняет
 * только роль Владелец (FR-29); серверный контроль ролей включится в шаге 6
 * (Story 1.6) — честная пометка вместо бутафорского замка.
 */
export const OWNER_ZONE_NOTE =
  'Токены видит и меняет только роль «Владелец» (контроль ролей включится в шаге 6 онбординга).'
