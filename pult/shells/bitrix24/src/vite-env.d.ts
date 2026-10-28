/// <reference types="vite/client" />

interface ImportMetaEnv {
  /**
   * Слот партнёрской рекомендации на исходящем экране не-селлера
   * (Story 1.2): конкретного партнёра выбирает владелец при публикации
   * (рев-шер ≥ 30%); дефолт — каталог партнёров Битрикс24.Маркет.
   */
  readonly VITE_PARTNER_TITLE?: string
  /** Ссылка слота; принимаются только http/https — иначе честный дефолт. */
  readonly VITE_PARTNER_URL?: string
}
