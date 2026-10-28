---
title: 'Story 1.2 — Шаг 0: фильтр периметра «ваш бизнес»'
type: 'feature'
created: '2026-10-28'
status: 'done'
route: 'dispatch'
review_loop_iteration: 0
baseline_commit: '2e02d8f'
story_key: '1-2-шаг-0-фильтр-периметра-ваш-бизнес'
context:
  - '_bmad-output/implementation-artifacts/epic-1-context.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** при первом открытии приложения селлер не может честно узнать, подходит ли продукт его модели торговли; не-селлеры (CTM-ателье, сервисные бизнесы, общепит, опт без площадок) попадают в онбординг и тратят время; доля установок, прошедших фильтр, не замеряется (FR-1; знаменатель активации UJ-1).

**Approach:** шаг 0 онбординга — минимальный вопросник торговой модели (без PII) в SPA; вердикт «селлер / не-селлер» вычисляется правилами домена `core/identity` и сохраняется на gateway (immutable per-user); селлер попадает в воронку (экран шага 1 — существующий «Начните подключение»), не-селлер — на терминальный исходящий экран с рекомендацией; метрика доли установок, прошедших шаг 0, — агрегат по таблице вердиктов через gateway.

## Boundaries & Constraints

**Always:**
- Шаг 0 собирает минимум данных и ровно ноль PII: только торговая модель и (для селлеров) список площадок; без имён, телефонов, почт.
- Вердикт иммутабелен per-user (UNIQUE(user_id) в БД; повторный POST возвращает сохранённый вердикт без перезаписи).
- Идентичность — только из контекста placement Б24 (query-параметры: DOMAIN, USER_ID, PLACEMENT); реальная проверка подписи/хендшейка — Story 1.3 (нужна регистрация приложения во внешнем контуре). Dev-режим без placement: документированный фоллбэк `member_id='dev-portal'` / `user='dev-user'`, исключаемый из метрик.
- Вход только через gateway (AD-13), контракты `/v1`; ошибки — конверт `{error:{code,message,request_id,details}}`; SQL — runtime-запросы sqlx (паттерн пробы /health; compile-time — при появлении PG в dev-цикле).
- Правила вердикта — в `core/identity` (чистый домен, без sqlx/axum/serde; строковые литералы маппятся в gateway явным match с тестами); gateway — композиция + sqlx-стор, реализующий порт домена (AD-1).
- Не-селлер не входит в воронку: его экран терминален, но приглашение вернуться — честное.

**Never:**
- Никакой собственной аутентификации, паролей, сессий, JWT (NFR-6).
- Никакого сбора PII на шаге 0.
- Мастер подключения/токены — Story 1.3; событийный конвейер/RabbitMQ/ClickHouse-аналитика — 1.4+ (метрика шага 0 — детерминированный SQL-агрегат; AD-22-событие появится вместе с конвейером).
- Не менять семантику и спеку `/health` и `/ready` (Story 1.1, проверено ревью).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Первый вход | GET /v1/onboarding/state + placement-контекст | 200 `{state:"step0_required"}`; org+user upsert-нуты (member_id → organizations.b24_member_id UNIQUE; user по external_ref) | БД недоступна → 503 error-конверт |
| Ответ селлера | POST /v1/perimeter/verdict `{trade_model:"marketplace_seller"|"marketplace_planned", marketplaces:[...]}` | 200 `{verdict:"seller", state:"funnel"}` | невалидная модель/пустые marketplaces → 422 |
| Ответ не-селлера | trade_model `"own_channels_no_marketplaces"` \| `"services_other"` | 200 `{verdict:"non_seller", state:"exit_screen"}` | — |
| Повторный POST | вердикт пользователя уже сохранён | 200 с ранее сохранённым вердиктом (без перезаписи/ошибки) | — |
| Повторное открытие | GET state при сохранённом вердикте | `state:"funnel"` или `"exit_screen"` — вопросник не показывается | — |
| Dev-режим | запрос без placement-параметров | фоллбэк dev org/user; работает весь поток | dev-сущности в метриках не считаются |
| Метрика | GET /v1/metrics/perimeter | 200 `{installs, step0_completed, sellers, step0_share}` — агрегат без PII, без dev-портала | — |
| Пустой ответ | POST без trade_model | 422 error-конверт, code человекочитаемый | — |

