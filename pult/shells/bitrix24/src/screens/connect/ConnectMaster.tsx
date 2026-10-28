import { useState } from 'react'
import type { B24PlacementContext } from '../../b24'
import { connectMarketplace, type ConnectOutcome, type MarketplaceId } from '../../api'
import {
  MARKETPLACE_LABELS,
  OWNER_ZONE_NOTE,
  SCHEMES,
  canSubmitConnect,
  validateConnectForm,
  wizardStep,
} from './step'

/**
 * Мастер подключения площадки (Story 1.3, шаги 1–3; FR-2, AD-16).
 *
 * Шаг 1 — выбор площадки (подключённая = режим ротации токена); шаг 2 —
 * токен (WB) или Client-Id + Api-Key (Ozon) и схемы продаж; шаг 3 — результат
 * автотеста прав: таблица прав при успехе, человекочитаемая диагностика при
 * отказе, честный статус при недоступности площадки. Токен уходит на шлюз
 * (сервер шифрует его в token-vault) и нигде не отображается и не хранится
 * на клиенте. Из шага 1 всегда можно вернуться к статусам (onBack).
 *
 * Прогон автотеста — до 2 × 10 с проб (таймауты площадок, бюджет клиента
 * 30 с) — кнопка переходит в состояние «Проверяем права…» и блокируется.
 */
export function ConnectMaster({
  ctx,
  connected,
  onConnected,
  onBack,
}: {
  ctx: B24PlacementContext
  connected: MarketplaceId[]
  onConnected: () => void
  onBack: () => void
}) {
  const [marketplace, setMarketplace] = useState<MarketplaceId | null>(null)
  const [schemes, setSchemes] = useState<string[]>([])
  const [token, setToken] = useState('')
  const [clientId, setClientId] = useState('')
  const [submitting, setSubmitting] = useState(false)
  const [outcome, setOutcome] = useState<ConnectOutcome | null>(null)
  const [apiError, setApiError] = useState<string | null>(null)

  const step = wizardStep(marketplace, outcome)
  const errors = validateConnectForm(
    marketplace ?? 'wildberries',
    token,
    clientId,
    schemes,
  )
  const canSubmit = canSubmitConnect(errors, submitting)

  const choose = (id: MarketplaceId) => {
    // Подключённая площадка тоже выбирается — это ротация токена (спека 1.3:
    // повторный POST перезаписывает; ревью: кнопка «обновить токен» была
    // недостижима при disabled-выборе).
    setMarketplace(id)
    setSchemes([])
    setToken('')
    setClientId('')
    setOutcome(null)
    setApiError(null)
  }

  const toggleScheme = (id: string) => {
    setSchemes((prev) => (prev.includes(id) ? prev.filter((s) => s !== id) : [...prev, id]))
  }

  const submit = async () => {
    if (marketplace === null || !canSubmit) return
    setSubmitting(true)
    setApiError(null)
    try {
      const result = await connectMarketplace(ctx, {
        marketplace,
        schemes,
        token: token.trim(),
        client_id: marketplace === 'ozon' ? clientId.trim() : undefined,
      })
      setOutcome(result)
      if (result.status === 'verified') onConnected()
    } catch (err) {
      setApiError(err instanceof Error ? err.message : 'Не удалось выполнить автотест прав')
    } finally {
      setSubmitting(false)
    }
  }

  const stepTitle = STEP_TITLES[step]

  return (
    <section className="card" aria-labelledby="connect-title">
      <h1 id="connect-title">Подключение площадки</h1>
      <p className="card__lead">{stepTitle}</p>

      {step === 1 && (
        <>
          <div className="connect-choices" role="radiogroup" aria-label="Площадка">
            {(Object.keys(MARKETPLACE_LABELS) as MarketplaceId[]).map((id) => {
              const isBusy = connected.includes(id)
              return (
                <button
                  key={id}
                  type="button"
                  className="q-option connect-choice"
                  onClick={() => choose(id)}
                  aria-label={MARKETPLACE_LABELS[id]}
                >
                  <span>{MARKETPLACE_LABELS[id]}</span>
                  <em className="q-option__hint">
                    {isBusy ? 'уже подключена — обновить токен' : 'подключить токен API'}
                  </em>
                </button>
              )
            })}
          </div>
          <button type="button" className="card__link" onClick={onBack}>
            Вернуться к статусам
          </button>
        </>
      )}

      {step === 2 && marketplace !== null && (
        <>
          <p className="connect-marketplace">
            {MARKETPLACE_LABELS[marketplace]}
            {connected.includes(marketplace) && ' — обновление токена'}
          </p>
          {marketplace === 'ozon' && (
            <label className="connect-field">
              <span>Client-Id кабинета</span>
              <input
                type="text"
                value={clientId}
                onChange={(e) => setClientId(e.target.value)}
                autoComplete="off"
                placeholder="например, 12345"
              />
            </label>
          )}
          <label className="connect-field">
            <span>{marketplace === 'ozon' ? 'Api-Key' : 'API-токен'}</span>
            <textarea
              value={token}
              onChange={(e) => setToken(e.target.value)}
              rows={3}
              autoComplete="off"
              placeholder="вставьте ключ целиком — он шифруется и больше нигде не показывается"
            />
          </label>
          <fieldset className="q-group">
            <legend className="q-group__legend">Схемы продаж</legend>
            {SCHEMES[marketplace].map((scheme) => (
              <label
                key={scheme.id}
                className={`q-option${schemes.includes(scheme.id) ? ' q-option--checked' : ''}`}
              >
                <input
                  type="checkbox"
                  checked={schemes.includes(scheme.id)}
                  onChange={() => toggleScheme(scheme.id)}
                />
                <span>{scheme.label}</span>
              </label>
            ))}
          </fieldset>
          {(errors.token || errors.clientId || errors.schemes) && (
            <p className="form-error" role="alert">
              {errors.token || errors.clientId || errors.schemes}
            </p>
          )}
          <p className="card__note">{OWNER_ZONE_NOTE}</p>
          <div className="connect-actions">
            <button type="button" className="card__action" onClick={submit} disabled={!canSubmit}>
              {submitting ? 'Проверяем права…' : 'Проверить права и подключить'}
            </button>
            <button
              type="button"
              className="card__link"
              onClick={() => {
                setMarketplace(null)
                setOutcome(null)
              }}
            >
              Назад
            </button>
          </div>
        </>
      )}

      {step === 3 && outcome !== null && (
        <ConnectOutcomeView
          outcome={outcome}
          onRetry={() => setOutcome(null)}
          onDone={onBack}
        />
      )}

      {apiError && (
        <p className="form-error" role="alert">
          {apiError}
        </p>
      )}
    </section>
  )
}

