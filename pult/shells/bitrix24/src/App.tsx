import { useEffect, useState } from 'react'
import { readPlacementContext } from './b24'
import { ApiError, fetchOnboardingState } from './api'
import { screenForState, type ScreenState } from './appState'
import { Skeleton } from './components/Skeleton'
import { Questionnaire } from './screens/Questionnaire'
import { StartConnection } from './screens/StartConnection'
import { ExitScreen } from './screens/ExitScreen'

/**
 * Оболочка Б24 (Story 1.2): роутинг по состоянию шага 0 онбординга.
 *
 * Чистая функция {@link screenForState} решает, какой экран показывать:
 * step0_required → вопросник «ваш бизнес», funnel → существующий экран
 * «Начните подключение», exit_screen → исходящий экран не-селлера,
 * ошибки/офлайн → честная заглушка. Идентичность — только placement-контекст
 * (NFR-6): без него шлюз применяет документированный dev-фоллбэк.
 */
export default function App() {
  const placement = readPlacementContext()
  const [screenState, setScreenState] = useState<ScreenState>({ kind: 'loading' })

  useEffect(() => {
    let cancelled = false
    fetchOnboardingState(placement)
      .then((state) => {
        if (!cancelled) setScreenState({ kind: 'resolved', state })
      })
      .catch((err: unknown) => {
        if (cancelled) return
        if (err instanceof ApiError && err.code !== 'network_error' && err.code !== 'invalid_response') {
          // Шлюз ответил ошибкой-конвертом (например, 503 database_unavailable).
          setScreenState({ kind: 'error', code: err.code, message: err.message })
        } else {
          // Сеть/не-JSON: шлюза нет — честный офлайн-статус.
          setScreenState({ kind: 'offline' })
        }
      })
    return () => {
      cancelled = true
    }
  }, [])

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

      {screen === 'unavailable' && <Unavailable state={screenState} />}

      <footer className="screen__footer">
        Вход и роли — только через контур Битрикс24, отдельных паролей нет.
      </footer>
    </main>
  )
}

/** Честная заглушка: шлюз недоступен или ответил ошибкой — без бутафории. */
function Unavailable({ state }: { state: ScreenState }) {
  const offline = state.kind === 'offline'
  const error = state.kind === 'error' ? state : null
  return (
    <section className="card" aria-labelledby="unavailable-title">
      <h1 id="unavailable-title">Онбординг сейчас недоступен</h1>
      <p className="card__lead">
        {offline
          ? 'Шлюз не отвечает из этого окружения — это честный статус. Фильтр «ваш бизнес» откроется, когда шлюз поднят (локально: dev-стек + pult-gateway).'
          : `Шлюз ответил ошибкой: ${error?.message ?? 'неизвестная ошибка'} (код: ${error?.code ?? 'unknown'}). Повторите попытку позже.`}
      </p>
    </section>
  )
}
