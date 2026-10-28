/**
 * Чистая логика шага 0 «ваш бизнес» (Story 1.2) — гейтинг ответов
 * вопросника, вынесенный из компонента для прямого тестирования
 * (ревью Story 1.2: клиентская половина контракта шага 0 обязана иметь
 * регрессионную сеть — как серверная).
 */

import type { TradeModel } from './api'

/** Селлерские модели: вопрос о площадках обязателен, список непуст.
 *  Серверная половина инварианта — 422 marketplaces_required (gateway). */
export function needsMarketplaces(tradeModel: TradeModel | null): boolean {
  return tradeModel === 'marketplace_seller' || tradeModel === 'marketplace_planned'
}

/** Готов ли ответ к отправке: модель выбрана; для селлерских моделей
 *  выбрана хотя бы одна площадка; повторная отправка заблокирована. */
export function canSubmitStep0(
  tradeModel: TradeModel | null,
  marketplaces: readonly string[],
  submitting: boolean,
): boolean {
  if (tradeModel === null || submitting) return false
  return !needsMarketplaces(tradeModel) || marketplaces.length > 0
}
