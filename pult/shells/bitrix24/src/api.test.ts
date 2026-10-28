import { afterEach, describe, expect, it, vi } from 'vitest'
import type { B24PlacementContext } from './b24'
import {
  ApiError,
  fetchOnboardingState,
  fetchPerimeterMetrics,
  identityQuery,
  parseErrorEnvelope,
  parseOnboardingState,
  parseVerdictResponse,
  submitVerdict,
} from './api'

// Контекст placement Б24 с identity-полями (DOMAIN/USER_ID — не PII).
const ctx: B24PlacementContext = {
  embedded: true,
  placement: 'DEFAULT',
  domain: 'shop.example',
  userId: '42',
}

const devCtx: B24PlacementContext = { embedded: false }

function jsonResponse(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { 'Content-Type': 'application/json' },
  })
}

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('identityQuery — placement-параметры без PII', () => {
  it('полный контекст → DOMAIN/USER_ID/PLACEMENT', () => {
    expect(identityQuery(ctx)).toBe('DOMAIN=shop.example&USER_ID=42&PLACEMENT=DEFAULT')
  })

  it('dev-контекст без placement-полей → пустая строка (dev-фоллбэк шлюза)', () => {
    expect(identityQuery(devCtx)).toBe('')
  })
})

describe('parseOnboardingState — семантика ответа state', () => {
  it('три состояния распознаются', () => {
    expect(parseOnboardingState({ state: 'step0_required' })).toBe('step0_required')
    expect(parseOnboardingState({ state: 'funnel' })).toBe('funnel')
    expect(parseOnboardingState({ state: 'exit_screen' })).toBe('exit_screen')
  })

  it('неизвестное состояние → invalid_response', () => {
    expect(() => parseOnboardingState({ state: 'welcome' })).toThrowError(ApiError)
    expect(() => parseOnboardingState({ state: 'welcome' })).toThrowError(
      /неизвестное состояние/,
    )
  })

  it('не-объект или отсутствие поля → invalid_response', () => {
    expect(() => parseOnboardingState(null)).toThrowError(ApiError)
    expect(() => parseOnboardingState('funnel')).toThrowError(ApiError)
    expect(() => parseOnboardingState({})).toThrowError(ApiError)
  })
})

describe('parseVerdictResponse — вердикт и состояние консистентны', () => {
  it('seller ↔ funnel, non_seller ↔ exit_screen', () => {
    expect(parseVerdictResponse({ verdict: 'seller', state: 'funnel' })).toEqual({
      verdict: 'seller',
      state: 'funnel',
    })
    expect(parseVerdictResponse({ verdict: 'non_seller', state: 'exit_screen' })).toEqual({
      verdict: 'non_seller',
      state: 'exit_screen',
    })
  })

  it('неконсистентная пара → invalid_response', () => {
    expect(() => parseVerdictResponse({ verdict: 'seller', state: 'exit_screen' })).toThrowError(
      ApiError,
    )
    expect(() => parseVerdictResponse({ verdict: 'wholesale' })).toThrowError(ApiError)
    expect(() => parseVerdictResponse(null)).toThrowError(ApiError)
  })
})

describe('parseErrorEnvelope — конверт {error:{code,message}}', () => {
  it('валидный конверт разбирается', () => {
    expect(
      parseErrorEnvelope({
        error: { code: 'database_unavailable', message: 'БД недоступна', request_id: null, details: null },
      }),
    ).toEqual({ code: 'database_unavailable', message: 'БД недоступна' })
  })

  it('не-конверт → null (не выбрасывает)', () => {
    expect(parseErrorEnvelope(null)).toBeNull()
    expect(parseErrorEnvelope({ error: 'flat' })).toBeNull()
    expect(parseErrorEnvelope({ error: { code: 42, message: 'x' } })).toBeNull()
    expect(parseErrorEnvelope({ nope: true })).toBeNull()
  })
})

