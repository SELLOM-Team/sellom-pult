/**
 * Чистая мапа состояния шага 0 → экран SPA (Story 1.2).
 *
 * Роутинг App построен на этой функции: step0_required → вопросник,
 * funnel → существующий экран «Начните подключение», exit_screen →
 * исходящий экран не-селлера, ошибки/офлайн → честная заглушка.
 * Функция чистая — покрыта тестами напрямую (appState.test.ts).
 */

import { ApiError, type OnboardingState } from './api'

/** Результат опроса шлюза на входе роутинга App. */
export type ScreenState =
  | { kind: 'loading' }
  | { kind: 'resolved'; state: OnboardingState }
  | { kind: 'offline' }
  | { kind: 'error'; code: string; message: string }

/** Экран, который рисует App. */
export type Screen = 'skeleton' | 'questionnaire' | 'funnel' | 'exit' | 'unavailable'

/**
 * Классификация ошибки опроса шлюза (ревью Story 1.2: вынесена из App
 * для прямого тестирования — инверсия веток больше не проходит мимо тестов).
 *
 * Ошибочный ответ шлюза-конвертом (например 503 database_unavailable) —
 * шлюз ЖИВ и честно рассказал о проблеме → kind: 'error'. Сеть/не-JSON —
 * шлюза нет в этом окружении → kind: 'offline'.
 */
export function screenStateFromError(err: unknown): ScreenState {
  if (
    err instanceof ApiError &&
    err.code !== 'network_error' &&
    err.code !== 'invalid_response'
  ) {
    return { kind: 'error', code: err.code, message: err.message }
  }
  return { kind: 'offline' }
}

export function screenForState(screenState: ScreenState): Screen {
  switch (screenState.kind) {
    case 'loading':
      return 'skeleton'
    case 'resolved':
      switch (screenState.state) {
        case 'step0_required':
          return 'questionnaire'
        case 'funnel':
          return 'funnel'
        case 'exit_screen':
          return 'exit'
      }
    // Офлайн (шлюз не отвечает / не-JSON) и ошибки шлюза (503-конверт и
    // прочие) — одна честная заглушка с разной диагностикой.
    case 'offline':
      return 'unavailable'
    case 'error':
      return 'unavailable'
  }
}
