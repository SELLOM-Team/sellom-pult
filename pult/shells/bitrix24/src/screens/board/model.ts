/**
 * Витрина доски заказов (Story 1.4, FR-16) — чистые функции без React.
 *
 * Доска — 7 колонок словаря FR-16, без фильтров (полный канбан — Epic 4).
 * Честные состояния: 409-конверты шлюза (синк идёт/не удался/не начинался)
 * отображаются объяснениями, а не пустой заглушкой. Тестируется напрямую
 * (model.test.ts).
 */

import { ApiError, type BoardCard, type BoardColumnKey, type BoardData } from '../../api'

/** Колонки витрины в порядке слева направо (словарь FR-16). */
export const BOARD_COLUMNS: readonly { key: BoardColumnKey; title: string }[] = [
  { key: 'new', title: 'Новые' },
  { key: 'assembling', title: 'К сборке' },
  { key: 'shipping', title: 'К отгрузке' },
  { key: 'in_transit', title: 'В пути' },
  { key: 'delivered', title: 'Доставлены' },
  { key: 'returned', title: 'Возвраты' },
  { key: 'risk', title: 'Риск' },
]

/** Честное состояние секции доски (что рисовать). */
export type BoardViewState =
  | { kind: 'loading' }
  | { kind: 'board'; data: BoardData }
  | { kind: 'empty' }
  | { kind: 'syncing' }
  | { kind: 'failed'; hint: string }
  | { kind: 'offline' }
  | { kind: 'error'; message: string }

/** 409-коды шлюза, означающие «заказы ещё синкаются». */
const SYNCING_CODES: readonly string[] = [
  'sync_in_progress',
  'sync_not_started',
  'sync_superseded',
]

/**
 * Состояние доски из последнего запроса: данные → доска (или честное
 * «заказов пока нет» при пустых колонках); ApiError → объяснение по коду;
 * сеть → offline.
 */
export function boardViewState(err: unknown, data: BoardData | null): BoardViewState {
  if (data !== null) {
    const hasCards = BOARD_COLUMNS.some(({ key }) => data.columns[key].length > 0)
    return hasCards ? { kind: 'board', data } : { kind: 'empty' }
  }
  if (err instanceof ApiError) {
    if (SYNCING_CODES.includes(err.code)) return { kind: 'syncing' }
    if (err.code === 'sync_failed') {
      return { kind: 'failed', hint: err.message }
    }
    if (err.code === 'network_error' || err.code === 'invalid_response') {
      return { kind: 'offline' }
    }
    return { kind: 'error', message: err.message }
  }
  return { kind: 'loading' }
}

/**
 * Копейки → «90,00 ₽» (разделитель тысяч — неразрывный пробел; запятая —
 * разделитель дроби, по-русски). Отрицательных сумм на витрине нет, но
 * функция их не ломает.
 */
export function formatKopecks(kopecks: number): string {
  const sign = kopecks < 0 ? '−' : ''
  const abs = Math.abs(Math.round(kopecks))
  const rubles = Math.floor(abs / 100)
  const cents = abs % 100
  const groups: string[] = []
  let rest = rubles
  do {
    groups.unshift(String(rest % 1000))
    rest = Math.floor(rest / 1000)
  } while (rest > 0)
  const whole = groups
    .map((group, index) => (index === 0 ? group : group.padStart(3, '0')))
    .join('\u00A0')
  return `${sign}${whole},${String(cents).padStart(2, '0')}\u00A0₽`
}

/** Заголовок карточки: «№771 · FBO». */
export function cardHeadline(card: BoardCard): string {
  return `№${card.external_order_id} · ${card.scheme.toUpperCase()}`
}

/** Строка позиции: «ART-DELIVERED, M × 1». */
export function itemLine(item: {
  article: string | null
  size: string | null
  quantity: number
}): string {
  const head = item.article ?? 'позиция'
  const size = item.size ? `, ${item.size}` : ''
  return `${head}${size} × ${item.quantity}`
}

/** Есть ли в колонке непоказанные заказы («показаны первые N из M»). */
export function hiddenCount(data: BoardData, key: BoardColumnKey): number {
  return Math.max(0, data.totals[key] - data.columns[key].length)
}
