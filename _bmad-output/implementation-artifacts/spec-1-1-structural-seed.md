---
title: 'Story 1.1 — Стартовый каркас приложения и Structural Seed'
type: 'feature'
created: '2026-10-28'
status: 'in-review'
route: 'dispatch'
review_loop_iteration: 0
baseline_commit: '35c2b774b8ec0ba1b048d85fbf39e6d9934dc3f2'
story_key: '1-1-стартовый-каркас-приложения-и-structural-seed'
context:
  - '_bmad-output/implementation-artifacts/epic-1-context.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** репозиторий содержит только планировочные артефакты BMAD — кода приложения не существует; ни одна из 39 сторис Ф0 не может начаться без стартового каркаса (Structural Seed из spine, AR-3).

**Approach:** создать Rust-workspace `pult/` по структуре spine (gateway + 7 core-доменов + 4 conveyor-крейта + platform), docker-compose инфраструктуру (PG 18 + RabbitMQ 4.4 quorum + Valkey 9.1), первые forward-only миграции `sqlx migrate`, CI с гейтами (build/clippy/test, hexagonal-lint, Valkey-греп, bundle-size) и SPA-каркас React 19 + Vite 8 в `shells/bitrix24` с экраном «Начните подключение».

## Boundaries & Constraints

**Always:**
- Hexagonal: крейты `core/*` не зависят и не импортируют `conveyor/*` и `shells/*` (проверяется гейтом).
- Сервер кэша — только Valkey 9.1, образ `valkey/valkey`; docker-образ `redis:` запрещён (CI-греп, AD-28).
- Стек зафиксирован: Rust stable 1.99.1 (`rust-toolchain.toml`), Tokio 1.54.1, Axum 0.8.9, SQLx 0.9.0; React 19 + Vite 8 + TypeScript (AD-20/30).
- Миграции — forward-only `sqlx migrate`; в Story 1.1 создаются ТОЛЬКО таблицы organizations, users, role_assignments, portal_settings (принцип «таблицы под собственные нужды сторис»).
- Конвенции данных: деньги — integer-копейки; время — timestamptz UTC; ошибки — конверт `{error:{code,message,request_id,details}}`; события — конверт AD-22 (event_id UUIDv7, past-tense тип, organization_id, occurred_at, schema_version, payload).
- `/health` у gateway проверяет БД (sqlx-пул) и отдаёт 200/503 с версией приложения.

**Never:**
- Никакой бизнес-логики Ф0 вне каркаса: без адаптеров площадок, канбанов, синков, авторизации Б24 (их сторис 1.2+, Epic 2+).
- Никаких отдельных паролей/собственной аутентификации — авторизация только через контур Б24 (NFR-6; в каркасе — точка интеграции, не реализация).
- Никакого Redis-клиента/образа; никакой обратной записи в Ozon (решение 8.1).
- Не переносить deploy-механику продакшена (k8s/systemd) в эту сторис — только dev-compose + CI.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Гейт Valkey | любой файл `*.yml/*.yaml/Dockerfile` содержит `image: ...redis` | CI падает с ошибкой AD-28 | греп-скрипт возвращает ненулевой код |
| Гейт hexagonal | `core/*/Cargo.toml` зависит от `pult-conveyor-*`/shell-крейтов, либо `use` путей к ним | CI падает с ошибкой AD-1 | скрипт перечисляет нарушившие крейты |
| Гейт bundle-size | суммарный gzip JS-ассетов входных чанков SPA ≥ 200 000 байт | CI падает с ошибкой AD-30 | скрипт печатает фактический размер и лимит |
| DB недоступна | gateway запущен, PG не отвечает | `/health` → 503, `status: degraded` | повторные попытки пула не блокируют поток ответа |
| Чистая БД | миграции применяются на пустой PG 18 | 4 таблицы созданы, повторный запуск миграций — no-op | sqlx миграции идемпотентны (таблица _sqlx_migrations) |

</frozen-after-approval>

## Code Map

Greenfield: переиспользовать нечего; всё создаётся заново.

