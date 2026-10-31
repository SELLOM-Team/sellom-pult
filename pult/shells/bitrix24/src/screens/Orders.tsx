import { useEffect, useMemo, useRef, useState } from 'react'
import {
  fetchBoardOrders,
  fetchSyncProgress,
  retrySync,
  type BoardData,
  type ConnectionInfo,
  type SyncProgress,
} from '../api'
import type { B24PlacementContext } from '../b24'
import { SyncProgressView } from './sync/SyncProgressView'
import { BoardView } from './board/BoardView'
import { ACTIVE_SYNC_STATUSES, classifyProgressError } from './sync/progress'

/**
 * Секция «Заказы» (Story 1.4): прогресс синка + первый канбан 7 колонок.
 *
 * Появляется при verified-подключении WB. Весь опрос — здесь (единственный
 * владелец таймеров): прогресс — каждые 5с, пока цикл активен; доска — при
 * изменении состояния синка и каждые 10с активного цикла («доска живёт с
 * первого окна»), после succeeded — финальная перезагрузка и стоп.
 * Ozon подключён → честный копирайт «данные после завершения адаптера».
 */
export function Orders({
  ctx,
  connections,
}: {
  ctx: B24PlacementContext
  connections: ConnectionInfo[]
}) {
  const [progress, setProgress] = useState<SyncProgress | null>(null)
  const [progressError, setProgressError] = useState<ReturnType<
    typeof classifyProgressError
  > | null>(null)
  const [board, setBoard] = useState<{ data: BoardData | null; error: unknown }>({
    data: null,
    error: null,
  })
  const [boardLoading, setBoardLoading] = useState(true)
  const [boardTick, setBoardTick] = useState(0)
  const [pollTick, setPollTick] = useState(0)
  const [retrying, setRetrying] = useState(false)

  const wbVerified = connections.some(
    (row) => row.marketplace === 'wildberries' && row.status === 'verified',
  )
  const ozonVerified = connections.some(
    (row) => row.marketplace === 'ozon' && row.status === 'verified',
  )

  // Цикл активен (или ещё не прочитан) → опрос продолжается.
  const active = useMemo(
    () => progress === null || ACTIVE_SYNC_STATUSES.includes(progress.status),
    [progress],
  )

  // Опрос прогресса: сразу + каждые 5с, пока активен (после ошибки — тоже:
  // progress остаётся null/старым, опрос честно продолжается).
  useEffect(() => {
    if (!wbVerified) return
    let cancelled = false
    const load = () => {
      fetchSyncProgress(ctx)
        .then((p) => {
          if (!cancelled) {
            setProgress(p)
            setProgressError(null)
          }
        })
        .catch((err: unknown) => {
          if (!cancelled) setProgressError(classifyProgressError(err))
        })
    }
    load()
    if (!active) return () => { cancelled = true }
    const timer = setInterval(load, 5000)
    return () => {
      cancelled = true
      clearInterval(timer)
    }
  }, [ctx, wbVerified, active, pollTick])

  // Терминальный переход (active true→false): финальная перезагрузка доски.
  const prevActive = useRef<boolean | null>(null)
  useEffect(() => {
    if (prevActive.current === true && !active) setBoardTick((t) => t + 1)
    prevActive.current = active
  }, [active])

  // Активный цикл: доска оживает каждые 10с (заказы приходят по окнам).
  useEffect(() => {
    if (!active) return
    const timer = setInterval(() => setBoardTick((t) => t + 1), 10000)
    return () => clearInterval(timer)
  }, [active])

  // Загрузка доски (mount + каждый тик).
  useEffect(() => {
    if (!wbVerified) return
    let cancelled = false
    setBoardLoading(true)
    fetchBoardOrders(ctx)
      .then((data) => {
        if (!cancelled) setBoard({ data, error: null })
      })
      .catch((error: unknown) => {
        if (!cancelled) setBoard({ data: null, error })
      })
      .finally(() => {
        if (!cancelled) setBoardLoading(false)
      })
    return () => {
      cancelled = true
    }
  }, [ctx, wbVerified, boardTick])

  // Повтор терминального failed: сброс на шлюзе (идемпотентен), опрос
  // подхватит pending и продолжит сам.
  const retry = async () => {
    setRetrying(true)
    try {
      const p = await retrySync(ctx)
      setProgress(p)
      setProgressError(null)
    } catch (err: unknown) {
      setProgressError(classifyProgressError(err))
    } finally {
      setRetrying(false)
      setBoardTick((t) => t + 1)
    }
  }

  if (!wbVerified) return null

  return (
    <div className="orders" aria-label="Заказы маркетплейсов">
      <h2 className="connections__title">Заказы</h2>
      <SyncProgressView
        progress={progress}
        error={progressError}
        retrying={retrying}
        onRetry={retry}
        onPollRetry={() => setPollTick((t) => t + 1)}
      />
      <BoardView
        data={board.data}
        error={board.error}
        loading={boardLoading}
        onRefresh={() => setBoardTick((t) => t + 1)}
      />
      {ozonVerified && (
        <p className="orders__ozon">
          Ozon подключён — данные появятся после завершения адаптера (в
          разработке, этап 2).
        </p>
      )}
    </div>
  )
}
