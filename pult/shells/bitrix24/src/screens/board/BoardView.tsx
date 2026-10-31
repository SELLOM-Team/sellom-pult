import type { BoardData } from '../../api'
import { Skeleton } from '../../components/Skeleton'
import {
  BOARD_COLUMNS,
  boardViewState,
  cardHeadline,
  formatKopecks,
  hiddenCount,
  itemLine,
} from './model'

/**
 * Доска заказов (Story 1.4, FR-16) — 7 колонок без фильтров, карточки с
 * позициями и суммами. «Глупый» компонент: загрузка/обновление — в родителе
 * ({@link ../Orders}), здесь честный рендер состояния (model.boardViewState).
 */
export function BoardView({
  data,
  error,
  loading,
  onRefresh,
}: {
  data: BoardData | null
  error: unknown
  loading: boolean
  onRefresh: () => void
}) {
  const state = boardViewState(error, data)

  return (
    <div className="board" aria-label="Доска заказов">
      <div className="board__head">
        <h3 className="board__title">Канбан заказов</h3>
        <button type="button" className="card__link" onClick={onRefresh}>
          {loading ? 'Обновляем…' : 'Обновить'}
        </button>
      </div>

      {state.kind === 'loading' && (
        <div aria-busy="true">
          <p className="board__note">Загружаем доску…</p>
          <Skeleton lines={2} />
        </div>
      )}

      {state.kind === 'syncing' && (
        <p className="board__note">
          Заказы загружаются — доска оживёт с первыми подтверждёнными окнами
          истории.
        </p>
      )}

      {state.kind === 'empty' && (
        <p className="board__note">
          Заказов пока нет: история загружена, новых заказов в кабинете не
          найдено.
        </p>
      )}

      {state.kind === 'failed' && (
        <p className="board__note board__note--failed">
          Доска недоступна: синк не удался. {state.hint}
        </p>
      )}

      {state.kind === 'offline' && (
        <p className="board__note">Доска сейчас не читается — шлюз не отвечает.</p>
      )}

      {state.kind === 'error' && (
        <p className="board__note">Доска недоступна: {state.message}</p>
      )}

      {state.kind === 'board' && (
        <div className="board__grid">
          {BOARD_COLUMNS.map(({ key, title }) => {
            const cards = state.data.columns[key]
            const hidden = hiddenCount(state.data, key)
            return (
              <section className="board__column" key={key} aria-label={title}>
                <h4 className="board__column-title">
                  {title}
                  <span className="board__count">{state.data.totals[key]}</span>
                </h4>
                {cards.map((card) => (
                  <article
                    className={`board__card${card.cancelled ? ' board__card--cancelled' : ''}`}
                    key={card.id}
                  >
                    <p className="board__card-head">
                      {cardHeadline(card)}
                      {card.cancelled && <span className="board__tag">отменён</span>}
                    </p>
                    <p className="board__card-sum">
                      {formatKopecks(card.total_kopecks)}
                    </p>
                    {card.items.slice(0, 3).map((item, index) => (
                      <p className="board__card-item" key={index}>
                        {itemLine(item)}
                      </p>
                    ))}
                    {card.items.length > 3 && (
                      <p className="board__card-item">и ещё {card.items.length - 3}</p>
                    )}
                    {card.warehouse && (
                      <p className="board__card-meta">{card.warehouse}</p>
                    )}
                  </article>
                ))}
                {hidden > 0 && (
                  <p className="board__hidden">показаны первые {cards.length} из {state.data.totals[key]}</p>
                )}
              </section>
            )
          })}
        </div>
      )}
    </div>
  )
}
