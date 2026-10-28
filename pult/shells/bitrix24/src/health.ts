/**
 * Чистая мапа ответа /health шлюза в фазу статуса (Story 1.1).
 *
 * Честная семантика: только JSON от самого шлюза считается ответом шлюза.
 * HTML-фоллбек статик-хостинга (index.html с кодом 200 на любой путь) —
 * НЕ ответ шлюза: без проверки Content-Type SPA показывало бы ложный
 * «шлюз жив». Выделено в чистую функцию — покрыта тестами напрямую.
 */

import type { ConnectionPhase } from './ConnectionStatus'

/** Терминальные фазы (после ответа/ошибки пробы); `checking` — до ответа. */
export type HealthPhase = Exclude<ConnectionPhase, 'checking'>

/**
 * @param status HTTP-код ответа
 * @param contentType значение заголовка Content-Type (или null)
 * @returns 200+JSON → gateway-up; 503+JSON → gateway-degraded; всё прочее → offline
 */
export function phaseFromHealthResponse(
  status: number,
  contentType: string | null,
): HealthPhase {
  const isJson = (contentType ?? '').toLowerCase().includes('json')
  if (isJson && status === 200) return 'gateway-up'
  if (isJson && status === 503) return 'gateway-degraded'
  return 'offline'
}
