import { useEffect, useState } from 'react'
import { readPlacementContext } from './b24'
import { fetchOnboardingState } from './api'
import { screenForState, screenStateFromError, type ScreenState } from './appState'
import { Skeleton } from './components/Skeleton'
import { Questionnaire } from './screens/Questionnaire'
import { StartConnection } from './screens/StartConnection'
import { ExitScreen } from './screens/ExitScreen'

/**
 * Оболочка Б24 (Story 1.2): роутинг по состоянию шага 0 онбординга.
 *
 * Чистые функции {@link screenForState} и {@link screenStateFromError}
 * решают, какой экран показывать: step0_required → вопросник «ваш бизнес»,
 * funnel → экран «Начните подключение», exit_screen → исходящий экран
 * не-селлера, ошибки/офлайн → честная заглушка с кнопкой повтора.
 * Идентичность — только placement-контекст (NFR-6): без него шлюз применяет
 * документированный dev-фоллбэк.
 */
export default function App() {
  const placement = readPlacementContext()
  const [screenState, setScreenState] = useState<ScreenState>({ kind: 'loading' })
  const [attempt, setAttempt] = useState(0)

  useEffect(() => {
    let cancelled = false
    fetchOnboardingState(placement)
      .then((state) => {
        if (!cancelled) setScreenState({ kind: 'resolved', state })
      })
      .catch((err: unknown) => {
        if (!cancelled) setScreenState(screenStateFromError(err))
      })
    return () => {
      cancelled = true
    }
    // attempt — осознанный повтор запроса пользователем (кнопка «Повторить»).
  }, [attempt])

  const retry = () => {
    setScreenState({ kind: 'loading' })
    setAttempt((a) => a + 1)
  }

  const screen = screenForState(screenState)

  return (
    <main className="screen">
      <header className="screen__header">
        <span className="brand">Пульт селлера</span>
        <span className="brand-note">операционная система селлера маркетплейсов</span>
      </header>

      {screen === 'skeleton' && (
        <section className="card" aria-busy="true" aria-labelledby="loading-title">
          <h1 id="loading-title">Определяем ваш шаг онбординга</h1>
          <Skeleton lines={3} />
        </section>
      )}

      {screen === 'questionnaire' && (
        <Questionnaire
          ctx={placement}
          onResolved={(state) => setScreenState({ kind: 'resolved', state })}
        />
      )}

      {screen === 'funnel' && <StartConnection placement={placement} />}

      {screen === 'exit' && <ExitScreen />}

      {screen === 'unavailable' && <Unavailable state={screenState} onRetry={retry} />}

      <footer className="screen__footer">
        Вход и роли — только через контур Битрикс24, отдельных паролей нет.
      </footer>
    </main>
  )
}

/** Честная заглушка с повтором: шлюз недоступен или ответил ошибкой. */
function Unavailable({
  state,
  onRetry,
}: {
  state: ScreenState
  onRetry: () => void
}) {
  const offline = state.kind === 'offline'
  const error = state.kind === 'error' ? state : null
  return (
    <section className="card" aria-labelledby="unavailable-title">
      <h1 id="unavailable-title">Онбординг сейчас недоступен</h1>
      <p className="card__lead">
        {offline
          ? 'Шлюз не отвечает из этого окружения — это честный статус. Фильтр «ваш бизнес» откроется, когда шлюз поднят (локально: dev-стек + pult-gateway).'
          : `Шлюз ответил ошибкой: ${error?.message ?? 'неизвестная ошибка'} (код: ${error?.code ?? 'unknown'}).`}
      </p>
      <button type="button" className="card__action" onClick={onRetry}>
        Повторить
      </button>
    </section>
  )
}
