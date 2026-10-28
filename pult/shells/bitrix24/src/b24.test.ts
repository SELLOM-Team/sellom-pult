import { describe, expect, it } from 'vitest'
import { readPlacementContext } from './b24'

// Контракт точки интеграции iframe-placement Битрикс24 (Story 1.1):
// параметры placement Б24 передаёт query-параметрами; ломается — должен
// падать сигнал, а не молча.
describe('readPlacementContext — точка интеграции iframe-placement Б24', () => {
  it('B24-query (?PLACEMENT=…&LANG=…&PLACEMENT_OPTIONS=…) даёт embedded:true и поля', () => {
    const ctx = readPlacementContext(
      '?PLACEMENT=crm.detail.widget.pult&LANG=ru&PLACEMENT_OPTIONS=%7B%22ID%22%3A42%7D',
    )
    expect(ctx.embedded).toBe(true)
    expect(ctx.placement).toBe('crm.detail.widget.pult')
    expect(ctx.lang).toBe('ru')
    expect(ctx.rawOptions).toBe('{"ID":42}')
  })

  it('пустой query — не embedded, полей нет', () => {
    const ctx = readPlacementContext('')
    expect(ctx.embedded).toBe(false)
    expect(ctx.placement).toBeUndefined()
    expect(ctx.lang).toBeUndefined()
    expect(ctx.rawOptions).toBeUndefined()
  })

  it('посторонний query без PLACEMENT — не embedded', () => {
    const ctx = readPlacementContext('?utm_source=test&LANG=en')
    expect(ctx.embedded).toBe(false)
    expect(ctx.lang).toBe('en')
  })

  it('пустой PLACEMENT не считается embedded', () => {
    expect(readPlacementContext('?PLACEMENT=').embedded).toBe(false)
  })
})