- `pult/` -- корень воркспейса: `Cargo.toml` (workspace, edition 2024), `rust-toolchain.toml` (1.99.1), `Cargo.lock` (коммитится).
- `pult/gateway/` -- бин `pult-gateway`: Axum 0.8.9, `/health`, `/ready`; при старте `sqlx::migrate!` из `../../db/migrations`.
- `pult/core/{sync,data,finance,shturman,rules,billing,identity}/` -- 7 lib-крейтов `pult-core-*`; каждый: `src/lib.rs` (пустой домен) + `src/ports.rs` (заготовка порта-трейта по AR-2).
- `pult/conveyor/{adapters,action_bus,reconciliation,workers}/` -- 4 lib-крейта `pult-conveyor-*`, пустые заготовки.
- `pult/platform/` -- крейт `pult-platform`: событийный конверт AD-22, конверт ошибок, конфиг; `platform/scripts/` -- гейт-скрипты (hexagonal-lint.mjs, valkey-grep.mjs, bundle-size.mjs).
- `pult/db/migrations/` -- `0001_organizations.sql` … `0004_portal_settings.sql` (формат sqlx migrate).
- `pult/deploy/docker-compose.yml` + `deploy/rabbitmq/rabbitmq.conf` -- dev-стек: postgres:18, rabbitmq:4.4-management (default_queue_type=quorum), valkey/valkey:9.1; healthchecks; БД pult/pult.
- `pult/shells/bitrix24/` -- SPA: Vite 8 + React 19 + TS; экран «Начните подключение»; точка интеграции B24 (placement-параметры iframe); честная заглушка статуса подключения.
- `.github/workflows/ci.yml` -- jobs: rust (build+clippy+test), spa (build+bundle-gate), gates (lint+греп), integration (compose up --wait, миграции, /health, psql-проверка таблиц).
- `_bmad-output/implementation-artifacts/epic-1-context.md` -- дистиллированный контекст эпика (не изменять).

## Tasks & Acceptance

**Execution:**
- [x] `pult/Cargo.toml`, `rust-toolchain.toml`, `.gitignore`-дополнения -- воркспейс на 13 крейтов, edition 2024, пины версий AD-20 -- единая точка сборки
- [x] `pult/platform/` -- конверты событий/ошибок/конфиг -- общий язык доменов
- [x] `pult/core/*` (7 крейтов) + `pult/conveyor/*` (4 крейта) -- lib + ports.rs заготовки -- AR-2/AR-3
- [x] `pult/gateway/` -- Axum-бин: `/health`, `/ready`, миграции на старте -- проверяемый жизненный цикл
- [x] `pult/db/migrations/0001..0004` -- 4 таблицы, FK users→organizations, role_assignments→organizations/users, portal_settings→organizations -- только нужды Story 1.1
- [x] `pult/deploy/` -- compose + rabbitmq.conf (quorum) -- AC «compose поднимает стек» (фикс volume-пути postgres:18 — см. Implementation Notes)
- [x] `pult/platform/scripts/` -- 3 гейт-скрипта -- AD-1/AD-28/AD-30 как исполняемые проверки
- [x] `pult/shells/bitrix24/` -- SPA-каркас: entry «Начните подключение», health-заглушка, скелетон -- AC SPA < 200 КБ gzip (69 534 Б gzip — PASS)
- [x] `.github/workflows/ci.yml` -- 4 jobs со всеми гейтами -- AC CI (фикс путей spa-job — см. Implementation Notes)
- [x] `pult/README.md` -- запуск dev-стека одной командой -- DX следующего агента

**Acceptance Criteria:**
- Given чистая машина с rustup, when `cargo build --workspace` в `pult/`, then сборка проходит без ошибок; `cargo clippy --workspace -- -D warnings` и `cargo test --workspace` чисты.
- Given `docker compose up -d --wait` в `pult/deploy/`, when стек поднялся, then PG 18, RabbitMQ 4.4 (quorum) и Valkey 9.1 (`valkey/valkey`) здоровы; повторный `up` не ломает данные.
- Given gateway запущен с DATABASE_URL на чистую БД, when применены миграции, then существуют organizations, users, role_assignments, portal_settings; `/health` → 200.
- Given CI на push, when все jobs отработали, then build/clippy/test, hexagonal-lint, Valkey-греп, bundle-size (<200 КБ gzip) и integration-проверки зелены; PR с `image: redis` или импортом adapters в core -- красный.
- Given SPA `npm run build`, when замерен критический путь, then gzip JS < 200 000 байт; каркас открывается в iframe-режиме с экраном «Начните подключение».
- Верификация «установка из Маркета Б24» -- внешняя, владелец (нужен dev-аккаунт Б24); код поставляет iframe-вход + локальный dev-режим.