</frozen-after-approval>

## Code Map

- `pult/core/identity/src/{lib.rs,ports.rs}` — пустой домен + IdentityPort-заглушка; наполняются perimeter-модулем (TradeModel, Verdict, decide_verdict) и портом `PerimeterStore` (load/save_verdict). Не добавлять serde/sqlx в крейт.
- `pult/gateway/src/main.rs` — bootstrap: AppState{db,version,migrations_applied}, /health+/ready; подключить новый router, расширить AppState.
- `pult/platform/src/error.rs` — конверт ошибок (формат переиспользовать как есть).
- `pult/db/migrations/0002_users.sql` — users: `external_ref` (точка привязки B24-пользователя), `UNIQUE (organization_id, external_ref)`, `UNIQUE (organization_id, id)` — цель составного FK (паттерн ревью 1.1).
- `pult/shells/bitrix24/src/{App.tsx,b24.ts,health.ts,ConnectionStatus.tsx,b24.test.ts,health.test.ts}` — текущий экран, точка интеграции placement, паттерны чистых функций + vitest.
- `pult/deploy/docker-compose.yml` + `.github/workflows/ci.yml` — integration-джоба: паттерн curl-проверок с http_code-ассертами (расширить шагом 0).
- `pult/README.md` — секции запуска/переменных (дополняются).
- Continuity: спека spec-1-1-structural-seed.md (done) — гейты, конвенции, уроки ревью.

## Tasks & Acceptance

**Execution:**
- [x] `pult/db/migrations/0005_perimeter_verdicts.sql` — таблица perimeter_verdicts (id uuid pk; organization_id+user_id c составным FK → users(organization_id,id) ON DELETE CASCADE; trade_model text NOT NULL; marketplaces jsonb NOT NULL DEFAULT '[]'; verdict text NOT NULL CHECK IN ('seller','non_seller'); created_at timestamptz DEFAULT now(); UNIQUE(user_id)) + ALTER organizations ADD b24_member_id text UNIQUE — ключ реестра установок
- [x] `pult/core/identity/src/perimeter.rs` — TradeModel (4 варианта), Verdict (Seller/NonSeller), decide_verdict (marketplace_seller|marketplace_planned → Seller; own_channels_no_marketplaces|services_other → NonSeller), PerimeterAnswers; юнит-тесты всех правил
- [x] `pult/core/identity/src/ports.rs` — трейт PerimeterStore (load_verdict/save_verdict) рядом с IdentityPort
- [x] `pult/core/identity/src/lib.rs` — pub mod perimeter + актуализация доко домена
- [x] `pult/gateway/src/onboarding.rs` — router `/v1/onboarding/state`, `/v1/perimeter/verdict`, `/v1/metrics/perimeter`; upsert org/user по placement-контексту (dev-фоллбэк); sqlx-стор PerimeterStore; строковый маппинг ↔ enum с тестами; идемпотентность через UNIQUE(user_id) ON CONFLICT
- [x] `pult/gateway/src/main.rs` — подключение router, расширение AppState
- [x] `pult/shells/bitrix24/src/b24.ts` + `b24.test.ts` — контекст расширяется полями DOMAIN/USER_ID (парсинг, без PII)
- [x] `pult/shells/bitrix24/src/api.ts` + `api.test.ts` (новые) — клиент state/verdict/metrics: JSON-семантика ответов (как health.ts), error-конверт-парсинг
- [x] `pult/shells/bitrix24/src/screens/Questionnaire.tsx`, `screens/ExitScreen.tsx` (новые) — шаг 0 (2 вопроса, honest-копирайт, скелетоны) и исходящий экран (честный отказ, периметр, рекомендация партнёрского решения — контент-слот VITE_PARTNER_* с честным дефолтом «каталог партнёров Битрикс24.Маркет», приглашение вернуться)
- [x] `pult/shells/bitrix24/src/App.tsx` + `appState.test.ts` — чистая функция screenForState (step0_required→Questionnaire, funnel→существующий экран, exit_screen→ExitScreen, ошибки/офлайн→честная заглушка); App-роутинг на ней
- [x] `pult/README.md` — эндпоинты шага 0, переменные VITE_PARTNER_*, поведение dev-фоллбэка
- [x] `.github/workflows/ci.yml` — integration: end-to-end шага 0 (state→step0_required; verdict seller→funnel; повторный POST идемпотентен; verdict non_seller→exit_screen; метрика считает; dev-фоллбэк в метрику не входит)

