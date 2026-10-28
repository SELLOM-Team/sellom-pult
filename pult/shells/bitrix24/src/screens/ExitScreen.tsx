const DEFAULT_PARTNER_TITLE = 'Каталог партнёров Битрикс24.Маркет'
const DEFAULT_PARTNER_URL = 'https://www.bitrix24.ru/apps/'

export interface PartnerEnv {
  readonly VITE_PARTNER_TITLE?: string
  readonly VITE_PARTNER_URL?: string
}

/**
 * Контент-слот партнёрской рекомендации (Story 1.2): конкретного партнёра
 * выбирает владелец при публикации через VITE_PARTNER_TITLE/VITE_PARTNER_URL
 * (критерий рев-шер ≥ 30% — бизнес-условие, делегированное владельцем).
 * Честный дефолт — публичный каталог партнёров Битрикс24.Маркет; ссылка
 * принимается только http/https (иначе дефолт — без тихих подмен).
 */
export function partnerSlot(env: PartnerEnv = import.meta.env): {
  title: string
  url: string
} {
  const title = env.VITE_PARTNER_TITLE?.trim() || DEFAULT_PARTNER_TITLE
  const rawUrl = env.VITE_PARTNER_URL?.trim()
  const url =
    rawUrl !== undefined && /^https?:\/\//i.test(rawUrl) ? rawUrl : DEFAULT_PARTNER_URL
  return { title, url }
}

/**
 * Терминальный исходящий экран не-селлера (Story 1.2, FR-1): честный отказ
 * «продукт не подходит» + периметр продукта + рекомендация партнёрского
 * решения + честное приглашение вернуться. В воронку не-селлер не попадает.
 */
export function ExitScreen() {
  const partner = partnerSlot()
  return (
    <section className="card" aria-labelledby="exit-title">
      <h1 id="exit-title">Продукт пока не подойдёт</h1>
      <p className="card__lead">
        «Пульт селлера» — операционная система для тех, кто продаёт или
        готовится продавать на маркетплейсах (Wildberries, Ozon). По вашим
        ответам продукт сейчас не решит ваших задач — мы честно говорим об
        этом и не ведём вас в онбординг.
      </p>
      <div className="partner">
        <p>Возможно, для ваших задач подойдёт партнёрское решение:</p>
        <a href={partner.url} target="_blank" rel="noreferrer noopener">
          {partner.title}
        </a>
      </div>
      <p className="card__lead">
        Если ваш бизнес изменится и вы начнёте торговать на маркетплейсах —
        возвращайтесь: ваш ответ сохранён и вопросник не повторится, а когда
        периметр продукта расширится, вход в онбординг откроется.
      </p>
    </section>
  )
}
