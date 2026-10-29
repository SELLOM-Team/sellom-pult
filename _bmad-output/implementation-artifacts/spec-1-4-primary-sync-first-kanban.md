---
title: 'Story 1.4 — Первичный синк истории и первый канбан'
type: 'feature'
created: '2026-10-29'
status: 'draft'
route: 'dispatch'
story_key: '1-4-первичный-синк-истории-и-первый-канбан'
context:
  - '_bmad-output/implementation-artifacts/epic-1-context.md'
  - '_bmad-output/implementation-artifacts/spec-1-3-marketplace-connect-token-vault.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** селлер, чьи токены WB подтверждены (Story 1.3), не видит своих заказов: конвейер (адаптер → RabbitMQ → worker → PG), синк истории и доска не существуют — ценность «свои заказы на живой доске ≤ 24 ч» недостижима (FR-3, гейт Ф0; Story 1.5 маппинг не начнётся без синка, Epic 4 канбан не на чем строить).

**Approach:** минимальный вертикальный срез конвейера: адаптер WB (conveyor/adapters) читает историю заказов statistics-API (FBO/FBS/realFBS — один источник), нормализует статусы в единый статусный словарь (core/sync, AD-22), публикует события в RabbitMQ quorum-очередь (routing key по organization_id — AD-4, приоритеты — заказы первыми); worker (conveyor/workers, бинарный) потребляет события и пишет заказы в PG идемпотентно (AD-15), чекпоинты цикла — (organization_id, тип цикла) с продолжением после сбоя (AD-3); прогресс синка экспонируется через gateway (фаза, %, ETA); SPA показывает прогресс и простейший канбан 7 колонок без фильтров (FR-16; полный канбан — Epic 4). Ozon в UI честно: «подключён, данные после завершения адаптера» (Epic 2, Story 2.2).

## Boundaries & Constraints

**Always:**
- Срез только по WB-заказам (history): catalog/номенклатура — Story 1.5; остатки/цены — Epic 5; продажи/финансы — Epic 6; циклический синк-поллинг как сервис — следующие сторис Epic 1 (здесь — первичный синк + чекпоинт-механизм, готовый к продолжению).
- Событие — конверт AD-22 `{event_id: UUIDv7, event_type: past-tense, organization_id, occurred_at, schema_version, payload}`; сущности версионируются в core-портах; консьюмер толерантен к неизвестным полям.
- Идемпотентность приёма AD-15: ключ `hash(площадка, тип сущности, внешний id, revision)`; доставка at-least-once (quorum + ретраи) — потребитель обязан быть идемпотентным; дубль события = NO-OP (проверяемо тестом).
- Токен WB расшифровывается ТОЛЬКО в адаптере конвейера (write-only vault 1.3 получает первого потребителя decrypt); расшифрованные байты не логируются, не покидают процесс адаптера, не хранятся.
- Прогресс синка честный: фаза/объём по фактическим чекпоинтам, ETA — оценка из наблюдаемой скорости; ошибки площадки (401/429/5xx/таймаут) — видимый статус sync с диагностикой, не «зелёный» прогресс.
- Канбан-карточка — сущность с позициями и суммами (FR-16-минимум); PII-контакты FBS НЕ синкаются в 1.4 (roles enforcement — Story 1.6/7.2; контакты требуют отдельного WB-метода) — в спеке это явный defer, не «потом додумаем».
- CI e2e: полный путь заказ-в-канбане через локальную WB-заглушку (детерминированно: без интернета, как 1.3).