**Acceptance Criteria:**
- Given первый вход с placement-контекстом, when GET /v1/onboarding/state, then создаются org+user и возвращается `step0_required` (вопросник показывается).
- Given ответы селлера, when POST verdict, then `verdict=seller` и все последующие открытия показывают экран воронки («Начните подключение»), вопросник не повторяется.
- Given ответ не-селлера, when POST verdict, then `verdict=non_seller`, показывается исходящий экран с честным отказом и рекомендацией, в воронку пользователь не попадает.
- Given сохранённый вердикт, when повторный POST и повторные GET, then возвращается прежний вердикт/state (идемпотентность, per-user запоминание).
- Given GET /v1/metrics/perimeter, when есть orgs и вердикты (без dev-портала), then вычисляются installs / step0_completed / sellers / step0_share — знаменатель активации UJ-1 виден.
- Given CI integration-джоба, when стек поднят, then шаг 0 прогоняется end-to-end и все ассерты зелены; гейты fmt/clippy/test, hexagonal-lint, valkey-grep, bundle-size (<200 КБ gzip) зелены.

## Implementation Notes

- 2026-10-28, оркестратор + dev-субагент: реализация по спеке (субагент прерван по таймауту сессии после ~90% работы — код/тесты/SPA готовы; оркестратор дозакрыл README-секцию шага 0 и CI-шаги e2e, YAML-ловушка «: » в именах шагов закавычена). Проверка «битых скобок» в выводе — python-строка (`[m`-глотание вывода подтверждено артефактом, не дефектом кода; урок Task 11-12 применим).
- pult/Cargo.toml: sqlx +features json,uuid (jsonb marketplaces и ключи users/organizations в рантайм-запросах шлюза); gateway +uuid; core/identity +uuid (только тип для порта) — serde/sqlx в домен НЕ добавлены.
- SPA: существующий экран «Начните подключение» извлечён в screens/StartConnection.tsx (без изменения семантики /health-статуса — health.ts переиспользован); App.tsx — роутинг через чистую screenForState; Vite-прокси расширен на /v1; vite-env.d.ts типизирует VITE_PARTNER_*.
- Тесты: 51 Rust (identity 5 perimeter + gateway 26 [21 новых onboarding] + platform 20) + 44 SPA (api 20 / appState 12 [включая partnerSlot] / b24 7 / health 5) — все зелены локально; гейты hexagonal/valkey/bundle PASS (72 318 Б gzip < 200 000); живая деградация: /v1 → 503 database_unavailable-конверт, 422-валидация работает без БД, /health не тронут.
- CI integration: +3 шага e2e шага 0 (селлер: state→verdict→funnel→иммутабельность; не-селлер: exit_screen; валидация 422 + метрика installs=2/completed=2/sellers=1/share=1.0 — dev-фоллбэк и sentinel-строка b24_member_id NULL не считаются). Прогон — на push (внешняя верификация).

## Spec Change Log
- 2026-10-28, оркестратор (ревью-патчи, шаг 04): 22 группы применены (таблица выше). Ключевое: таймауты (клиент 8с / стор 5с) — зависание больше не «вечный скелетон»; валидация площадок закрытым списком; dev-portal — единственная константа (bind) + CI реально создаёт dev-фоллбэк и проверяет исключение из метрик; деградация /v1 закреплена в CI; 404/405 под конвертом; канонизация DOMAIN; чистые функции screenStateFromError/step0 с тестами. СРЕДА: окружение сессии частично умирало (~16:10 UTC+8) — ~/.cargo и ~/.rustup исчезли, node_modules SPA снесён, _bmad/render и tool-results стёрты; восстановлено: rustup 1.99.1 (rustfmt+clippy), npm ci, re-render скилла. Урок: целевые каталоги вне Git-репо не переживают сброс среды — репо единственная константа (подтверждение Директивы 3).


## Review Triage Log

