---
title: 'Story 1.2 — Шаг 0: фильтр периметра «ваш бизнес»'
type: 'feature'
created: '2026-10-28'
status: 'in-progress'
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
- [ ] `pult/db/migrations/0005_perimeter_verdicts.sql` — таблица perimeter_verdicts (id uuid pk; organization_id+user_id c составным FK → users(organization_id,id) ON DELETE CASCADE; trade_model text NOT NULL; marketplaces jsonb NOT NULL DEFAULT '[]'; verdict text NOT NULL CHECK IN ('seller','non_seller'); created_at timestamptz DEFAULT now(); UNIQUE(user_id)) + ALTER organizations ADD b24_member_id text UNIQUE — ключ реестра установок
- [ ] `pult/core/identity/src/perimeter.rs` — TradeModel (4 варианта), Verdict (Seller/NonSeller), decide_verdict (marketplace_seller|marketplace_planned → Seller; own_channels_no_marketplaces|services_other → NonSeller), PerimeterAnswers; юнит-тесты всех правил
- [ ] `pult/core/identity/src/ports.rs` — трейт PerimeterStore (load_verdict/save_verdict) рядом с IdentityPort
- [ ] `pult/core/identity/src/lib.rs` — pub mod perimeter + актуализация доко домена
- [ ] `pult/gateway/src/onboarding.rs` — router `/v1/onboarding/state`, `/v1/perimeter/verdict`, `/v1/metrics/perimeter`; upsert org/user по placement-контексту (dev-фоллбэк); sqlx-стор PerimeterStore; строковый маппинг ↔ enum с тестами; идемпотентность через UNIQUE(user_id) ON CONFLICT
- [ ] `pult/gateway/src/main.rs` — подключение router, расширение AppState
- [ ] `pult/shells/bitrix24/src/b24.ts` + `b24.test.ts` — контекст расширяется полями DOMAIN/USER_ID (парсинг, без PII)
- [ ] `pult/shells/bitrix24/src/api.ts` + `api.test.ts` (новые) — клиент state/verdict/metrics: JSON-семантика ответов (как health.ts), error-конверт-парсинг
- [ ] `pult/shells/bitrix24/src/screens/Questionnaire.tsx`, `screens/ExitScreen.tsx` (новые) — шаг 0 (2 вопроса, honest-копирайт, скелетоны) и исходящий экран (честный отказ, периметр, рекомендация партнёрского решения — контент-слот VITE_PARTNER_* с честным дефолтом «каталог партнёров Битрикс24.Маркет», приглашение вернуться)
- [ ] `pult/shells/bitrix24/src/App.tsx` + `appState.test.ts` — чистая функция screenForState (step0_required→Questionnaire, funnel→существующий экран, exit_screen→ExitScreen, ошибки/офлайн→честная заглушка); App-роутинг на ней
- [ ] `pult/README.md` — эндпоинты шага 0, переменные VITE_PARTNER_*, поведение dev-фоллбэка
- [ ] `.github/workflows/ci.yml` — integration: end-to-end шага 0 (state→step0_required; verdict seller→funnel; повторный POST идемпотентен; verdict non_seller→exit_screen; метрика считает; dev-фоллбэк в метрику не входит)

**Acceptance Criteria:**
- Given первый вход с placement-контекстом, when GET /v1/onboarding/state, then создаются org+user и возвращается `step0_required` (вопросник показывается).
- Given ответы селлера, when POST verdict, then `verdict=seller` и все последующие открытия показывают экран воронки («Начните подключение»), вопросник не повторяется.
- Given ответ не-селлера, when POST verdict, then `verdict=non_seller`, показывается исходящий экран с честным отказом и рекомендацией, в воронку пользователь не попадает.
- Given сохранённый вердикт, when повторный POST и повторные GET, then возвращается прежний вердикт/state (идемпотентность, per-user запоминание).
- Given GET /v1/metrics/perimeter, when есть orgs и вердикты (без dev-портала), then вычисляются installs / step0_completed / sellers / step0_share — знаменатель активации UJ-1 виден.
- Given CI integration-джоба, when стек поднят, then шаг 0 прогоняется end-to-end и все ассерты зелены; гейты fmt/clippy/test, hexagonal-lint, valkey-grep, bundle-size (<200 КБ gzip) зелены.

## Implementation Notes

## Spec Change Log

## Review Triage Log

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