## Implementation Notes

- 2026-10-28, dev-субагент: Structural Seed поставлен по Code Map; `sqlx::migrate!` путь `../db/migrations` (в Code Map была опечатка `../../` — из pult/gateway один уровень до pult/db); Rust 1.99.1 установлен rustup'ом в песочнице; версии SPA (Vite 8.3.5 / React 19.3.1 / TS 5.9.3) верифицированы реестром; Cargo.lock и package-lock.json закоммичены.
- 2026-10-28, оркестратор (верификация по диффу): локальные проверки воспроизведены — cargo build/clippy/test 11 тестов зелены; гейты PASS на каркасе и FAIL на инъекциях (exit=1 все три, включая негатив bundle-size с лимитом 1000); degraded-режим живьём: /health→503 {"status":"degraded","version":"0.1.0"}, /ready→503 not_ready, шлюз жив, ретрай миграций в логе.
- 2026-10-28, оркестратор (фиксы первого CI-прогона 035a781): (1) spa-job — шаг bundle-гейта резолвил пути относительно job-дефолта pult/shells/bitrix24 → добавлен шаговый `working-directory: ${{ github.workspace }}`; (2) integration-job — postgres:18 exited(1): образ сменил каталог данных (web-верифицировано 28.10.2026), volume перемонтирован `pgdata:/var/lib/postgresql` (было `/var/lib/postgresql/data`), добавлен диагностический шаг `compose logs` при failure.
- Внешние верификации (владелец/CI): зелёный CI-прогон после фиксов (строка матрицы «миграции идемпотентны» покрывается job-ой integration); установка из Маркета Б24 — dev-аккаунт владельца.

## Spec Change Log

## Review Triage Log

Ревью 28.10.2026 (3 слоя: blind-hunter, edge-case-hunter, verification-gap). Вердикты: high/medium/low/false/maybe-false; маршруты: patch / отклонено.