Ревью 28.10.2026 (3 слоя: blind-hunter [17], edge-case-hunter [10], verification-gap [4, pre-verified]). Триаж: 0 intent_gap / 0 bad_spec — лупбэка нет; 22 группы patch (применены оркестратором: dev-субагент шага 03 умер по таймауту сессии без возможности re-engage) / 3 defer / 3 rejected.

| # | Слой | Локация | Находка | Вердикт | Маршрут |
|---|------|---------|---------|---------|---------|
| 1 | blind+edge | App.tsx/api.ts | fetch state/verdict без таймаута — зависший шлюз = вечный скелетон (регресс против паттерна 1.1) | medium | patch: REQUEST_TIMEOUT_MS 8с + AbortController в requestJson (покрывает оба вызова) |
| 2 | edge | onboarding.rs | хендлеры ждут sqlx без бюджета — PG-зависание вешает запрос без 503 | medium | patch: STORE_BUDGET 5с (budgeted_sqlx/budgeted_store, паттерн /health) |
| 3 | blind+edge | onboarding.rs | marketplaces — свободный текст (мусор/дубли в jsonb) | medium | patch: закрытый список wildberries/ozon/yandex_market + lowercase + sort/dedup + 422 unknown_marketplace |
| 4 | blind+edge+VG | onboarding.rs, ci.yml | dev-portal: литерал 3× в SQL (дрейф константы) + исключение из метрик не проверено CI | medium | patch: bind $1 константой DEV_MEMBER_ID; no-query curl в CI до метрики — счётчики 2/2/1 неизменны |
| 5 | VG | ci.yml | контракт деградации /v1 (422 без БД; 503-конверт) не верифицирован | medium | patch: /v1-ассерты в шаге stop-postgres (v1_code=422 invalid_trade_model; state_code=503 database_unavailable) |
| 6 | VG | App.tsx | классификация ошибки входа (конверт шлюза ≠ офлайн) инлайн и не протестирована | medium | patch: screenStateFromError в appState.ts + 4 теста |
| 7 | VG | Questionnaire.tsx | клиентский гейтинг (селлер ⇒ площадки; блок повторной отправки) без регрессионной сети | medium | patch: step0.ts (needsMarketplaces/canSubmitStep0) + 5 тестов |
| 8 | blind | ConnectionStatus.tsx | устаревший копирайт «фильтр появится в 1.2» — фильтр уже пройден пользователем | low | patch: копирайт про мастер 1.3 |
| 9 | blind | ExitScreen.tsx | обещание автоматического «вход откроется» противоречит иммутабельности вердикта | low | patch: «поддержка пересмотрит вход» — честный ручной путь |
| 10 | blind | onboarding.rs | метрика raw-SQL в хендлере мимо стора | low | patch: PgPerimeterStore::perimeter_metrics |
| 11 | blind | onboarding.rs | request_id генерируется, но не логируется — не коррелируется с логами | low | patch: warn!(code, request_id) в error_response |
| 12 | blind | ci.yml | psql-шаг «ровно 4 таблицы» устарел после 0005 | low | patch: 5 таблиц (IN + perimeter_verdicts) |
| 13 | blind | sprint-status.yaml | статус стори (in-progress) расходится с фазой ревью | low | patch: 1-2 → review (шаг 05) |
| 14 | blind | README.md | нет кодов ошибок /v1, семантики счётчиков, заметки same-origin | low | patch: три абзаца добавлены |
| 15 | blind | App.tsx | unavailable-экран без кнопки повтора | low | patch: «Повторить» (attempt-рестарт эффекта) |
| 16 | blind | onboarding.rs | лог «зафиксирован» и на идемпотентном повторе | low | patch: стор логирует replay («уже сохранён»), хендлер — «действующий вердикт» |
| 17 | edge | onboarding.rs | эхо невалидной trade_model без обрезки (мегабайт в 422) | low | patch: echo() ≤64 символа + тест |
| 18 | edge | onboarding.rs | DOMAIN=dev-portal коллизирует с фикстурой (слияние порталов) | low | patch: 422 reserved_member_id + тест |
| 19 | edge | onboarding.rs | DOMAIN без канонизации регистра — дубли организаций, инфляция знаменателя | low | patch: to_lowercase + тест |
| 20 | edge | router | 404/405 без error-конверта (обход контракта) | low | patch: fallback + method_not_allowed_fallback (main.rs) |
| 21 | edge | ci.yml | grep '"installs":2' матчит префикс (installs=25 — ложный зелёный) | low | patch: якоря «значение,» / «share:1.0}» |
| 22 | edge | ExitScreen.tsx | 'https://' (голая схема) проходила prefix-regex | low | patch: new URL-парсинг (hostname обязателен) + тесты; НЮАНС: http:///path валиден по WHATWG (hostname='path') — ожидание теста исправлено, не код |
| 23 | blind | GET state | неаутентифицированный write-on-GET создаёт «установки» (краулеры) до хендшейка | medium | defer: документированная аппроксимация спеки до 1.3; защита = хендшейк 1.3 / rate-limit 2.3 |
| 24 | blind | gateway | нет HTTP-уровня Rust-тестов (роутинг/rejections/race) | low | defer: тест-инфраструктура (tower oneshot / sqlx::test при PG в dev-цикле) — Epic 2 hardening |
| 25 | blind | Questionnaire.tsx | только 3 площадки, нет «другой» (AliExpress-селлер вынужден мисселектить) | low | defer: продуктовое решение мастера 1.3 (Ф0-периметр площадок = WB/Ozon/ЯМ) |
| 26 | blind | api.ts | fetchPerimeterMetrics без потребителя (мёртвый код) | false | отклонено: клиент metrics — явный таск спеки; tree-shaken в бандле; фикс = правка спеки |
| 27 | blind | ports.rs | save_verdict принимает и answers, и verdict (избыточность) | false | отклонено: единственный вызов консистентен по построению (decide_verdict в хендлере); расходящегося вызывающего нет |

