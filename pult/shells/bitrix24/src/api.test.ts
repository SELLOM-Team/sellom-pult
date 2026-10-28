import { afterEach, describe, expect, it, vi } from 'vitest'
import type { B24PlacementContext } from './b24'
import {
  ApiError,
  CONNECT_TIMEOUT_MS,
  connectMarketplace,
  fetchConnections,
  deleteConnection,
  fetchOnboardingState,
  fetchPerimeterMetrics,
  identityQuery,
  parseConnections,
  parseConnectOutcome,
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

  it('бюджет 8с: шлюз принял соединение и молчит → network_error (AbortController)', async () => {
    // Регрессия заголовочного поведения (ревью 1.2: зависший шлюз — офлайн,
    // не вечный скелетон): fetch никогда не отвечает сам — abort по таймеру
    // обязан отвергнуть запрос с network_error.
    vi.useFakeTimers()
    try {
      vi.stubGlobal(
        'fetch',
        vi.fn(
          (_url: string | URL | Request, init?: RequestInit) =>
            new Promise<never>((_resolve, reject) => {
              init?.signal?.addEventListener('abort', () =>
                reject(new DOMException('The operation was aborted', 'AbortError')),
              )
            }),
        ),
      )
      const pending = fetchOnboardingState(ctx)
      const assertion = expect(pending).rejects.toMatchObject({ code: 'network_error' })
      await vi.advanceTimersByTimeAsync(8000)
      await assertion
    } finally {
      vi.useRealTimers()
    }
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

describe('parseConnections — список подключений без секретов', () => {
  it('валидный список разбирается', () => {
    const rows = parseConnections({
      connections: [
        {
          marketplace: 'wildberries',
          schemes: ['fbo', 'fbs'],
          status: 'verified',
          verified_at: '2026-10-29T00:00:00+00:00',
          client_id: null,
          rights: [{ name: 'Статистика', ok: true }],
        },
        {
          marketplace: 'ozon',
          schemes: ['fbo'],
          status: 'verified',
          verified_at: null,
          client_id: '42',
          rights: [],
        },
      ],
    })
    expect(rows).toHaveLength(2)
    expect(rows[0]?.marketplace).toBe('wildberries')
    expect(rows[1]?.client_id).toBe('42')
  })

  it('пустой список валиден (портал без подключений)', () => {
    expect(parseConnections({ connections: [] })).toEqual([])
  })

  it('чужая форма — invalid_response', () => {
    expect(() => parseConnections(null)).toThrowError(ApiError)
    expect(() => parseConnections({ nope: true })).toThrowError(ApiError)
    expect(() =>
      parseConnections({ connections: [{ marketplace: 'aliexpress', status: 'verified' }] }),
    ).toThrowError(ApiError)
  })
})

describe('parseConnectOutcome — итог автотеста прав', () => {
  it('verified → права', () => {
    const outcome = parseConnectOutcome({
      status: 'verified',
      rights: [{ name: 'Статистика', ok: true }],
    })
    expect(outcome).toEqual({ status: 'verified', rights: [{ name: 'Статистика', ok: true }] })
  })

  it('rejected/probe_unavailable → диагностика', () => {
    const rejected = parseConnectOutcome({
      status: 'rejected',
      checks: [{ name: 'Цены и скидки', ok: false, hint: 'пересоздайте токен' }],
    })
    expect(rejected.status).toBe('rejected')
    const unavailable = parseConnectOutcome({ status: 'probe_unavailable', checks: [] })
    expect(unavailable.status).toBe('probe_unavailable')
  })

  it('чужая форма — invalid_response', () => {
    expect(() => parseConnectOutcome({ status: 'секрет' })).toThrowError(ApiError)
    expect(() => parseConnectOutcome(null)).toThrowError(ApiError)
  })
})

describe('connectMarketplace / deleteConnection — контракт 1.3', () => {
  it('POST уходит с identity-query и телом без изменений формы', async () => {
    const fetchMock = vi.fn(async () =>
      jsonResponse({ status: 'verified', rights: [{ name: 'Статистика', ok: true }] }),
    )
    vi.stubGlobal('fetch', fetchMock)
    const outcome = await connectMarketplace(ctx, {
      marketplace: 'wildberries',
      schemes: ['fbo', 'fbs'],
      token: 'wb-secret',
    })
    expect(outcome.status).toBe('verified')
    const [url, init] = fetchMock.mock.calls[0] as unknown as [string, RequestInit]
    expect(String(url)).toBe('/v1/connections?DOMAIN=shop.example&USER_ID=42&PLACEMENT=DEFAULT')
    expect(init.method).toBe('POST')
    expect(JSON.parse(String(init.body))).toEqual({
      marketplace: 'wildberries',
      schemes: ['fbo', 'fbs'],
      token: 'wb-secret',
    })
  })

  it('DELETE без тела; секрет не возвращается ответом', async () => {
    const fetchMock = vi.fn(async () => jsonResponse({ status: 'deleted' }))
    vi.stubGlobal('fetch', fetchMock)
    await expect(deleteConnection(devCtx, 'ozon')).resolves.toBeUndefined()
    const [url, init] = fetchMock.mock.calls[0] as unknown as [string, RequestInit]
    expect(String(url)).toBe('/v1/connections/ozon')
    expect(init.method).toBe('DELETE')
  })
})

describe('fetchConnections — единственный GET-клиент 1.3 обязан иметь URL-тест (ревью 1.3)', () => {
  it('GET с identity-query', async () => {
    const fetchMock = vi.fn(
      async (_url?: string | URL | Request, _init?: RequestInit) =>
        jsonResponse({
          connections: [
          {
            marketplace: 'wildberries',
            schemes: ['fbo'],
            status: 'verified',
            verified_at: '2026-10-29T00:00:00+00:00',
            client_id: null,
            rights: [],
          },
          ],
        }),
    )
    vi.stubGlobal('fetch', fetchMock)
    await expect(fetchConnections(ctx)).resolves.toHaveLength(1)
    expect(String(fetchMock.mock.calls[0]?.[0])).toBe(
      '/v1/connections?DOMAIN=shop.example&USER_ID=42&PLACEMENT=DEFAULT',
    )
  })

  it('dev-контекст → запрос без query', async () => {
    const fetchMock = vi.fn(
      async (_url?: string | URL | Request, _init?: RequestInit) =>
        jsonResponse({ connections: [] }),
    )
    vi.stubGlobal('fetch', fetchMock)
    await expect(fetchConnections(devCtx)).resolves.toEqual([])
    expect(String(fetchMock.mock.calls[0]?.[0])).toBe('/v1/connections')
  })
})

describe('бюджет connectMarketplace покрывает худший случай проб (ревью 1.3)', () => {
  it('CONNECT_TIMEOUT_MS ≥ 2 × PROBE_TIMEOUT(10с) — честный probe_unavailable доезжает до шага 3', () => {
    // Серверные пробы: WB = 2 × 10с последовательно (PROBE_TIMEOUT, gateway);
    // клиентский бюджет обязан быть больше, иначе POST оборвётся
    // network_error'ом ровно на том пути, ради которого есть probe_unavailable.
    expect(CONNECT_TIMEOUT_MS).toBeGreaterThanOrEqual(20000)
  })

  it('успешный POST на 12-й секунде проходит в 30с-бюджете (probe_unavailable доезжает)', async () => {
    vi.useFakeTimers()
    try {
      // Медленный, но успешный ответ (12с > общего 8с, < 30с-бюджета POST):
      // честный итог автотеста обязан доехать, а не оборваться network_error.
      vi.stubGlobal(
        'fetch',
        vi.fn(
          () =>
            new Promise<Response>((resolve) => {
              setTimeout(
                () =>
                  resolve(
                    new Response(JSON.stringify({ status: 'verified', rights: [] }), {
                      status: 200,
                      headers: { 'Content-Type': 'application/json' },
                    }),
                  ),
                12000,
              )
            }),
        ),
      )
      const pending = connectMarketplace(devCtx, {
        marketplace: 'ozon',
        schemes: ['fbo'],
        token: 'k',
        client_id: '1',
      })
      const assertion = expect(pending).resolves.toMatchObject({ status: 'verified' })
      await vi.advanceTimersByTimeAsync(12000)
      await assertion
    } finally {
      vi.useRealTimers()
    }
  })
})
