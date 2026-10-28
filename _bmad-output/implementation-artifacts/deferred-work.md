# Deferred Work

Отложенные находки ревью и техдолг, не привязанный к спринту. Пополняется воркфлоуами
bmad-code-review / bmad-retrospective. Каждая запись обязана содержать: откуда, что,
почему отложено, кто закроет.

## Deferred from: code review of spec-1-1-structural-seed (2026-10-28)

- **updated_at без механизма обновления** (organizations/users/portal_settings, миграции 0001/0002/0004): колонки `updated_at` имеют `DEFAULT now()`, но ни триггера, ни задокументированной конвенции обновления нет. В Story 1.0 нет ни одного UPDATE-пути, поэтому дефект пока не проявляется; первая сторис, которая делает UPDATE этих таблиц (кандидаты: Story 1.6 — роли/portal_settings), обязана явно выбрать механизм: триггер `set_updated_at()` в миграции или `SET updated_at = now()` в SQL приложения — и покрыть тестом. Владелец закрытия: соответствующая сторис; контроль — ревью той сторис.

## Deferred from: bmad-build review of spec-1-2-perimeter-filter (2026-10-28)

- source_spec: `_bmad-output/implementation-artifacts/spec-1-2-perimeter-filter.md`
  summary: Неаутентифицированный write-on-GET (/v1/onboarding/state) создаёт строки-«установки» до хендшейка Б24
  evidence: Краулер/ prefetch/ опечатка в DOMAIN заводят постоянные org-строки и инфлируют знаменатель активации UJ-1; вердикты иммутабельны, пути очистки нет. Спека 1.2 фиксирует это как документированную аппроксимацию до хендшейка. Реальное закрытие: проверка подписи placement (Story 1.3) и/или rate-limit-менеджер (Story 2.3, AD-22).
- source_spec: `_bmad-output/implementation-artifacts/spec-1-2-perimeter-filter.md`
  summary: Нет HTTP-уровня Rust-тестов gateway (роутинг, Query/JsonRejection, идемпотентная гонка save_verdict)
  evidence: Юнит-тесты покрывают чистые функции; контракт живого шлюза проверяет только CI-curl (серверно) — инверсия веток хендлеров/Race-условия не ловятся локально. Закрытие: tower::ServiceExt::oneshot-тесты с мок-стором и/или sqlx::test при появлении PG в dev-цикле (кандидат — Epic 2 hardening).
- source_spec: `_bmad-output/implementation-artifacts/spec-1-2-perimeter-filter.md`
  summary: Вопросник шага 0 не предлагает «другую площадку» (список WB/Ozon/ЯМ закрыт и в UI, и в API)
  evidence: Селлер только на AliExpress/Мегамаркете вынужден мисселектить площадку; Ф0-периметр данных = WB/Ozon (ЯМ в списке UI), так что продукт всё равно не соберёт его данные. Продуктовое решение — за мастером подключения (Story 1.3): расширить список или добавить честную опцию «другая площадка» с объяснением границ Ф0.

## Deferred from: formal code review of spec-1-2-perimeter-filter (2026-10-29)

- source_spec: `_bmad-output/implementation-artifacts/spec-1-2-perimeter-filter.md`
  summary: Ноль React-компонентных/интеграционных тестов — вся SPA-сеть покрыта только чистыми функциями (api/appState/step0/b24)
  evidence: Submit-флоу Questionnaire, роутинг-эффект App и рендер экранов (ExitScreen/StartConnection) не тестируются — нужна инфраструктура RTL+jsdom (новые devDeps, настройка vitest environment). Осознанный паттерн 1.1–1.2 («логика — в чистые функции, компоненты — тонкие») компенсирует часть риска, но регрессии JSX/эффектов ловит только CI-e2e. Закрытие: при первом сложном компоненте (виджет 1.8) или в Epic 2 hardening — добавить @testing-library/react + jsdom иSmoke-тесты экранов. Владелец закрытия: соответствующая сторис/ретроспектива эпика 1.