const STEP_TITLES: Record<1 | 2 | 3, string> = {
  1: 'Какую площадку подключаем? Каждая подключается своим токеном, обе живут в одном портале.',
  2: 'Вставьте ключ API площадки и выберите схемы продаж — мы проверим права автотестом, прежде чем сохранить токен.',
  3: 'Результат автотеста прав.',
}

/** Шаг 3: таблица прав (ok) или честная диагностика (rejected/probe_unavailable). */
function ConnectOutcomeView({
  outcome,
  onRetry,
  onDone,
}: {
  outcome: ConnectOutcome
  onRetry: () => void
  onDone: () => void
}) {
  if (outcome.status === 'verified') {
    return (
      <>
        <p className="connect-ok" role="status">
          Права подтверждены — токен зашифрован и сохранён. Сбор данных начнётся после первичного
          синка (следующий шаг онбординга).
        </p>
        <ul className="rights-list">
          {outcome.rights.map((check) => (
            <li key={check.name} className="rights-list__item rights-list__item--ok">
              {check.name}: подтверждено
            </li>
          ))}
        </ul>
        <button type="button" className="card__action" onClick={onDone}>
          Готово
        </button>
      </>
    )
  }
  return (
    <>
      <p className="form-error" role="alert">
        {outcome.status === 'rejected'
          ? 'Токен не принят: часть прав не подтвердилась — исправьте и попробуйте снова (прежнее подключение не тронуто).'
          : 'Площадка не ответила на проверку прав — подключение не выполнено, повторите позже.'}
      </p>
      <ul className="rights-list">
        {outcome.checks.map((check) => (
          <li
            key={check.name}
            className={`rights-list__item${check.ok ? ' rights-list__item--ok' : ' rights-list__item--fail'}`}
          >
            {check.name}: {check.ok ? 'подтверждено' : (check.hint ?? 'не подтвердилось')}
          </li>
        ))}
      </ul>
      <button type="button" className="card__action" onClick={onRetry}>
        Вернуться к форме
      </button>
    </>
  )
}