describe('fetchOnboardingState', () => {
  it('200 + JSON {state} → состояние', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => jsonResponse({ state: 'step0_required' })))
    await expect(fetchOnboardingState(ctx)).resolves.toBe('step0_required')
  })

  it('identity-query подставляется из контекста', async () => {
    const fetchMock = vi.fn(async (_url?: string | URL | Request, _init?: RequestInit) =>
      jsonResponse({ state: 'funnel' }),
    )
    vi.stubGlobal('fetch', fetchMock)
    await expect(fetchOnboardingState(ctx)).resolves.toBe('funnel')
    expect(String(fetchMock.mock.calls[0]?.[0])).toBe(
      '/v1/onboarding/state?DOMAIN=shop.example&USER_ID=42&PLACEMENT=DEFAULT',
    )
  })

  it('dev-контекст → запрос без query (dev-фоллбэк шлюза)', async () => {
    const fetchMock = vi.fn(async (_url?: string | URL | Request, _init?: RequestInit) =>
      jsonResponse({ state: 'step0_required' }),
    )
    vi.stubGlobal('fetch', fetchMock)
    await expect(fetchOnboardingState(devCtx)).resolves.toBe('step0_required')
    expect(String(fetchMock.mock.calls[0]?.[0])).toBe('/v1/onboarding/state')
  })

  it('503 + error-конверт → ApiError с кодом шлюза', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () =>
        jsonResponse(
          {
            error: {
              code: 'database_unavailable',
              message: 'База данных недоступна — повторите попытку позже',
              request_id: null,
              details: null,
            },
          },
          503,
        ),
      ),
    )
    const err = await fetchOnboardingState(ctx).catch((e: unknown) => e)
    expect(err).toBeInstanceOf(ApiError)
    expect((err as ApiError).code).toBe('database_unavailable')
    expect((err as ApiError).status).toBe(503)
    expect((err as ApiError).message).toMatch(/База данных недоступна/)
  })

  it('200 + HTML (фоллбек статик-хостинга) → invalid_response, не состояние', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(
        async () =>
          new Response('<html>fallback</html>', {
            status: 200,
            headers: { 'Content-Type': 'text/html' },
          }),
      ),
    )
    const err = await fetchOnboardingState(ctx).catch((e: unknown) => e)
    expect(err).toBeInstanceOf(ApiError)
    expect((err as ApiError).code).toBe('invalid_response')
  })

  it('сеть отказала → network_error', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => {
        throw new TypeError('fetch failed')
      }),
    )
    const err = await fetchOnboardingState(ctx).catch((e: unknown) => e)
    expect(err).toBeInstanceOf(ApiError)
    expect((err as ApiError).code).toBe('network_error')
  })
})

describe('submitVerdict', () => {
  it('POST с identity-query, JSON-телом и разбором ответа', async () => {
    const fetchMock = vi.fn(
      async (_url?: string | URL | Request, _init?: RequestInit) =>
        jsonResponse({ verdict: 'seller', state: 'funnel' }),
    )
    vi.stubGlobal('fetch', fetchMock)

    const result = await submitVerdict(ctx, {
      trade_model: 'marketplace_seller',
      marketplaces: ['wildberries', 'ozon'],
    })

    expect(result).toEqual({ verdict: 'seller', state: 'funnel' })
    const [url, init] = fetchMock.mock.calls[0] as unknown as [string, RequestInit]
    expect(url).toBe('/v1/perimeter/verdict?DOMAIN=shop.example&USER_ID=42&PLACEMENT=DEFAULT')
    expect(init.method).toBe('POST')
    expect(new Headers(init.headers).get('Content-Type')).toBe('application/json')
    expect(JSON.parse(String(init.body))).toEqual({
      trade_model: 'marketplace_seller',
      marketplaces: ['wildberries', 'ozon'],
    })
  })

  it('не-селлер → exit_screen', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => jsonResponse({ verdict: 'non_seller', state: 'exit_screen' })),
    )
    await expect(
      submitVerdict(devCtx, {
        trade_model: 'services_other',
        marketplaces: [],
      }),
    ).resolves.toEqual({ verdict: 'non_seller', state: 'exit_screen' })
  })

  it('422 + конверт валидации → ApiError с человекочитаемым кодом', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () =>
        jsonResponse(
          {
            error: {
              code: 'marketplaces_required',
              message: 'для торговли на маркетплейсах укажите хотя бы одну площадку',
              request_id: null,
              details: null,
            },
          },
          422,
        ),
      ),
    )
    const err = await submitVerdict(ctx, {
      trade_model: 'marketplace_seller',
      marketplaces: [],
    }).catch((e: unknown) => e)
    expect(err).toBeInstanceOf(ApiError)
    expect((err as ApiError).code).toBe('marketplaces_required')
    expect((err as ApiError).status).toBe(422)
  })
})

describe('fetchPerimeterMetrics', () => {
  it('200 + четыре счётчика → метрика', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () =>
        jsonResponse({ installs: 3, step0_completed: 2, sellers: 1, step0_share: 0.6667 }),
      ),
    )
    await expect(fetchPerimeterMetrics()).resolves.toEqual({
      installs: 3,
      step0_completed: 2,
      sellers: 1,
      step0_share: 0.6667,
    })
  })

  it('битая форма → invalid_response', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => jsonResponse({ installs: 'three' })))
    const err = await fetchPerimeterMetrics().catch((e: unknown) => e)
    expect(err).toBeInstanceOf(ApiError)
    expect((err as ApiError).code).toBe('invalid_response')
  })
})