Патчи применены + полная верификация: 57 Rust-тестов (identity 5, gateway 32 [+6], platform 20), 54 SPA-теста (api 20, appState 17 [+5], step0 5 [новые], b24 7, health 5), гейты hexagonal/valkey/bundle PASS (72 318 Б gzip), живьём: reserved_member_id 422 / 404 и 405 конверты / деградация /v1, fmt/clippy чисто, YAML валиден. CI-прогон — внешний (push).
## Design Notes

- Реестр установок: organizations.b24_member_id (UNIQUE, nullable) — реальная установка Б24 отразится здесь при ONAPPINSTALL (1.3+); в 1.2 строка создаётся первым placement-запросом (документированная аппроксимация «установки» для Ф0-пилотов).
- display_name пользователя = `Б24 user {external_ref}` — имя не собираем (шаг 0 без PII); реальные имена — только после хендшейка 1.3, под PII-периметром AD-16.
- Идемпотентность вердикта: `INSERT … ON CONFLICT (user_id) DO NOTHING` + дочитать существующую строку — вердикт неизменяем, гонки двух параллельных POST дают один результат.
- Метрика excludes: `b24_member_id = 'dev-portal'` (dev-фоллбэк — фикстура, не установка).
- Партнёрская рекомендация: критерий рев-шер ≥ 30% — бизнес-условие выбора партнёра владельцем при публикации; код поставляет контент-слот (VITE_PARTNER_TITLE/URL/с honest-дефолтом), решение зафиксировано делегированием владельца (как 8.1).
- Event envelope AD-22 для шага 0 — отложен до конвейера (1.4): источник метрики — таблица вердиктов (детерминированный агрегат), событие будет журналироваться тем же кодом публикации.

## Verification

**Commands:**
- `cargo build --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace` (в `pult/`) -- expected: чисто; новые тесты identity+gateway зелены
- `npm run test && npm run build` (в `pult/shells/bitrix24/`) -- expected: vitest зелены (b24/api/appState); dist собран
- `node platform/scripts/bundle-size.mjs shells/bitrix24/dist 200000` -- expected: PASS (бюджет AD-30 с новыми экранами)
- `node platform/scripts/hexagonal-lint.mjs && node platform/scripts/valkey-grep.mjs` -- expected: PASS
- Живой прогон без БД: gateway стартует, /v1-эндпоинты отдают 503 error-конверт (деградация не ломает маршруты)

**Manual checks (owner/CI):**
- CI integration: end-to-end шага 0 зелёный; вживую в iframe Б24 — после регистрации приложения (1.3, внешняя зависимость)
