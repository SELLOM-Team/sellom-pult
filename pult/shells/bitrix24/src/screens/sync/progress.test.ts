import { describe, expect, it } from 'vitest'
import { ApiError, type SyncProgress } from '../../api'
import { classifyProgressError, formatEta, progressView, ACTIVE_SYNC_STATUSES } from './progress'

function progress(partial: Partial<SyncProgress>): SyncProgress {
  return {
    status: 'running',
    phase: 'fetching_history',
    percent: 40,
    eta_seconds: null,
    details: {
      cycle: 'initial_orders',
      windows_total: 5,
      windows_done: 2,
      orders_fetched: 12,
      events_published: 12,
      completed_until: '2026-10-20T00:00:00+00:00',
      error_hint: null,
    },
    ...partial,
  }
}

describe('progressView — честная витрина статусов', () => {
  it('running: счётчики + оценка времени, полоса активна', () => {
    const view = progressView(progress({ percent: 40, eta_seconds: 90 }))
    expect(view.title).toBe('Загружаем историю заказов')
    expect(view.percent).toBe(40)
    expect(view.note).toContain('2/5 окон')
    expect(view.note).toContain('заказов 12')
    expect(view.note).toContain('~2 мин')
    expect(view.active).toBe(true)
    expect(view.retry).toBe(false)
    expect(view.tone).toBe('working')
  })

  it('running без ETA — «оцениваем», не выдумываем', () => {
    const view = progressView(progress({ eta_seconds: null }))
    expect(view.note).toContain('оцениваем время')
    expect(view.note).not.toContain('~')
  })

  it('succeeded: 100%, финальная доска, опрос останавливается', () => {
    const view = progressView(progress({ status: 'succeeded', phase: 'completed', percent: 100 }))
    expect(view.title).toBe('История загружена')
    expect(view.percent).toBe(100)
    expect(view.done).toBe(true)
    expect(view.active).toBe(false)
    expect(view.tone).toBe('ok')
    expect(view.note).toContain('заказов 12')
  })

  it('failed: видимый сбой + кнопка повтора, не «зелёный» прогресс', () => {
    const view = progressView(
      progress({
        status: 'failed',
        phase: 'fetching_history',
        percent: 0,
        eta_seconds: null,
        details: {
          cycle: 'initial_orders',
          windows_total: 5,
          windows_done: 0,
          orders_fetched: 0,
          events_published: 0,
          completed_until: null,
          error_hint: 'токен WB отклонён площадкой (отозван/недействителен): WB-статистика ответила 401',
        },
      }),
    )
    expect(view.title).toBe('Загрузка истории не удалась')
    expect(view.retry).toBe(true)
    expect(view.active).toBe(false)
    expect(view.tone).toBe('failed')
    expect(view.note).toContain('токен WB отклонён площадкой')
  })

  it('needs_retry: автоматическое продолжение, без кнопки повтора (AD-3)', () => {
    const view = progressView(
      progress({
        status: 'needs_retry',
        details: {
          cycle: 'initial_orders',
          windows_total: 5,
          windows_done: 2,
          orders_fetched: 4,
          events_published: 4,
          completed_until: '2026-10-10T00:00:00+00:00',
          error_hint: 'конвейер: брокер не подтвердил события окна',
        },
      }),
    )
    expect(view.title).toBe('Загрузка продолжится автоматически')
    expect(view.active).toBe(true)
    expect(view.retry).toBe(false)
    expect(view.note).toContain('брокер')
  })

  it('not_started: ждём первый цикл, опрос продолжается', () => {
    const view = progressView(
      progress({ status: 'not_started', phase: 'pending', percent: 0, eta_seconds: null }),
    )
    expect(view.title).toBe('Ждём первый цикл загрузки истории')
    expect(view.active).toBe(true)
    expect(view.percent).toBe(0)
  })

  it('pending: подготовка цикла', () => {
    const view = progressView(progress({ status: 'pending', phase: 'pending', percent: 0 }))
    expect(view.title).toBe('Готовим цикл загрузки истории')
    expect(view.active).toBe(true)
  })

  it('superseded: ожидание нового цикла', () => {
    const view = progressView(
      progress({ status: 'superseded', phase: 'completed', percent: 100 }),
    )
    expect(view.title).toBe('Цикл заменён новым')
    expect(view.active).toBe(false)
    expect(view.done).toBe(false)
  })
})

describe('formatEta — человекочитаемая оценка', () => {
  it('границы минут/часов', () => {
    expect(formatEta(0)).toBe('меньше минуты')
    expect(formatEta(59)).toBe('меньше минуты')
    expect(formatEta(90)).toBe('~2 мин')
    expect(formatEta(600)).toBe('~10 мин')
    expect(formatEta(3599)).toBe('~60 мин')
    expect(formatEta(3600)).toBe('~1 ч')
    expect(formatEta(3700)).toBe('~1 ч 2 мин')
  })
})

describe('ACTIVE_SYNC_STATUSES — опрос живых статусов', () => {
  it('активные и терминальные разделены', () => {
    expect(ACTIVE_SYNC_STATUSES).toContain('running')
    expect(ACTIVE_SYNC_STATUSES).toContain('needs_retry')
    expect(ACTIVE_SYNC_STATUSES).toContain('not_started')
    expect(ACTIVE_SYNC_STATUSES).not.toContain('succeeded')
    expect(ACTIVE_SYNC_STATUSES).not.toContain('failed')
    expect(ACTIVE_SYNC_STATUSES).not.toContain('superseded')
  })
})

describe('classifyProgressError — конверт или сеть', () => {
  it('503-конверт — ошибка шлюза (gateway)', () => {
    expect(classifyProgressError(new ApiError('database_unavailable', 'БД недоступна', 503))).toBe(
      'gateway',
    )
  })

  it('409 sync_failed — тоже gateway-класс с кодом', () => {
    expect(classifyProgressError(new ApiError('sync_failed', 'синк не удался', 409))).toBe('gateway')
  })

  it('сеть и битые ответы — network', () => {
    expect(classifyProgressError(new ApiError('network_error', 'нет сети'))).toBe('network')
    expect(classifyProgressError(new ApiError('invalid_response', 'не-JSON'))).toBe('network')
    expect(classifyProgressError(new Error('что-то другое'))).toBe('network')
  })
})
