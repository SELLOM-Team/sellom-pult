import { describe, expect, it } from 'vitest'
import { canSubmitStep0, needsMarketplaces } from './step0'

describe('шаг 0 — гейтинг ответов вопросника (клиентская половина контракта)', () => {
  it('селлерские модели требуют площадки', () => {
    expect(needsMarketplaces('marketplace_seller')).toBe(true)
    expect(needsMarketplaces('marketplace_planned')).toBe(true)
    expect(needsMarketplaces('own_channels_no_marketplaces')).toBe(false)
    expect(needsMarketplaces('services_other')).toBe(false)
    expect(needsMarketplaces(null)).toBe(false)
  })

  it('без модели кнопка заблокирована', () => {
    expect(canSubmitStep0(null, [], false)).toBe(false)
    expect(canSubmitStep0(null, ['wildberries'], false)).toBe(false)
  })

  it('селлерская модель: кнопка ждёт хотя бы одну площадку', () => {
    expect(canSubmitStep0('marketplace_seller', [], false)).toBe(false)
    expect(canSubmitStep0('marketplace_planned', [], false)).toBe(false)
    expect(canSubmitStep0('marketplace_seller', ['ozon'], false)).toBe(true)
    expect(canSubmitStep0('marketplace_planned', ['wildberries', 'ozon'], false)).toBe(true)
  })

  it('не-селлерские модели отправляются без площадок', () => {
    expect(canSubmitStep0('services_other', [], false)).toBe(true)
    expect(canSubmitStep0('own_channels_no_marketplaces', [], false)).toBe(true)
  })

  it('во время отправки кнопка заблокирована (защита от двойного POST)', () => {
    expect(canSubmitStep0('marketplace_seller', ['ozon'], true)).toBe(false)
    expect(canSubmitStep0('services_other', [], true)).toBe(false)
  })
})