| # | Слой | Место | Находка | Вердикт | Обоснование / маршрут |
|---|------|-------|---------|---------|----------------------|
| 1 | verification-gap | ci.yml gates+spa | Гейты AD-1/28/30 в CI исполняются только в PASS-направлении; FAIL-направление — разовый ручной прогон; регресс детекции пройдёт зелёным | medium | Подтверждено: CI-джобы зовут скрипты только на чистом дереве; тестов .mjs нет. → patch: самотесты гейтов с фикстурами в CI |
| 2 | verification-gap | ci.yml:146-160 | Шаг «миграции no-op» после рестарта шлюза ассертит только /health, который не читает migrations_applied; идемпотентность ничем не pinned | medium | Подтверждено: health() не читает флаг миграций; /ready ассертится только до рестарта. → patch: после рестарта ассерт /ready→200 |
| 3 | verification-gap | b24.ts, spa job | readPlacementContext (контракт iframe-placement) без автотестов; «честный статус» может сломаться без сигнала | medium | Подтверждено: ни vitest/node:test, ни тест-степа в spa job. → patch: unit-тест + npm test в CI |
| 4 | verification-gap | ci.yml:140-145 | Форма схемы (колонки/FK) не ассертится, только count 4 таблиц | low | Потребителей формы ещё нет; каждую таблицу пиннует её сторис (1.2+). → отклонено |
| 5 | verification-gap | rabbitmq.conf, CI | quorum-дефолт не наблюдается CI (healthcheck не различает queue type) | low | Потребителей queue type нет до Story 1.4; ассерт придёт со сторис-владельцем. → отклонено |
| 6 | blind | vite.config.ts | Нет server.proxy — dev-запрос /health падает в Vite (SPA-fallback 200 → ложный gateway-up), комментарий в App.tsx ссылается на несуществующий прокси | medium | Подтверждено: proxy отсутствует; App комментирует «Vite-прокси». → patch: proxy /health,/ready → :8080 |
| 7 | blind | README.md | README приписывает `npm run build` запуск size-гейта, которого в скрипте нет | low | Подтверждено: build = tsc+vite, гейт отдельно. → patch: формулировка README (гейт — CI/отдельной командой) |
| 8 | blind | ci.yml | FAIL-пути гейтов не исполняются в CI (дубль №1) | medium | См. №1. → patch (та же группа) |
| 9 | blind | bundle-size.mjs | Fallback без module-ссылок суммирует ВСЕ assets (включая lazy) и лишь предупреждает — противоречит собственному контракту заголовка | low | Подтверждено: fallback суммирует assets/*.js с warn. → patch: fallback → fail; вместе с №17/18 |
| 10 | blind | hexagonal-lint.mjs | Гейт не покрывает независимость platform (без axum/sqlx) и триггерится на упоминания маркеров в комментариях Cargo.toml | low | Платформа — shared kernel вне периметра AD-1 (intent гейта — core↔внешние слои) → эта часть отклонена; часть про комментарии реальна. → patch: чистка комментариев перед сопоставлением |
| 11 | blind | CI/rust | Нет `cargo fmt --check`; rustfmt установлен, но не enforced | low | Мульти-агентная разработка гарантированно дрейфует по стилю. → patch: шаг fmt --check |
| 12 | blind | CI/rust | Сборки без `--locked` — CI может молча обновлять закоммиченный Cargo.lock | low | Подтверждено: флага нет нигде. → patch: --locked в build/clippy/test |
| 13 | blind | gateway | Мёртвый dep uuid; signal-фича включена, graceful shutdown отсутствует | low | uuid подтверждён неиспользуемым → patch: убрать dep. Graceful shutdown — не intent seed'а (CI-kill на эфемерном раннере) → отклонено |
| 14 | blind | config.rs | from_env без тестов веток (missing/empty DATABASE_URL, невалидный порт/env) | low | Подтверждено: единственный тест config — environment_labels_parse. → patch: тесты веток (группа с №22–24) |
| 15 | blind | events.rs | AD-22 «past tense» не enforce'ится — валидатор проверяет только snake_case, доко-коммент обещает больше | low | Морфология past tense нестандартизируема без хрупкой эвристики; честный фикс — зафиксировать проверяемое подмножество в доке. → patch: правка доко-коммента |
| 16 | blind | 0002_users | UNIQUE(org, external_ref) с NULL не различает дубли unlinked-юзеров; нет комментария о семантике | low | NULL-дубли безвредны до линковки; семантику линковки пиннует Story 1.3; композитный unique уже даёт tenant-lookup. → отклонено |
| 17 | blind | compose | Healthchecks без start_period (медленная first-init PG может вписаться в лимит впритык); valkey --appendonly противоречит «эфемерности» | low | start_period реален (anti-flake) → patch: добавить start_period. appendonly — не противоречие: AD-28 требует ephemeral state В Valkey, а не non-persistence Valkey → отклонено |
| 18 | blind | ci.yml | Нет permissions: contents: read и timeout-minutes — широкий GITHUB_TOKEN и 6-часовой дефолт при зависании | low | Стандартное упрочнение, прямой фикс. → patch |
| 19 | blind | ci.yml integration | /ready → 503 not_ready при недоступной БД не ассертится (покрыта только /health) | low | Готовность half-covered. → patch (вместе с №2) |
| 20 | blind | SPA | Тестов SPA нет вообще (дубль №3) | medium | См. №3. → patch (та же группа) |
| 21 | blind | спека Code Map | Противоречие «не изменять» vs «Edit freely» в epic-1-context.md; erratum ../../ живёт в Code Map | low | Фикс = правка спеки (запрещено триажем); уже митигировано Implementation Notes + worklog. → отклонено |
| 22 | edge | ci.yml:116-128 | При провале degraded-ассерта bash -e обрывает шаг ДО `start postgres` — misleading-логи в уже красной джобе | low | Подтверждено (bash -e). Фикс прямой (reorder/‖ true). → patch |
| 23 | edge | main.rs run_migrations | Перманентная ошибка миграций → бесконечный ретрай | low | Громко: /ready stuck not_ready + лог каждые 5с; reachable только с битыми будущими миграциями, которые ловит CI; фикс добавляет классификацию ошибок (не прямой). → отклонено |
| 24 | edge | config.rs:70 | NotUnicode порт молча → дефолт 8080 | low | Подтверждено (Err(_) ловит всё). → patch: явная ветка InvalidEnv |
| 25 | edge | config.rs | PULT_HTTP_PORT=0 валиден → случайный порт, health-checks мимо | low | Подтверждено: u16 принимает 0. → patch: reject 0 |
| 26 | edge | config.rs | DATABASE_URL с пробелами сохраняется нетримнутым → lazy-фэйл в рантайме | low | Подтверждено: trim только для empty-проверки. → patch: хранить trimmed |
| 27 | edge | events.rs | Deserialize обходит validate_event_type (контракт только на construct-пути) | low | Достижимого продюсера кривых событий сегодня нет (все — через new()); фикс = кастомный Deserialize (не прямой). Владелец read-path — Story 1.4. → отклонено, заметка для 1.4 |
| 28 | edge | hexagonal-lint.mjs | path-dep `conveyor/...` с переименованным ключом крейта проходит гейт | medium | Подтверждено: маркера 'conveyor/' нет. → patch (группа robustness гейта) |
| 29 | edge | hexagonal-lint.mjs | Пустой core/ → PASS без единой проверки | low | Подтверждено: пустой цикл → PASS. → patch: guard exit(2) |
| 30 | edge | bundle-size.mjs | TYPE=module в верхнем регистре не распознаётся (regex case-sensitive) | low | Vite генерит lowercase, но гейт должен быть честным. → patch: /i (группа №9) |
| 31 | edge | bundle-size.mjs | module-ref на внешний URL/не-.js молча выпадает из бюджета | low | Подтверждено: filter endsWith('.js'). → patch: fail на неклассифицируемом ref |
| 32 | edge | Skeleton.tsx | lines>5 даёт отрицательную width | low | Достижимо только будущим вызовом; фикс — clamp, прямой. → patch |
| 33 | edge | App.tsx | /health с кодом ≠200/503 (напр. 404) помечается gateway-degraded | medium | Подтверждено: res.ok false → degraded; вместе с №6 — dev-режим показывает фикцию. → patch (та же группа: явные коды) |
| 34 | edge | conveyor/* | Claim: таск-чекбокс обещает ports.rs и в conveyor, а их там нет | false | Опровергнуто Code Map и AC: ports.rs — только core/*/ports.rs (AR-2); conveyor — adapter-сторона, портов не объявляет. Гексагон соблюдён |
| 35 | edge | CI up --wait | Лог упавшего подъёма контейнеров не диагностируется в джобе | low | Уже закрыто фикс-коммитом 8b673dd (диагностический шаг compose logs при failure). carried: закрыто |

