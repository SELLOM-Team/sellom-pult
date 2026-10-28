import { useEffect, useState } from 'react'
import { ConnectionStatus, type ConnectionPhase } from './ConnectionStatus'
import { Skeleton } from './components/Skeleton'
import { readPlacementContext } from './b24'

/**
 * Экран входа каркаса: «Начните подключение» (Story 1.1).
 *
 * Честность UI (контракт эпика): осмысленное пустое состояние вместо
 * бутафорского CTA — подключение площадок и фильтр периметра появятся
 * в Stories 1.2–1.4, статус подключения — честная заглушка.
 */
export default function App() {
  const placement = readPlacementContext()
  const [phase, setPhase] = useState<ConnectionPhase>('checking')

  useEffect(() => {
    let cancelled = false
    const controller = new AbortController()
    const timeout = setTimeout(() => controller.abort(), 1500)

    // Dev-удобство: пробуем /health шлюза через Vite-прокси (→ localhost:8080).
    // Честная семантика: 200 → шлюз жив; 503 → шлюз жив, но БД degraded;
    // любой другой код (в т.ч. SPA-fallback 404 без прокси) → шлюза нет.
    fetch('/health', { signal: controller.signal })
      .then((res) => {
        if (cancelled) return
        if (res.status === 200) setPhase('gateway-up')
        else if (res.status === 503) setPhase('gateway-degraded')
        else setPhase('offline')
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

  return (
    <main className="screen">
      <header className="screen__header">
        <span className="brand">Пульт селлера</span>
        <span className="brand-note">операционная система селлера маркетплейсов</span>
      </header>

      <section className="card" aria-labelledby="onboarding-title">
        <h1 id="onboarding-title">Начните подключение</h1>
        <p className="card__lead">
          Здесь появится пошаговое подключение вашего кабинета маркетплейса:
          фильтр «ваш бизнес», площадки, первичный синк истории с видимым
          прогрессом и первый живой канбан.
        </p>

        {phase === 'checking' ? (
          <Skeleton lines={3} />
        ) : (
          <ConnectionStatus phase={phase} />
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

      <footer className="screen__footer">
        Вход и роли — только через контур Битрикс24, отдельных паролей нет.
      </footer>
    </main>
  )
}
