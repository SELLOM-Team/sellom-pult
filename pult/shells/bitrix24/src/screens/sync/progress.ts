/**
 * Витрина прогресса синка (Story 1.4) — чистые функции без React.
 *
 * Домен честности (спека): фаза/объём по фактическим чекпоинтам, ETA —
 * оценка из наблюдаемой скорости (null → «оцениваем», не выдумываем);
 * ошибки — видимый статус с диагностикой, не «зелёный» прогресс.
 * Тестируются напрямую (progress.test.ts).
 */

import { ApiError, type SyncProgress, type SyncStatus } from '../../api'

/** Статусы, при которых опрос продолжается (цикл ещё жив). */
export const ACTIVE_SYNC_STATUSES: readonly SyncStatus[] = [
  'not_started',
  'pending',
  'running',
  'needs_retry',
]

/** Класс ошибки опроса для витрины (как в appState.ts — но для виджета). */
export type ProgressErrorKind = 'gateway' | 'network'

/** Классификация ошибки опроса: конверт шлюза — gateway, сеть — network. */
export function classifyProgressError(err: unknown): ProgressErrorKind {
  if (
    err instanceof ApiError &&
    err.code !== 'network_error' &&
    err.code !== 'invalid_response'
  ) {
    return 'gateway'
  }
  return 'network'
}

/** Витрина прогресса: ровно то, что рисует виджет. */
export interface ProgressView {
  /** Заголовок состояния. */
  title: string
  /** Подзаголовок: счётчики, ETA, диагностика. */
  note: string
  /** Процент для полосы (0..100). */
  percent: number
  /** Показывать кнопку «Повторить синк» (только терминальный failed). */
  retry: boolean
  /** Цикл завершён успешно — доска финальная. */
  done: boolean
  /** Работа идёт (полоса анимируется, опрос продолжается). */
  active: boolean
  /** Диагностический тон виджета. */
  tone: 'working' | 'ok' | 'failed'
}

/** Формат ETA: секунды → человекочитаемо («меньше минуты», «~2 мин»). */
export function formatEta(seconds: number): string {
  if (seconds < 60) return 'меньше минуты'
  if (seconds < 3600) {
    const minutes = Math.round(seconds / 60)
    return `~${minutes} мин`
  }
  const hours = Math.floor(seconds / 3600)
  const minutes = Math.round((seconds % 3600) / 60)
  return minutes > 0 ? `~${hours} ч ${minutes} мин` : `~${hours} ч`
}

/** Счётчики одной строкой: «3/5 окон · заказов 12». */
function countersNote(p: SyncProgress): string {
  const parts = [`${p.details.windows_done}/${p.details.windows_total} окон`]
  if (p.details.orders_fetched > 0 || p.status === 'succeeded') {
    parts.push(`заказов ${p.details.orders_fetched}`)
  }
  return parts.join(' · ')
}

/** Витрина прогресса из ответа шлюза (см. спеку 1.4, I/O-матрица). */
export function progressView(p: SyncProgress): ProgressView {
  const base = { percent: p.percent, note: countersNote(p) }
  switch (p.status) {
    case 'not_started':
      // Цикла ещё нет: воркер подхватит verified-подключение своим тиком.
      return {
        ...base,
        title: 'Ждём первый цикл загрузки истории',
        note: 'Воркер конвейера подхватит подключение автоматически — это займёт до минуты',
        percent: 0,
        retry: false,
        done: false,
        active: true,
        tone: 'working',
      }
    case 'pending':
      return {
        ...base,
        title: 'Готовим цикл загрузки истории',
        retry: false,
        done: false,
        active: true,
        tone: 'working',
      }
    case 'running':
      return {
        ...base,
        title: 'Загружаем историю заказов',
        note:
          countersNote(p) +
          (p.eta_seconds === null
            ? ' · оцениваем время'
            : ` · осталось ${formatEta(p.eta_seconds)}`),
        retry: false,
        done: false,
        active: true,
        tone: 'working',
      }
    case 'needs_retry':
      // Временный сбой: продолжение с чекпоинта — автоматическое (AD-3).
      return {
        ...base,
        title: 'Загрузка продолжится автоматически',
        note: p.details.error_hint ?? countersNote(p),
        retry: false,
        done: false,
        active: true,
        tone: 'working',
      }
    case 'failed':
      return {
        ...base,
        title: 'Загрузка истории не удалась',
        note: p.details.error_hint ?? 'причина неизвестна — попробуйте повторить синк',
        percent: p.percent,
        retry: true,
        done: false,
        active: false,
        tone: 'failed',
      }
    case 'succeeded':
      return {
        ...base,
        title: 'История загружена',
        percent: 100,
        retry: false,
        done: true,
        active: false,
        tone: 'ok',
      }
    case 'superseded':
      // Цикл заменён новым (ротация/усечение — будущие сторис): ждём новый.
      return {
        ...base,
        title: 'Цикл заменён новым',
        note: 'Дождитесь нового цикла загрузки',
        retry: false,
        done: false,
        active: false,
        tone: 'working',
      }
  }
}
