/**
 * Точка интеграции с Битрикс24 (Story 1.1 — каркас, не реализация).
 *
 * Приложение живёт в iframe-placement Битрикс24: параметры placement
 * передаются query-параметрами (PLACEMENT, PLACEMENT_OPTIONS, LANG, ...).
 * Аутентификация — только через контур Б24 (NFR-6): никаких отдельных
 * паролей и собственной сессии здесь нет и не будет. Реальная проверка
 * установки/подписи появится в Stories 1.2–1.3.
 */

export interface B24PlacementContext {
  /** true — приложение открыто как placement внутри Битрикс24. */
  embedded: boolean
  /** Код placement (например, placement-код виджета), если задан. */
  placement?: string
  /** Язык портала из параметров Б24. */
  lang?: string
  /** Сырые placement-опции Б24 (JSON-строка), для будущих сторис. */
  rawOptions?: string
}

/** Читает параметры iframe-placement Б24 из query-string текущего URL. */
export function readPlacementContext(
  search: string = window.location.search,
): B24PlacementContext {
  const params = new URLSearchParams(search)
  const placement = params.get('PLACEMENT') ?? undefined
  const lang = params.get('LANG') ?? undefined
  const rawOptions = params.get('PLACEMENT_OPTIONS') ?? undefined
  return {
    embedded: placement !== undefined && placement !== null && placement !== '',
    placement,
    lang,
    rawOptions,
  }
}
