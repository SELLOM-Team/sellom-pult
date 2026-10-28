import { describe, expect, it } from 'vitest'
import {
  MARKETPLACE_LABELS,
  OWNER_ZONE_NOTE,
  SCHEMES,
  canSubmitConnect,
  validateConnectForm,
  wizardStep,
} from './step'

describe('wizardStep — активный шаг мастера', () => {
  it('без площадки и итога — шаг 1 (выбор площадки)', () => {
    expect(wizardStep(null, null)).toBe(1)
  })

  it('выбранная площадка без итога — шаг 2 (форма)', () => {
    expect(wizardStep('wildberries', null)).toBe(2)
    expect(wizardStep('ozon', null)).toBe(2)
  })

  it('итог автотеста открывает шаг 3, даже если площадка потеряна', () => {
    expect(wizardStep(null, { status: 'verified', rights: [] })).toBe(3)
    expect(wizardStep('ozon', { status: 'rejected', checks: [] })).toBe(3)
    expect(wizardStep(null, undefined)).toBe(1)
  })
})

describe('SCHEMES — матрица схем по площадке (серверная половина в core/identity)', () => {
  it('WB: fbo/fbs/realfbs; Ozon: fbo/fbs/rfbs', () => {
    expect(SCHEMES.wildberries.map((s) => s.id)).toEqual(['fbo', 'fbs', 'realfbs'])
    expect(SCHEMES.ozon.map((s) => s.id)).toEqual(['fbo', 'fbs', 'rfbs'])
  })

  it('у обеих площадок есть человекочитаемые метки и имя', () => {
    for (const id of ['wildberries', 'ozon'] as const) {
      expect(MARKETPLACE_LABELS[id].length).toBeGreaterThan(0)
      for (const scheme of SCHEMES[id]) {
        expect(scheme.label.length).toBeGreaterThan(0)
      }
    }
  })
})

describe('validateConnectForm — клиентская половина 422-контракта', () => {
  it('пустой токен — ошибка, валидный WB — чисто', () => {
    expect(validateConnectForm('wildberries', '', '', ['fbo']).token).not.toBe('')
    expect(validateConnectForm('wildberries', '  ', '', ['fbo']).token).not.toBe('')
    expect(validateConnectForm('wildberries', 'ey-token', '', ['fbo']).token).toBe('')
  })

  it('токен с переносом строки отвергается (мусор вставки)', () => {
    const errors = validateConnectForm('wildberries', 'ey\ntoken', '', ['fbo'])
    expect(errors.token).toMatch(/перенос/)
  })

  it('паритет с сервером: >1024 символов и control-символы ловятся клиентом (ревью 1.3)', () => {
    const tooLong = validateConnectForm('wildberries', 'x'.repeat(1025), '', ['fbo'])
    expect(tooLong.token).toMatch(/слишком длин/)
    const control = validateConnectForm('wildberries', 'ey\u001btoken', '', ['fbo'])
    expect(control.token).toMatch(/управляющ/)
    expect(validateConnectForm('wildberries', 'x'.repeat(1024), '', ['fbo']).token).toBe('')
  })

  it('Ozon без Client-Id — ошибка; с Client-Id — чисто', () => {
    const without = validateConnectForm('ozon', 'key', '', ['fbo'])
    expect(without.clientId).toMatch(/Client-Id/)
    const withId = validateConnectForm('ozon', 'key', '42', ['fbo'])
    expect(withId.clientId).toBe('')
  })

  it('WB Client-Id не требуется (контракт без односторонних требований)', () => {
    expect(validateConnectForm('wildberries', 'tok', '', ['fbo']).clientId).toBe('')
  })

  it('пустой список схем — ошибка; реальная схема чужой площадки — ошибка', () => {
    expect(validateConnectForm('wildberries', 't', '', []).schemes).toMatch(/одну схему/)
    expect(validateConnectForm('ozon', 't', '1', ['realfbs']).schemes).toMatch(/не существует/)
    expect(validateConnectForm('ozon', 't', '1', ['rfbs', 'fbo']).schemes).toBe('')
  })
})

describe('canSubmitConnect — гейт отправки', () => {
  const clean = { token: '', clientId: '', schemes: '' }

  it('чистая форма и не в полёте — можно отправлять', () => {
    expect(canSubmitConnect(clean, false)).toBe(true)
  })

  it('в полёте или с любой ошибкой — нельзя', () => {
    expect(canSubmitConnect(clean, true)).toBe(false)
    expect(canSubmitConnect({ ...clean, token: 'ошибка' }, false)).toBe(false)
    expect(canSubmitConnect({ ...clean, schemes: 'ошибка' }, false)).toBe(false)
  })
})

describe('OWNER_ZONE_NOTE — честная пометка зоны Владельца (FR-29)', () => {
  it('упоминает Владельца и шаг 6 (контроль ролей — Story 1.6, не бутафорский замок)', () => {
    expect(OWNER_ZONE_NOTE).toContain('Владелец')
    expect(OWNER_ZONE_NOTE).toContain('шаге 6')
  })
})
