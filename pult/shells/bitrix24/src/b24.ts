/**
 * Точка интеграции с Битрикс24 (Story 1.1 — каркас; Story 1.2 — identity).
 *
 * Приложение живёт в iframe-placement Битрикс24: параметры placement
 * передаются query-параметрами (PLACEMENT, DOMAIN, USER_ID, LANG, ...).
 * Идентичность — только из этого контекста: DOMAIN (портал) и USER_ID
 * (пользователь портала) — внешние ключи, не PII; реальная проверка
 * подписи/хендшейка — Story 1.3. Никакой собственной аутентификации и
 * паролей (NFR-6) — шаг 0 не собирает имена, телефоны, почты.
 */

export interface B24PlacementContext {
  /** true — приложение открыто как placement внутри Битрикс24. */
  embedded: boolean
  /** Код placement (например, placement-код виджета), если задан. */
  placement?: string
  /** Домен портала Б24 (DOMAIN) — идентификатор организации, не PII. */
  domain?: string
  /** ID пользователя портала (USER_ID) — внешний ref, не PII. */
  userId?: string
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
  const domain = params.get('DOMAIN') ?? undefined
  const userId = params.get('USER_ID') ?? undefined
  const lang = params.get('LANG') ?? undefined
  const rawOptions = params.get('PLACEMENT_OPTIONS') ?? undefined
  return {
    // placement здесь string | undefined (get() ?? undefined): null недостижим.
    embedded: placement !== undefined && placement !== '',
    placement,
    domain,
    userId,
    lang,
    rawOptions,
  }
}
