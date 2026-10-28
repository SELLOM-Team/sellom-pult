import { useState } from 'react'
import type { B24PlacementContext } from '../b24'
import { ApiError, submitVerdict, type TradeModel } from '../api'
import { canSubmitStep0, needsMarketplaces } from '../step0'

const TRADE_MODELS: { value: TradeModel; label: string; hint?: string }[] = [
  {
    value: 'marketplace_seller',
    label: 'Продаю на маркетплейсах',
    hint: 'Wildberries, Ozon и другие площадки',
  },
  {
    value: 'marketplace_planned',
    label: 'Планирую выйти на маркетплейсы',
  },
  {
    value: 'own_channels_no_marketplaces',
    label: 'Продаю только через свои каналы',
    hint: 'Свой сайт или соцсети, маркетплейсы не планирую',
  },
  {
    value: 'services_other',
    label: 'Услуги или другой бизнес',
  },
]

const MARKETPLACES: { id: string; label: string }[] = [
  { id: 'wildberries', label: 'Wildberries' },
  { id: 'ozon', label: 'Ozon' },
  { id: 'yandex_market', label: 'Яндекс Маркет' },
]

interface QuestionnaireProps {
  ctx: B24PlacementContext
  onResolved: (state: 'funnel' | 'exit_screen') => void
}

/**
 * Шаг 0 — фильтр периметра «ваш бизнес» (Story 1.2, FR-1).
 *
 * Два вопроса, честный копирайт, ноль PII (не спрашиваем имя, телефон,
 * почту — только модель торговли и площадки для селлеров). Ответ уходит
 * на шлюз и запоминается per-user: повторно вопросник не показывается.
 */
export function Questionnaire({ ctx, onResolved }: QuestionnaireProps) {
  const [tradeModel, setTradeModel] = useState<TradeModel | null>(null)
  const [marketplaces, setMarketplaces] = useState<string[]>([])
  const [submitting, setSubmitting] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const askMarketplaces = needsMarketplaces(tradeModel)
  const canSubmit = canSubmitStep0(tradeModel, marketplaces, submitting)

  const toggleMarketplace = (id: string) => {
    setMarketplaces((prev) =>
      prev.includes(id) ? prev.filter((m) => m !== id) : [...prev, id],
    )
  }

  const submit = async () => {
    if (!canSubmit || tradeModel === null) return
    setSubmitting(true)
    setError(null)
    try {
      const { state } = await submitVerdict(ctx, {
        trade_model: tradeModel,
        // Для не-селлерских моделей площадки не собираются (минимум данных).
        marketplaces: askMarketplaces ? marketplaces : [],
      })
      onResolved(state)
    } catch (err) {
      setError(
        err instanceof ApiError
          ? err.message
          : 'Не удалось отправить ответ — попробуйте ещё раз.',
      )
      setSubmitting(false)
    }
  }

  return (
    <section className="card" aria-labelledby="step0-title">
      <h1 id="step0-title">Ваш бизнес</h1>
      <p className="card__lead">
        Два коротких вопроса, чтобы честно сказать, подойдёт ли вам «Пульт
        селлера». Мы не спрашиваем имя, телефон или почту — только модель
        торговли. Ответ запоминается: повторно этот шаг не появится.
      </p>

      <fieldset className="q-group" disabled={submitting}>
        <legend className="q-group__legend">Как вы сейчас торгуете?</legend>
        {TRADE_MODELS.map((option) => (
          <label
            key={option.value}
            className={`q-option${tradeModel === option.value ? ' q-option--checked' : ''}`}
          >
            <input
              type="radio"
              name="trade-model"
              value={option.value}
              checked={tradeModel === option.value}
              onChange={() => setTradeModel(option.value)}
            />
            <span>
              {option.label}
              {option.hint && <em className="q-option__hint">{option.hint}</em>}
            </span>
          </label>
        ))}
      </fieldset>

      {askMarketplaces && (
        <fieldset className="q-group" disabled={submitting}>
          <legend className="q-group__legend">
            На каких площадках продаёте или планируете?
          </legend>
          {MARKETPLACES.map((m) => (
            <label
              key={m.id}
              className={`q-option${marketplaces.includes(m.id) ? ' q-option--checked' : ''}`}
            >
              <input
                type="checkbox"
                checked={marketplaces.includes(m.id)}
                onChange={() => toggleMarketplace(m.id)}
              />
              <span>{m.label}</span>
            </label>
          ))}
        </fieldset>
      )}

      {error && (
        <p className="form-error" role="alert">
          {error}
        </p>
      )}

      <button type="button" className="card__action" onClick={submit} disabled={!canSubmit}>
        {submitting ? 'Отправляем…' : 'Ответить и продолжить'}
      </button>
    </section>
  )
}
