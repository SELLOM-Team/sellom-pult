/**
 * Клиент API шага 0 (Story 1.2): состояние онбординга, вердикт периметра,
 * метрика доли прошедших.
 *
 * Семантика как в health.ts: только JSON-ответ шлюза считается ответом
 * шлюза (HTML-фоллбек статик-хостинга — не ответ). Ошибки приходят
 * конвертом `{error:{code,message,request_id,details}}` и разбираются в
 * {@link ApiError} с машинным кодом. Идентичность передаётся только
 * placement-параметрами (DOMAIN/USER_ID/PLACEMENT) — без PII, без
 * собственной аутентификации (NFR-6).
 */

import type { B24PlacementContext } from './b24'

/** Состояние шага 0 (ответ GET /v1/onboarding/state). */
export type OnboardingState = 'step0_required' | 'funnel' | 'exit_screen'

/** Торговая модель — 4 литерала контракта (маппинг в enum — на шлюзе). */
export type TradeModel =
  | 'marketplace_seller'
  | 'marketplace_planned'
  | 'own_channels_no_marketplaces'
  | 'services_other'

/** Вердикт периметра. */
export type Verdict = 'seller' | 'non_seller'

/** Метрика шага 0 (GET /v1/metrics/perimeter). */
export interface PerimeterMetrics {
  installs: number
  step0_completed: number
  sellers: number
  step0_share: number
}

/** Ошибка вызова API: машинный код + человекочитаемое сообщение. */
export class ApiError extends Error {
  constructor(
    readonly code: string,
    message: string,
    readonly status?: number,
  ) {
    super(message)
    this.name = 'ApiError'
  }
}

/**
 * Identity-параметры placement-контекста в query-string (DOMAIN/USER_ID/
 * PLACEMENT). Пустой контекст → пустая строка → шлюз применяет
 * документированный dev-фоллбэк (dev-portal/dev-user).
 */
export function identityQuery(ctx: B24PlacementContext): string {
  const params = new URLSearchParams()
  if (ctx.domain) params.set('DOMAIN', ctx.domain)
  if (ctx.userId) params.set('USER_ID', ctx.userId)
  if (ctx.placement) params.set('PLACEMENT', ctx.placement)
  return params.toString()
}

/** Разбор ответа GET /v1/onboarding/state ({state}); чужая форма — ошибка. */
export function parseOnboardingState(body: unknown): OnboardingState {
  const state = (body as { state?: unknown } | null | undefined)?.state
  if (state === 'step0_required' || state === 'funnel' || state === 'exit_screen') {
    return state
  }
  throw new ApiError(
    'invalid_response',
    `шлюз вернул неизвестное состояние онбординга: ${JSON.stringify(state)}`,
  )
}

/** Разбор ответа POST /v1/perimeter/verdict: вердикт и состояние консистентны. */
export function parseVerdictResponse(body: unknown): {
  verdict: Verdict
  state: 'funnel' | 'exit_screen'
} {
  if (typeof body === 'object' && body !== null) {
    const { verdict, state } = body as Record<string, unknown>
    if (verdict === 'seller' && state === 'funnel') return { verdict, state }
    if (verdict === 'non_seller' && state === 'exit_screen') return { verdict, state }
  }
  throw new ApiError('invalid_response', 'шлюз вернул неконсистентный ответ вердикта')
}

/** Разбор error-конверта {error:{code,message}}; не-конверт → null. */
export function parseErrorEnvelope(body: unknown): { code: string; message: string } | null {
  if (typeof body === 'object' && body !== null) {
    const error = (body as Record<string, unknown>).error
    if (typeof error === 'object' && error !== null) {
      const { code, message } = error as Record<string, unknown>
      if (typeof code === 'string' && typeof message === 'string') {
        return { code, message }
      }
    }
  }
  return null
}

function parsePerimeterMetrics(body: unknown): PerimeterMetrics {
  if (typeof body === 'object' && body !== null) {
    const { installs, step0_completed, sellers, step0_share } = body as Record<
      string,
      unknown
    >
    if (
      typeof installs === 'number' &&
      typeof step0_completed === 'number' &&
      typeof sellers === 'number' &&
      typeof step0_share === 'number'
    ) {
      return { installs, step0_completed, sellers, step0_share }
    }
  }
  throw new ApiError('invalid_response', 'шлюз вернул метрику неожиданной формы')
}

/**
 * Выполняет запрос к шлюзу и возвращает разобранное JSON-тело успешного
 * ответа. Любая ошибка — ApiError: network_error (шлюз недоступен),
 * invalid_response (не-JSON/битый JSON/ошибка без конверта) либо код
 * error-конверта шлюза (например, database_unavailable при 503).
 */
async function requestJson(url: string, init?: RequestInit): Promise<unknown> {
  let response: Response
  try {
    response = await fetch(url, init)
  } catch {
    throw new ApiError('network_error', 'шлюз недоступен: сеть или сервер не отвечает')
  }
  const contentType = response.headers.get('content-type') ?? ''
  if (!contentType.toLowerCase().includes('json')) {
    throw new ApiError(
      'invalid_response',
      `шлюз ответил не-JSON (HTTP ${response.status})`,
      response.status,
    )
  }
  let body: unknown
  try {
    body = await response.json()
  } catch {
    throw new ApiError('invalid_response', 'шлюз ответил битым JSON', response.status)
  }
  if (!response.ok) {
    const envelope = parseErrorEnvelope(body)
    if (envelope) {
      throw new ApiError(envelope.code, envelope.message, response.status)
    }
    throw new ApiError(
      'invalid_response',
      `шлюз ответил ошибкой без конверта (HTTP ${response.status})`,
      response.status,
    )
  }
  return body
}

/** GET /v1/onboarding/state — состояние шага 0 для пользователя placement-контекста. */
export async function fetchOnboardingState(
  ctx: B24PlacementContext,
): Promise<OnboardingState> {
  const query = identityQuery(ctx)
  return parseOnboardingState(
    await requestJson(`/v1/onboarding/state${query ? `?${query}` : ''}`),
  )
}

/** POST /v1/perimeter/verdict — ответы шага 0 → иммутабельный вердикт. */
export async function submitVerdict(
  ctx: B24PlacementContext,
  answers: { trade_model: TradeModel; marketplaces: string[] },
): Promise<{ verdict: Verdict; state: 'funnel' | 'exit_screen' }> {
  const query = identityQuery(ctx)
  return parseVerdictResponse(
    await requestJson(`/v1/perimeter/verdict${query ? `?${query}` : ''}`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(answers),
    }),
  )
}

/** GET /v1/metrics/perimeter — доля установок, прошедших шаг 0 (без PII). */
export async function fetchPerimeterMetrics(): Promise<PerimeterMetrics> {
  return parsePerimeterMetrics(await requestJson('/v1/metrics/perimeter'))
}
