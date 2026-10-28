import { describe, expect, it } from 'vitest'
import { phaseFromHealthResponse } from './health'

describe('phaseFromHealthResponse — честная семантика /health', () => {
  it('200 + JSON → gateway-up', () => {
    expect(phaseFromHealthResponse(200, 'application/json')).toBe('gateway-up')
  })

  it('503 + JSON → gateway-degraded (шлюз жив, БД нет)', () => {
    expect(phaseFromHealthResponse(503, 'application/json')).toBe('gateway-degraded')
  })

  it('200 + HTML (фоллбек статик-хостинга) → offline, не gateway-up', () => {
    expect(phaseFromHealthResponse(200, 'text/html')).toBe('offline')
    expect(phaseFromHealthResponse(200, 'text/html; charset=utf-8')).toBe('offline')
  })

  it('любой код без JSON → offline (SPA-fallback, прокси-ошибка, чужой сервер)', () => {
    expect(phaseFromHealthResponse(200, null)).toBe('offline')
    expect(phaseFromHealthResponse(404, 'application/json')).toBe('offline')
    expect(phaseFromHealthResponse(500, 'text/plain')).toBe('offline')
    expect(phaseFromHealthResponse(503, 'text/html')).toBe('offline')
  })

  it('Content-Type регистронезависим', () => {
    expect(phaseFromHealthResponse(200, 'APPLICATION/JSON')).toBe('gateway-up')
    expect(phaseFromHealthResponse(503, 'Application/JSON; charset=utf-8')).toBe(
      'gateway-degraded',
    )
  })
})
