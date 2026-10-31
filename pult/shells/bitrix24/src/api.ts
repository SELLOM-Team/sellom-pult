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
 * Бюджет POST /v1/connections: сервер может гонять до 2 проб × 10с
 * (PROBE_TIMEOUT) последовательно + БД — клиентский бюджет обязан покрывать
 * худший случай, иначе честный probe_unavailable не доедет до шага 3
 * (ревью Story 1.3: рассинхрон бюджетов = ложный network_error).
 */
export const CONNECT_TIMEOUT_MS = 30000

const REQUEST_TIMEOUT_MS = 8000

/**
 * Выполняет запрос к шлюзу с бюджетом времени (по умолчанию 8с, для
 * долгих операций — свой бюджет параметром) и возвращает разобранное
 * JSON-тело успешного ответа. Таймаут обязателен (ревью Story 1.2):
 * зависший шлюз — это офлайн, а не вечный скелетон. Любая ошибка —
 * ApiError: network_error (нет сети/сервера/просрочен бюджет),
 * invalid_response (не-JSON/битый JSON/ошибка без конверта) либо код
 * error-конверта шлюза (например, database_unavailable при 503).
 */
async function requestJson(
  url: string,
  init?: RequestInit,
  budgetMs: number = REQUEST_TIMEOUT_MS,
): Promise<unknown> {
  const controller = new AbortController()
  const timer = setTimeout(() => controller.abort(), budgetMs)
  let response: Response
  try {
    response = await fetch(url, { ...init, signal: controller.signal })
  } catch {
    throw new ApiError(
      'network_error',
      'шлюз недоступен: сеть или сервер не отвечает (или превысен бюджет запроса)',
    )
  }
  // Бюджет покрывает и чтение тела, а не только заголовки (формальное ревью
  // Story 1.2): шлюз, отправивший заголовки и замолчавший посреди тела,
  // тоже уходит в network_error, а не висит вечным скелетоном.
  try {
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
  } finally {
    clearTimeout(timer)
  }
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

// --- Подключения площадок (Story 1.3, token-vault) ---

/** Площадка подключения (Ф0: две; маппинг в enum — на шлюзе). */
export type MarketplaceId = 'wildberries' | 'ozon'

/** Проверка одного права в автотесте (hint — только при отказе). */
export interface RightsCheck {
  name: string
  ok: boolean
  hint?: string
}

/** Строка GET /v1/connections — статус площадки БЕЗ секретов. */
export interface ConnectionInfo {
  marketplace: MarketplaceId
  schemes: string[]
  status: 'verified' | 'rejected' | 'probe_unavailable'
  verified_at: string | null
  client_id: string | null
  rights: RightsCheck[]
}

/** Разбор GET /v1/connections: чужая форма элемента — ошибка. */
export function parseConnections(body: unknown): ConnectionInfo[] {
  const list = (body as { connections?: unknown } | null | undefined)?.connections
  if (!Array.isArray(list)) {
    throw new ApiError('invalid_response', 'шлюз вернул список подключений неожиданной формы')
  }
  // Контракт статусов домена: в 1.3 пишется только verified, но rejected /
  // probe_unavailable допустимы схемой БД и status_from_str — парсер обязан
  // принимать все три, иначе одна будущая строка роняет весь список
  // (ревью Story 1.3).
  const STATUSES = ['verified', 'rejected', 'probe_unavailable'] as const
  return list.map((item) => {
    const row = item as Record<string, unknown>
    const marketplace = row.marketplace
    const status = row.status
    if (
      (marketplace === 'wildberries' || marketplace === 'ozon') &&
      (STATUSES as readonly unknown[]).includes(status) &&
      Array.isArray(row.schemes) &&
      Array.isArray(row.rights) &&
      (row.verified_at === null || typeof row.verified_at === 'string')
    ) {
      return {
        marketplace,
        schemes: row.schemes as string[],
        status: status as ConnectionInfo['status'],
        verified_at: (row.verified_at as string | null) ?? null,
        client_id: (row.client_id as string | null) ?? null,
        rights: row.rights as RightsCheck[],
      }
    }
    throw new ApiError('invalid_response', 'шлюз вернул подключение неожиданной формы')
  })
}

/** Итог автотеста прав POST /v1/connections (права — только при verified). */
export type ConnectOutcome =
  | { status: 'verified'; rights: RightsCheck[] }
  | { status: 'rejected' | 'probe_unavailable'; checks: RightsCheck[] }

/** Разбор POST /v1/connections: статусы и форма прав консистентны. */
export function parseConnectOutcome(body: unknown): ConnectOutcome {
  const row = body as Record<string, unknown> | null | undefined
  const checksOf = (value: unknown): RightsCheck[] =>
    Array.isArray(value) ? (value as RightsCheck[]) : []
  if (row?.status === 'verified') {
    return { status: 'verified', rights: checksOf(row.rights) }
  }
  if (row?.status === 'rejected' || row?.status === 'probe_unavailable') {
    return { status: row.status, checks: checksOf(row.checks) }
  }
  throw new ApiError('invalid_response', 'шлюз вернул итог автотеста неожиданной формы')
}

/** GET /v1/connections — статусы подключений портала (без секретов). */
export async function fetchConnections(ctx: B24PlacementContext): Promise<ConnectionInfo[]> {
  const query = identityQuery(ctx)
  return parseConnections(await requestJson(`/v1/connections${query ? `?${query}` : ''}`))
}

/**
 * POST /v1/connections — автотест прав и сохранение при успехе (шаги 2–3
 * мастера). Токен передаётся только в теле запроса (TLS) и никогда не
 * возвращается: ответы содержат только права/диагностику.
 */
export async function connectMarketplace(
  ctx: B24PlacementContext,
  payload: {
    marketplace: MarketplaceId
    schemes: string[]
    token: string
    client_id?: string
  },
): Promise<ConnectOutcome> {
  const query = identityQuery(ctx)
  // Бюджет покрывает худший случай сервера (2 пробы × 10с + БД) — см.
  // CONNECT_TIMEOUT_MS; общий 8с оборвал бы честный probe_unavailable.
  return parseConnectOutcome(
    await requestJson(
      `/v1/connections${query ? `?${query}` : ''}`,
      {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(payload),
      },
      CONNECT_TIMEOUT_MS,
    ),
  )
}

/** DELETE /v1/connections/{marketplace} — отзыв токена (удаление строки). */
export async function deleteConnection(
  ctx: B24PlacementContext,
  marketplace: MarketplaceId,
): Promise<void> {
  const query = identityQuery(ctx)
  await requestJson(`/v1/connections/${marketplace}${query ? `?${query}` : ''}`, {
    method: 'DELETE',
  })
}

// --- Прогресс синка и доска заказов (Story 1.4) ---

/** Статус цикла синка: словарь CHECK шлюза + `not_started` (цикла нет). */
export type SyncStatus =
  | 'pending'
  | 'running'
  | 'succeeded'
  | 'failed'
  | 'needs_retry'
  | 'superseded'
  | 'not_started'

/** Фаза цикла (домен core/sync: производная статуса и счётчиков). */
export type SyncPhase = 'pending' | 'fetching_history' | 'completed'

/** Ответ GET /v1/sync/progress (и POST /v1/sync/retry — единая форма). */
export interface SyncProgress {
  status: SyncStatus
  phase: SyncPhase
  percent: number
  eta_seconds: number | null
  details: {
    cycle: string
    windows_total: number
    windows_done: number
    orders_fetched: number
    events_published: number
    completed_until: string | null
    error_hint: string | null
  }
}

const SYNC_STATUSES: readonly string[] = [
  'pending',
  'running',
  'succeeded',
  'failed',
  'needs_retry',
  'superseded',
  'not_started',
]

const SYNC_PHASES: readonly string[] = ['pending', 'fetching_history', 'completed']

/**
 * Разбор ответа прогресса: чужая форма — invalid_response (паттерн 1.2/1.3).
 * `percent` зажимается в 0..100, `eta_seconds` — null или неотрицательное
 * число (шлюз так и отвечает; зажимы — защита витрины от кривых данных).
 */
export function parseSyncProgress(body: unknown): SyncProgress {
  const row = body as Record<string, unknown> | null | undefined
  const details = row?.details as Record<string, unknown> | null | undefined
  if (
    typeof row === 'object' &&
    row !== null &&
    typeof row.status === 'string' &&
    SYNC_STATUSES.includes(row.status) &&
    typeof row.phase === 'string' &&
    SYNC_PHASES.includes(row.phase) &&
    typeof row.percent === 'number' &&
    (row.eta_seconds === null || typeof row.eta_seconds === 'number') &&
    typeof details === 'object' &&
    details !== null &&
    typeof details.cycle === 'string' &&
    typeof details.windows_total === 'number' &&
    typeof details.windows_done === 'number' &&
    typeof details.orders_fetched === 'number' &&
    typeof details.events_published === 'number' &&
    (details.completed_until === null || typeof details.completed_until === 'string') &&
    (details.error_hint === null || typeof details.error_hint === 'string')
  ) {
    return {
      status: row.status as SyncStatus,
      phase: row.phase as SyncPhase,
      percent: Math.max(0, Math.min(100, Math.round(row.percent))),
      eta_seconds:
        row.eta_seconds === null ? null : Math.max(0, Math.round(row.eta_seconds)),
      details: {
        cycle: details.cycle,
        windows_total: details.windows_total,
        windows_done: details.windows_done,
        orders_fetched: details.orders_fetched,
        events_published: details.events_published,
        completed_until: details.completed_until as string | null,
        error_hint: details.error_hint as string | null,
      },
    }
  }
  throw new ApiError('invalid_response', 'шлюз вернул прогресс синка неожиданной формы')
}

/** GET /v1/sync/progress — честный прогресс цикла для пользователя. */
export async function fetchSyncProgress(ctx: B24PlacementContext): Promise<SyncProgress> {
  const query = identityQuery(ctx)
  return parseSyncProgress(
    await requestJson(`/v1/sync/progress${query ? `?${query}` : ''}`),
  )
}

/**
 * POST /v1/sync/retry — сброс терминального `failed` (идемпотентен: активные
 * и завершённые циклы — NO-OP, ответ всё равно свежий прогресс).
 */
export async function retrySync(ctx: B24PlacementContext): Promise<SyncProgress> {
  const query = identityQuery(ctx)
  return parseSyncProgress(
    await requestJson(`/v1/sync/retry${query ? `?${query}` : ''}`, { method: 'POST' }),
  )
}

/** Колонка канбана — 7-словарь FR-16 (контракт БД и API). */
export type BoardColumnKey =
  | 'new'
  | 'assembling'
  | 'shipping'
  | 'in_transit'
  | 'delivered'
  | 'returned'
  | 'risk'

/** Все 7 колонок — источник порядка витрины (проверка парсера). */
export const BOARD_COLUMN_KEYS: readonly BoardColumnKey[] = [
  'new',
  'assembling',
  'shipping',
  'in_transit',
  'delivered',
  'returned',
  'risk',
]

/** Позиция карточки заказа. */
export interface BoardItem {
  article: string | null
  size: string | null
  nm_id: number | null
  quantity: number
  total_kopecks: number
}

/** Карточка заказа на доске (суммы — копейки; форматирование — на витрине). */
export interface BoardCard {
  id: string
  external_order_id: string
  marketplace: string
  scheme: string
  board_column: BoardColumnKey
  status_code: string | null
  warehouse: string | null
  total_kopecks: number
  currency: string
  cancelled: boolean
  order_date: string | null
  last_change_at: string | null
  items: BoardItem[]
}

/** Ответ GET /v1/board/orders (все 7 ключей приходят всегда — контракт). */
export interface BoardData {
  columns: Record<BoardColumnKey, BoardCard[]>
  totals: Record<BoardColumnKey, number>
}

function parseBoardCard(row: Record<string, unknown>): BoardCard {
  const column = row.board_column
  if (
    typeof row.id === 'string' &&
    typeof row.external_order_id === 'string' &&
    typeof row.marketplace === 'string' &&
    typeof row.scheme === 'string' &&
    (BOARD_COLUMN_KEYS as readonly unknown[]).includes(column) &&
    typeof row.total_kopecks === 'number' &&
    typeof row.currency === 'string' &&
    typeof row.cancelled === 'boolean' &&
    (row.status_code === null || typeof row.status_code === 'string') &&
    (row.warehouse === null || typeof row.warehouse === 'string') &&
    (row.order_date === null || typeof row.order_date === 'string') &&
    (row.last_change_at === null || typeof row.last_change_at === 'string') &&
    Array.isArray(row.items)
  ) {
    return {
      id: row.id,
      external_order_id: row.external_order_id,
      marketplace: row.marketplace,
      scheme: row.scheme,
      board_column: column as BoardColumnKey,
      status_code: row.status_code as string | null,
      warehouse: row.warehouse as string | null,
      total_kopecks: row.total_kopecks,
      currency: row.currency,
      cancelled: row.cancelled,
      order_date: row.order_date as string | null,
      last_change_at: row.last_change_at as string | null,
      items: (row.items as Record<string, unknown>[]).map((item) => ({
        article: (item.article as string | null) ?? null,
        size: (item.size as string | null) ?? null,
        nm_id: (item.nm_id as number | null) ?? null,
        quantity: typeof item.quantity === 'number' ? item.quantity : 1,
        total_kopecks: typeof item.total_kopecks === 'number' ? item.total_kopecks : 0,
      })),
    }
  }
  throw new ApiError('invalid_response', 'шлюз вернул карточку заказа неожиданной формы')
}

/**
 * Разбор доски: все 7 ключей columns и totals обязательны (контракт — пустые
 * колонки приходят явными пустыми массивами, а не отсутствием ключей).
 */
export function parseBoardOrders(body: unknown): BoardData {
  const row = body as Record<string, unknown> | null | undefined
  const columns = row?.columns as Record<string, unknown> | null | undefined
  const totals = row?.totals as Record<string, unknown> | null | undefined
  if (typeof columns !== 'object' || columns === null || typeof totals !== 'object' || totals === null) {
    throw new ApiError('invalid_response', 'шлюз вернул доску неожиданной формы')
  }
  const parsedColumns = {} as Record<BoardColumnKey, BoardCard[]>
  const parsedTotals = {} as Record<BoardColumnKey, number>
  for (const key of BOARD_COLUMN_KEYS) {
    const list = columns[key]
    const total = totals[key]
    if (!Array.isArray(list) || typeof total !== 'number') {
      throw new ApiError('invalid_response', `шлюз вернул доску без колонки «${key}»`)
    }
    parsedColumns[key] = list.map((item) => parseBoardCard(item as Record<string, unknown>))
    parsedTotals[key] = total
  }
  return { columns: parsedColumns, totals: parsedTotals }
}

/**
 * GET /v1/board/orders — канбан 7 колонок. Честные состояния без заказов
 * приходят 409-конвертами (sync_in_progress / sync_failed /
 * sync_not_started / sync_superseded) — они разбираются в ApiError с тем же
 * кодом, витрина решает, что показывать.
 */
export async function fetchBoardOrders(ctx: B24PlacementContext): Promise<BoardData> {
  const query = identityQuery(ctx)
  return parseBoardOrders(await requestJson(`/v1/board/orders${query ? `?${query}` : ''}`))
}
