/**
 * Честная заглушка статуса подключения (Story 1.1).
 *
 * Никаких «прикидок» живого подключения: каркас только сообщает реальное
 * состояние шлюза (dev) и честно пишет, что поток подключения появится
 * в Stories 1.2+. Внутри Б24 проверка установки будет выполняться
 * серверной стороной (Stories 1.2–1.3), а не этим компонентом.
 */

export type ConnectionPhase =
  /** Проба шлюза в полёте (в UI — скелетон, не спиннер). */
  | 'checking'
  /** /health шлюза ответил 200 — каркас запущен вместе с dev-стеком. */
  | 'gateway-up'
  /** /health ответил 503 + JSON — шлюз жив, но БД degraded (честно показываем). */
  | 'gateway-degraded'
  /** Шлюз недоступен — открыт чисто локальный dev-режим без бэкенда. */
  | 'offline'

const LABELS: Record<Exclude<ConnectionPhase, 'checking'>, { title: string; detail: string }> = {
  'gateway-up': {
    title: 'Шлюз запущен — каркас готов',
    detail:
      'Подключение кабинета появится в следующих сборках: фильтр «ваш бизнес» и мастер площадок (Stories 1.2–1.3).',
  },
  'gateway-degraded': {
    title: 'Шлюз запущен, но база недоступна',
    detail:
      'Каркас работает, инфраструктура поднята не полностью. Проверьте dev-стек: docker compose -f pult/deploy/docker-compose.yml up -d --wait',
  },
  offline: {
    title: 'Статус подключения пока не проверяется',
    detail:
      'Шлюз не отвечает из этого окружения — это честный статус каркаса. Проверка подключения и установка из Маркета появятся в Stories 1.2–1.3.',
  },
}

export function ConnectionStatus({ phase }: { phase: Exclude<ConnectionPhase, 'checking'> }) {
  const { title, detail } = LABELS[phase]
  return (
    <div className={`status status--${phase}`} role="status">
      <span className="status__dot" aria-hidden="true" />
      <div>
        <strong>{title}</strong>
        <p>{detail}</p>
      </div>
    </div>
  )
}
