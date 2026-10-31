import { useCallback, useEffect, useState } from 'react'
import { ConnectionStatus, type ConnectionPhase } from '../ConnectionStatus'
import { Skeleton } from '../components/Skeleton'
import { phaseFromHealthResponse } from '../health'
import type { B24PlacementContext } from '../b24'
import { fetchConnections, type ConnectionInfo, type MarketplaceId } from '../api'
import { ConnectMaster } from './connect/ConnectMaster'
import { MARKETPLACE_LABELS } from './connect/step'
import { Orders } from './Orders'

/**
 * Экран воронки «Начните подключение» (Story 1.1 — существующий экран;
 * Story 1.2 — сюда попадает селлер, прошедший фильтр периметра;
 * Story 1.3 — статусы подключений площадок и вход в мастер token-vault).
 *
 * Честность UI (контракт эпика): осмысленное пустое состояние вместо
 * бутафорского CTA; статусы подключений — реальные ответы шлюза; мастер —
 * шаги 1–3 (площадка → токен → автотест прав).
 */
export function StartConnection({ placement }: { placement: B24PlacementContext }) {
  const [phase, setPhase] = useState<ConnectionPhase>('checking')
  const [connections, setConnections] = useState<ConnectionInfo[] | null>(null)
  const [connectionsError, setConnectionsError] = useState(false)
  const [masterOpen, setMasterOpen] = useState(false)

  const loadConnections = useCallback(() => {
    // Бюджет запроса — внутренний (requestJson, 8с): внешний контроллер
    // сюда не протянешь — сигнал не передаётся; от отмены при unmount /
    // повторной загрузке — флаг cancelled (ревью Story 1.3: мёртвый
    // AbortController вводил в заблуждение, а гонка ответов — нет).
    let cancelled = false
    fetchConnections(placement)
      .then((rows) => {
        if (!cancelled) {
          setConnections(rows)
          setConnectionsError(false)
        }
      })
      .catch(() => {
        if (!cancelled) setConnectionsError(true)
      })
    return () => {
      cancelled = true
    }
  }, [placement])

  useEffect(() => {
    let cancelled = false
    const controller = new AbortController()
    const timeout = setTimeout(() => controller.abort(), 1500)

    // Dev-удобство: пробуем /health шлюза через Vite-прокси (→ localhost:8080).
    // Честная семантика (health.ts): только JSON-ответ шлюза считается ответом
    // шлюза — 200+JSON → жив; 503+JSON → жив, но БД degraded; всё прочее
    // (в т.ч. HTML-фоллбек статик-хостинга с кодом 200) → шлюза нет.
    fetch('/health', { signal: controller.signal })
      .then((res) => {
        if (cancelled) return
        setPhase(phaseFromHealthResponse(res.status, res.headers.get('content-type')))
      })
      .catch(() => {
        if (!cancelled) setPhase('offline')
      })
      .finally(() => clearTimeout(timeout))

    return () => {
      cancelled = true
      controller.abort()
      clearTimeout(timeout)
    }
  }, [])

  useEffect(() => {
    // Статусы подключений опрашиваются только при живом шлюзе (их источник — /v1).
    if (phase !== 'gateway-up') return
    return loadConnections()
  }, [phase, loadConnections])

  const connectedMarketplaces: MarketplaceId[] = (connections ?? [])
    .map((row) => row.marketplace)
    .filter((id, index, all) => all.indexOf(id) === index)

  if (masterOpen) {
    return (
      <ConnectMaster
        ctx={placement}
        connected={connectedMarketplaces}
        onConnected={loadConnections}
        onBack={() => {
          setMasterOpen(false)
          loadConnections()
        }}
      />
    )
  }

  return (
    <section className="card" aria-labelledby="onboarding-title">
      <h1 id="onboarding-title">Начните подключение</h1>
      <p className="card__lead">
        Здесь появится пошаговое подключение вашего кабинета маркетплейса:
        фильтр «ваш бизнес» пройден, дальше — площадки, первичный синк истории
        с видимым прогрессом и первый живой канбан.
      </p>

      {phase === 'checking' ? <Skeleton lines={3} /> : <ConnectionStatus phase={phase} />}

      {phase === 'gateway-up' && (
        <div className="connections" aria-label="Статусы подключений площадок">
          <h2 className="connections__title">Площадки</h2>
          {connectionsError ? (
            <p className="card__note">
              Статусы подключений сейчас не читаются{connections ? " — показаны последние известные" : ""} — повторите позже.
            </p>
          ) : connections === null ? (
            <Skeleton lines={1} />
          ) : (
            <>
              <ul className="connections__list">
                {(Object.keys(MARKETPLACE_LABELS) as MarketplaceId[]).map((id) => {
                  const row = connections.find((c) => c.marketplace === id)
                  // Честность UI (формальное ревью 1.3): «подключена» — только
                  // verified; rejected/probe_unavailable — свои честные метки.
                  const stateText = !row
                    ? 'не подключена'
                    : row.status === 'verified'
                      ? `подключена (${row.schemes.join(', ')})`
                      : row.status === 'rejected'
                        ? 'права не подтверждены — повторите подключение'
                        : 'площадка не ответила — повторите позже'
                  return (
                    <li key={id} className="connections__row">
                      <span>{MARKETPLACE_LABELS[id]}</span>
                      <strong className={row?.status === 'verified' ? 'connections__ok' : 'connections__off'}>
                        {stateText}
                      </strong>
                    </li>
                  )
                })}
              </ul>
              <button type="button" className="card__action" onClick={() => setMasterOpen(true)}>
                {connections.length === 0
                  ? 'Подключить площадку (шаги 1–3)'
                  : 'Подключить ещё или обновить токен'}
              </button>
            </>
          )}
        </div>
      )}

      {/* Story 1.4: при verified-WB — прогресс синка и первый канбан (доска
          оживает по окнам истории); Ozon — честный копирайт до адаптера. */}
      {phase === 'gateway-up' && connections !== null && !connectionsError && (
        <Orders ctx={placement} connections={connections} />
      )}

      <dl className="context">
        <dt>Режим открытия</dt>
        <dd>
          {placement.embedded
            ? `iframe-placement Битрикс24${placement.placement ? ` (${placement.placement})` : ''}`
            : 'локальный dev-режим — после установки приложение откроется внутри Битрикс24'}
        </dd>
        {placement.lang && (
          <>
            <dt>Язык портала Б24</dt>
            <dd>{placement.lang}</dd>
          </>
        )}
      </dl>
    </section>
  )
}