Итог группировки: без intent_gap и bad_spec — лупбэка нет. Выжившие маршрутизируются в **patch** (группы: №1/8 гейт-самотесты; №6/33 dev-прокси; №2/19 /ready-ассерты; №3/20 SPA-тест; №10/28/29 robustness hex-lint; №9/30/31 честность bundle-гейта; №22 старт postgres; №24/25/26/14 config; №11/12 fmt+locked; №17 start_period; №18 permissions/timeout; №7 README; №13 uuid; №15 доко events; №32 clamp). Отклонены: №4,5,13(shutdown),16,21,23,27,10(platform-часть),17(appendonly-часть),34.

## Design Notes

- UUIDv7: конвенция — на слое приложения (uuid crate `now_v7()`); дефолт в DDL — `gen_random_uuid()` как fallback (без ставки на неподтверждённую в этой сессии нативную функцию PG 18).
- RabbitMQ quorum: `default_queue_type = quorum` в rabbitmq.conf; management-плагин только для dev-отладки.
- Sandbox без docker: integration-AC верифицируется job-ой CI (ubuntu-latest имеет docker); локально — `docker compose config` недоступен, структура проверяется ревью + CI.
- CI-кэш: `Swatinem/rust-cache@v2` + `actions/setup-node` c npm-кэшем; суммарное время CI целить ≤ 10 мин.

## Verification

**Commands:**
- `cargo build --workspace` (в `pult/`) -- expected: 0 ошибок, 13 крейтов собраны
- `cargo clippy --workspace --all-targets -- -D warnings` -- expected: 0 предупреждений
- `cargo test --workspace` -- expected: тесты platform-конвертов зелены
- `npm ci && npm run build` (в `pult/shells/bitrix24/`) -- expected: dist собран
- `node platform/scripts/bundle-size.mjs pult/shells/bitrix24/dist 200000` -- expected: PASS
- `node platform/scripts/hexagonal-lint.mjs` и `node platform/scripts/valkey-grep.mjs` -- expected: PASS
- `bash -n`/`docker compose config` недоступен локально -- expected: проверка в CI job `integration`

**Manual checks (owner/CI):**
- CI-лог: все 4 jobs зелены на push в main
- Установка из Маркета Б24 в тестовый портал -- владелец, после регистрации приложения (внешняя зависимость, вне кода)
