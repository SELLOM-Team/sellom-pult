import { describe, expect, it } from 'vitest'
import { screenForState, screenStateFromError, type ScreenState } from './appState'
import { ApiError } from './api'
import { partnerSlot } from './screens/ExitScreen'

// Чистая функция роутинга App (Story 1.2): мапа состояния шага 0 → экран.
describe('screenForState — мапа состояния онбординга → экран', () => {
  const cases: { state: ScreenState; screen: ReturnType<typeof screenForState> }[] = [
    { state: { kind: 'loading' }, screen: 'skeleton' },
    { state: { kind: 'resolved', state: 'step0_required' }, screen: 'questionnaire' },
    { state: { kind: 'resolved', state: 'funnel' }, screen: 'funnel' },
    { state: { kind: 'resolved', state: 'exit_screen' }, screen: 'exit' },
    { state: { kind: 'offline' }, screen: 'unavailable' },
    {
      state: { kind: 'error', code: 'database_unavailable', message: 'БД недоступна' },
      screen: 'unavailable',
    },
  ]

  it.each(cases)('$state → $screen', ({ state, screen }) => {
    expect(screenForState(state)).toBe(screen)
  })

  it('каждая ветка определена (инверсия веток сломает тест)', () => {
    // Все три состояния онбординга дают разные экраны — вопросник не
    // показывается повторно после вердикта.
    expect(screenForState({ kind: 'resolved', state: 'funnel' })).not.toBe('questionnaire')
    expect(screenForState({ kind: 'resolved', state: 'exit_screen' })).not.toBe('questionnaire')
    expect(screenForState({ kind: 'resolved', state: 'exit_screen' })).not.toBe('funnel')
  })
})

// Классификация ошибки опроса шлюза (ревью Story 1.2: инверсия веток в App
// больше не проходит мимо тестов — шлюз-жив-но-ошибся ≠ шлюза-нет).
describe('screenStateFromError — конверт шлюза ≠ офлайн', () => {
  it('ошибка-конверт шлюза (например 503 database_unavailable) → kind: error', () => {
    const state = screenStateFromError(
      new ApiError('database_unavailable', 'База данных недоступна', 503),
    )
    expect(state).toEqual({
      kind: 'error',
      code: 'database_unavailable',
      message: 'База данных недоступна',
    })
  })

  it('network_error → офлайн (шлюза нет в этом окружении)', () => {
    expect(screenStateFromError(new ApiError('network_error', 'нет сети'))).toEqual({
      kind: 'offline',
    })
  })

  it('invalid_response (HTML-фоллбек/битый JSON) → офлайн', () => {
    expect(screenStateFromError(new ApiError('invalid_response', 'не JSON', 200))).toEqual({
      kind: 'offline',
    })
  })

  it('не-ApiError (любой другой throw) → офлайн, без падения', () => {
    expect(screenStateFromError(new Error('что-то другое'))).toEqual({ kind: 'offline' })
    expect(screenStateFromError('строка')).toEqual({ kind: 'offline' })
  })
})

// Контент-слот партнёрской рекомендации (Story 1.2): честный дефолт —
// каталог Битрикс24.Маркет; конкретного партнёра задаёт владелец.
describe('partnerSlot — слот VITE_PARTNER_*', () => {
  it('дефолт: каталог партнёров Битрикс24.Маркет', () => {
    expect(partnerSlot({})).toEqual({
      title: 'Каталог партнёров Битрикс24.Маркет',
      url: 'https://www.bitrix24.ru/apps/',
    })
  })

  it('кастомные title и url применяются', () => {
    expect(
      partnerSlot({
        VITE_PARTNER_TITLE: 'Партнёр X',
        VITE_PARTNER_URL: 'https://partner.example/offer',
      }),
    ).toEqual({ title: 'Партнёр X', url: 'https://partner.example/offer' })
  })

  it('пустые строки → честный дефолт', () => {
    expect(partnerSlot({ VITE_PARTNER_TITLE: '   ', VITE_PARTNER_URL: '  ' })).toEqual({
      title: 'Каталог партнёров Битрикс24.Маркет',
      url: 'https://www.bitrix24.ru/apps/',
    })
  })

  it('невалидный протокол URL → дефолтная ссылка (без тихих подмен)', () => {
    expect(partnerSlot({ VITE_PARTNER_URL: 'javascript:alert(1)' })).toEqual({
      title: 'Каталог партнёров Битрикс24.Маркет',
      url: 'https://www.bitrix24.ru/apps/',
    })
  })

  it('кастомный title с битым URL: title сохраняется, URL — дефолт', () => {
    expect(partnerSlot({ VITE_PARTNER_TITLE: 'Партнёр Y', VITE_PARTNER_URL: 'ftp://x' })).toEqual(
      { title: 'Партнёр Y', url: 'https://www.bitrix24.ru/apps/' },
    )
  })

  it('голая схема «https://» без хоста — не ссылка: дефолт (ревью 1.2)', () => {
    // new URL('https://') бросает ERR_INVALID_URL — парсинг, а не префикс-regex.
    expect(partnerSlot({ VITE_PARTNER_URL: 'https://' })).toEqual({
      title: 'Каталог партнёров Битрикс24.Маркет',
      url: 'https://www.bitrix24.ru/apps/',
    })
    expect(partnerSlot({ VITE_PARTNER_URL: 'совсем не url' })).toEqual({
      title: 'Каталог партнёров Битрикс24.Маркет',
      url: 'https://www.bitrix24.ru/apps/',
    })
  })
})
