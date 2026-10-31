import { describe, expect, it } from 'vitest'
import { ApiError, BOARD_COLUMN_KEYS, type BoardData } from '../../api'
import {
  BOARD_COLUMNS,
  boardViewState,
  cardHeadline,
  formatKopecks,
  hiddenCount,
  itemLine,
} from './model'

function emptyBoard(): BoardData {
  const columns = {} as BoardData['columns']
  const totals = {} as BoardData['totals']
  for (const key of BOARD_COLUMN_KEYS) {
    columns[key] = []
    totals[key] = 0
  }
  return { columns, totals }
}

function boardWithCard(): BoardData {
  const board = emptyBoard()
  board.columns.delivered = [
    {
      id: '018f3b00-0000-7000-8000-000000000001',
      external_order_id: '771',
      marketplace: 'wildberries',
      scheme: 'fbo',
      board_column: 'delivered',
      status_code: '3',
      warehouse: 'Коледино',
      total_kopecks: 9000,
      currency: 'RUB',
      cancelled: false,
      order_date: '2026-10-30T16:25:50Z',
      last_change_at: '2026-10-30T16:25:50Z',
      items: [
        {
          article: 'ART-DELIVERED',
          size: 'M',
          nm_id: 101,
          quantity: 1,
          total_kopecks: 9000,
        },
      ],
    },
  ]
  board.totals.delivered = 1
  return board
}

describe('BOARD_COLUMNS — словарь FR-16 в порядке витрины', () => {
  it('ровно 7 колонок с заголовками, порядок фиксирован', () => {
    expect(BOARD_COLUMNS.map((c) => c.key)).toEqual([
      'new',
      'assembling',
      'shipping',
      'in_transit',
      'delivered',
      'returned',
      'risk',
    ])
    expect(BOARD_COLUMNS.every((c) => c.title.length > 0)).toBe(true)
  })
})

describe('boardViewState — честные состояния', () => {
  it('данные с карточками → доска', () => {
    expect(boardViewState(null, boardWithCard()).kind).toBe('board')
  })

  it('пустая доска при succeeded → честное «заказов пока нет»', () => {
    expect(boardViewState(null, emptyBoard()).kind).toBe('empty')
  })

  it('409-конверты синка → «загружается» (in_progress/not_started/superseded)', () => {
    expect(
      boardViewState(new ApiError('sync_in_progress', 'синк идёт', 409), null).kind,
    ).toBe('syncing')
    expect(
      boardViewState(new ApiError('sync_not_started', 'цикла нет', 409), null).kind,
    ).toBe('syncing')
    expect(
      boardViewState(new ApiError('sync_superseded', 'заменён', 409), null).kind,
    ).toBe('syncing')
  })

  it('409 sync_failed → видимый сбой с подсказкой', () => {
    const view = boardViewState(
      new ApiError('sync_failed', 'токен отклонён площадкой', 409),
      null,
    )
    expect(view.kind).toBe('failed')
    if (view.kind === 'failed') expect(view.hint).toContain('токен')
  })

  it('сеть/битый ответ → offline; прочий конверт → error', () => {
    expect(boardViewState(new ApiError('network_error', 'нет сети'), null).kind).toBe('offline')
    expect(
      boardViewState(new ApiError('database_unavailable', 'БД', 503), null).kind,
    ).toBe('error')
  })

  it('нет ни данных, ни ошибки → загрузка', () => {
    expect(boardViewState(null, null).kind).toBe('loading')
  })
})

describe('formatKopecks — суммы витрины', () => {
  it('копейки → рубли с разделителями', () => {
    expect(formatKopecks(9000)).toBe('90,00\u00A0₽')
    expect(formatKopecks(5005)).toBe('50,05\u00A0₽')
    expect(formatKopecks(123456)).toBe('1\u00A0234,56\u00A0₽')
    expect(formatKopecks(100)).toBe('1,00\u00A0₽')
    expect(formatKopecks(0)).toBe('0,00\u00A0₽')
  })

  it('большие суммы группируются по три', () => {
    expect(formatKopecks(123456789)).toBe('1\u00A0234\u00A0567,89\u00A0₽')
  })
})

describe('карточки — заголовок и позиции', () => {
  it('заголовок: номер + схема', () => {
    const card = boardWithCard().columns.delivered[0]!
    expect(cardHeadline(card)).toBe('№771 · FBO')
  })

  it('позиция: артикул, размер, количество', () => {
    expect(itemLine({ article: 'ART-DELIVERED', size: 'M', quantity: 1 })).toBe(
      'ART-DELIVERED, M × 1',
    )
    expect(itemLine({ article: null, size: null, quantity: 2 })).toBe('позиция × 2')
  })

  it('hiddenCount: показано меньше, чем есть (лимит 50/колонку)', () => {
    const board = emptyBoard()
    board.columns.new = Array.from({ length: 50 }, (_, i) => ({
      ...boardWithCard().columns.delivered[0]!,
      id: `card-${i}`,
    }))
    board.totals.new = 137
    expect(hiddenCount(board, 'new')).toBe(87)
    expect(hiddenCount(board, 'delivered')).toBe(0)
  })
})