**Never:**
- Никаких фильтров/драг-дропа/виртуализации канбана (Epic 4), SLA-таймеров (Epic 2+), вебхуков Ozon (Epic 2), ClickHouse (аналитика — позже),action-bus записи (AD-7 — канал ДЕЙСТВИЙ, не синка).
- Никакого изменения подключений/token-vault (1.3 закрыта; адаптер только читает шифротекст через порт TokenCipher).
- Никаких фоновых демонов «на глазок»: лимиты запросов к WB — консервативные, адаптивный backoff на 429/Retry-After; точные лимиты — верифицируются вебом на имплементации адаптера (паттерн 1.3: конфиг-константы).
- Никаких секретов в логах/событиях/чекпоинтах (только метаданные; маски по образцу 1.3).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|-----------|--------------|----------------------------|----------------|
| Автостарт синка | verified-подключение WB, синка не было | worker-планировщик подхватывает организацию: создаёт run (initial_orders), циклы по окнам дат от старейшей даты WB | нет verified-подключения → синк не стартует |
| Прогресс | GET /v1/sync/progress | 200 `{status: running/succeeded/failed/needs_retry, phase, percent, eta_seconds, details}` из sync_runs/checkpoints | БД недоступна → 503-конверт |
| Чекпоинт-продолжение | worker упал на окне 2024-05, чекпоинт сохранён | новый запуск продолжает с чекпоинта (AD-3), уже съеденные окна повторно не запрашиваются | — |
| Сбой WB-запроса | 401/403 (токен отозван), 429, 5xx, таймаут | run → failed/needs_retry с диагнозом; прогресс видит `error_hint`; повтор — POST /v1/sync/retry (идемпотентен) | экспоненциальный backoff внутри окна; 401 ≠ «ретрай» — человекочитаемая диагностика |
| Пустая история | кабинет без заказов | run succeeded, канбан пуст с честной заглушкой «заказов пока нет» | — |
| Дубль события | worker получил то же событие повторно (at-least-once) | INSERT … ON CONFLICT (organization_id, idem_key) DO NOTHING → NO-OP; счётчики не растут | — |
| Ревизия старше | событие с revision ≤ существующей строки | строка не откатывается (max-revision выигрывает); более новая revision = upsert | — |
| Канбан | GET /v1/board/orders | 200 `{columns:{new:[…],assembling:[…],shipping:[…],in_transit:[…],delivered:[…],returned:[…],risk:[…]}, totals:{…}}` — колонки 7, лимит на колонку Ф0=50, карточка: id, схема, склад, сумма, позиции, статусные метки времени | нет синка → 409/структурный honest-ответ «данные ещё синкаются» |
| Ozon подключён | пользователь на доске/прогрессе | везде копирайт: «Ozon подключён — данные появятся после завершения адаптера» | — |
| Деградация PG | gateway при мёртвой БД | /v1/sync/progress, /v1/board/orders → 503-конверт; SPA показывает offline-заглушку с retry (паттерн 1.2/1.3) | бюджет-паттерн на чтения |

</frozen-after-approval>

## Code Map

- `pult/core/sync/src/lib.rs` + `orders.rs` + `progress.rs` (новые модули) — чистый домен: каноническая модель Order/OrderItem/BoardColumn (7 словарь FR-16), статусный словарь WB→канон (маппер-таблица, версия словаря в schema_version), idem-ключ hash(площадка, тип, внешний id, revision), прогресс-домен (фазы, расчёт percent/ETA из оставшихся окон), конфигурация цикла (размер окна дат); порты в `ports.rs` — OrderEventsPublisher, OrdersProjectionStore, SyncCheckpointStore, WbOrdersSource (адаптеры реализуют).
- `pult/conveyor/adapters/src/wb/mod.rs` (новый) — клиент statistics-API: `GET {PULT_WB_STAT_BASE}/api/v1/supplier/orders?dateFrom=&flag=` (веб-верификация при имплементации; лимиты — адаптивные), расшифровка токена через порт TokenCipher (из core/identity — первый decrypt-потребитель), пагинация по окнам дат, конвертация строк в Order/OrderItem (raw payload → platform_payload), маппинг статусов через словарь core/sync.
- `pult/conveyor/workers/src/main.rs` (бинарный) — роли: (1) sync-оркестратор: планировщик организаций с verified-WB и незавершённым initial_orders (poll БД), прогон окон, публикация событий; (2) consumer: подписка на quorum-очередь, идемпотентная проекция в PG, ack после коммита; Graceful shutdown; stateless (AD-23-горизонт).
- `pult/db/migrations/0007_sync_orders_board.sql` (новый) — `sync_runs` (organization_id, cycle_type, status, window cursor, checkpoint jsonb, counters, error_hint, timestamps — чекпоинты AD-3), `orders` PARTITION BY HASH (organization_id) (AD-4; PARTITIONS 4 + default), `order_items` (FK на orders), UNIQUE (organization_id, idem_key) — дедуп AD-15, revision bigint, внешний id, scheme, канон-колонка, суммы, склад, platform_payload jsonb; `sync_progress`-представление поверх runs.
- `pult/gateway/src/sync.rs` + `board.rs` (новые) — GET /v1/sync/progress, POST /v1/sync/retry, GET /v1/board/orders; бюджеты на чтения (паттерн 1.2); error-конверты; placement-идентичность как в 1.2/1.3.
- `pult/platform/src/config.rs` — PULT_WB_STAT_BASE (default prod URL; CI-override), PULT_RABBITMQ_URL (amqp), sync-тюнинг (окно дат, batch, лимиты rps) — конфиг-постоянные, не код (AR-4).
- `pult/shells/bitrix24/src/screens/board/` + `sync/` (новые) — доска 7 колонок (без фильтров; карточки с позициями/суммами; пустые состояния честные), экран прогресса синка (фаза/%/ETA, retry при failed), Ozon-копирайт; api.ts — sync/board клиенты (JSON-семантика 1.2, таймауты).
- `deploy/docker-compose` (+worker-сервис в dev/CI-профиле, где он отсутствует), `.github/workflows/ci.yml` — integration: WB-заглушка (2xx с страницей заказов + 429-сценарий), полный e2e «событие → очередь → проекция → канбан», инварианты идемпотентности (дубль события = NO-OP), чекпоинт-продолжение (убить/перезапустить worker в CI — или моделировать рестарт через повторный прогон), plaintext-инварианты лога (по образцу 1.3).
- Продолжение: спеки 1-2/1.3 (бюджеты/конверты/SPA-паттерны), deferred-work.md (+2 defer: FBS-контакты PII — Story 1.6/7.2; физическое партиционирование order_items — при объёме, решения AD-4 здесь: orders HASH-партиции + routing key).

