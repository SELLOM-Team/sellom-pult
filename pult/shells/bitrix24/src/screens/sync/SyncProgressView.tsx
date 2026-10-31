import { Skeleton } from '../../components/Skeleton'
import type { SyncProgress } from '../../api'
import {
  progressView,
  type ProgressErrorKind,
} from './progress'

/**
 * Виджет прогресса синка (Story 1.4) — «глупый» компонент: опрос живёт
 * в родителе ({@link ../Orders}), здесь только честный рендер состояния.
 *
 * failed → кнопка «Повторить синк» (терминальный сбой решает человек);
 * ошибки опроса — диагностикой, не «зелёным» прогрессом.
 */
export function SyncProgressView({
  progress,
  error,
  retrying,
  onRetry,
  onPollRetry,
}: {
  progress: SyncProgress | null
  error: ProgressErrorKind | null
  retrying: boolean
  onRetry: () => void
  onPollRetry: () => void
}) {
  if (error !== null && progress === null) {
    return (
      <div className="progress progress--failed" aria-live="polite">
        <p className="progress__title">
          {error === 'network'
            ? 'Прогресс синка недоступен: шлюз не отвечает из этого окружения.'
            : 'Прогресс синка временно недоступен — шлюз ответил ошибкой.'}
        </p>
        <button type="button" className="card__action" onClick={onPollRetry}>
          Повторить опрос
        </button>
      </div>
    )
  }

  if (progress === null) {
    return (
      <div className="progress" aria-busy="true">
        <p className="progress__title">Определяем состояние загрузки истории</p>
        <Skeleton lines={1} />
      </div>
    )
  }

  const view = progressView(progress)
  return (
    <div className={`progress progress--${view.tone}`} aria-live="polite">
      <p className="progress__title">{view.title}</p>
      <div
        className="progress__bar"
        role="progressbar"
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={view.percent}
        aria-label="Прогресс загрузки истории заказов"
      >
        <div className="progress__fill" style={{ width: `${view.percent}%` }} />
      </div>
      <p className="progress__note">
        {view.percent}% · {view.note}
      </p>
      {view.retry && (
        <button
          type="button"
          className="card__action"
          onClick={onRetry}
          disabled={retrying}
        >
          {retrying ? 'Отправляем команду…' : 'Повторить синк'}
        </button>
      )}
    </div>
  )
}