## Tasks & Acceptance

**Execution:**
- [x] `0007_sync_orders_board.sql` — sync_runs + orders (HASH-партиции AD-4) + order_items + idem UNIQUE (AD-15) + триггеры updated_at + индексы под канбан-запрос (колонка+org)
- [x] `core/sync` — домен: Order/OrderItem/BoardColumn, статусный словарь WB→канон (первые ~15 кодов WB; `[ASSUMPTION]` до живого токена владельца — фиксируется в спеке), idem-ключ, прогресс-математика (окна → percent/ETA); юнит-тесты: маппер, дедуп-логика, прогресс-расчёт, порядок ревизий
- [x] `ports.rs` — Publisher/ProjectionStore/CheckpointStore/WbOrdersSource; события — конверт AD-22 (schema_version, UUIDv7)
- [x] `conveyor/adapters` — WbOrdersSource: расшифровка (TokenCipher), окна дат, backoff 429/Retry-After, потолок тела, без логирования секретов; юнит-тесты (парсинг wire/фильтр окна/URL/redact; cipher round-trip vs схема шлюза; мок-HTTP уровня порта — в CI e2e)
- [ ] `conveyor/workers` — бинарный main: оркестратор (подбор org, прогон окон, чекпоинты после каждого окна/страницы) + consumer (ack после коммита, идемпотентность, max-revision); integration-тест с in-memory фейками портов (lapin не в юнитах — в CI)
- [ ] `gateway/sync.rs`/`board.rs` — 3 эндпоинта + бюджеты + конверты + placement; Rust-тесты хендлеров (мок-сторы)
- [ ] `platform/config.rs` — новые переменные + валидация + тесты веток
- [ ] compose — сервис worker (dev/CI), healthcheck; CI-интеграция: заглушка WB, e2e «заказ в канбане», дубль-событие NO-OP, чекпоинт-продолжение, 429-терпимость, plaintext-инварианты
- [ ] SPA — доска + прогресс + retry + Ozon-копирайт; Vitest: чистые функции (борд-группировка, статус-подсветка, прогресс-скрин из JSON), api-клиент, скелетоны
- [ ] Верификация: cargo fmt/clippy -D warnings/test --locked; npm test/build; гейты (hexagonal — adapters/workers не зависят от gateway; valkey; vault-grep теперь и на события; bundle AD-30 < 200 КБ gzip)
- [ ] Спека: задачи → [x], Review Triage Log,Verification-раздел (что проверено CI, что живым токеном — владельцем)

**Acceptance (из epics.md, верифицируется):**
1. Verified WB → автостарт initial_orders; активная часть онбординга ≤ 15 мин (селлер не ждёт у экрана — progress-виджет); полный первичный синк ≤ 24 ч (механика: окна + чекпоинты, скорость зависит от лимитов WB)
2. Прогресс виден: фаза, %, ETA; повтор после сбоя продолжает с чекпоинта (CI-проверка продолжения)
3. 10К+ SKU-каталоги не блокируют лёгкие порталы: слоты/приоритеты очереди (routing key org; quorum priority) — CI-стресс минимальный (2 org изолированы), полная изоляция — AD-6 в Epic 2
4. Заказы WB на доске 7 колонок ≤ 15 мин после появления (механика доставки); события AD-22; идемпотентность AD-15 (CI-инвариант дубля)
5. Ozon — честный копирайт «данные после завершения адаптера» (Epic 2)

## Implementation Notes

- lapin (RabbitMQ) и uuid v7 — версии веб-верифицировать при добавлении в workspace.dependencies (Директива 1; пины как AD-20-стайл); deps: tokio/axum/sqlx уже закреплены.
- quorum-очередь объявляется декларативно (x-quorum-queue: true); приоритеты quorum-очередей поддерживаются нативно (в отличие от classic) — проверить версию RabbitMQ 4.4 в compose при имплементации.
- Не вводить ClickHouse/CH-витрины — только PG-проекция канбана (FR-16 читает PG).
- Отдельная очередь НЕ создаётся под каждую org (Ф0); изоляция — routing key + fair-consumer (prefetch), per-org слоты — AD-6 при Epic 2.
- Статусный словарь WB: коды statistics-API имеют числовую семантику; полный словарь — файл-константа + карта на 7 канон-колонок; неизвестный код → колонка `risk` + платформа-событие «неизвестный статус» (не теряем).
- ETA = оставшиеся_окна × средняя_длительность_окна (скользящее); percent = подтверждённые окна + частичный вес текущего.
- CI-заглушка WB обязана повторить форму реального ответа orders (массив строк с date/lastChangeDate/nmId/orderId/status/…) — иначе маппер не проверен.
