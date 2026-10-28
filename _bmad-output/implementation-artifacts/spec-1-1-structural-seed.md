---
title: 'Story 1.1 — Стартовый каркас приложения и Structural Seed'
type: 'feature'
created: '2026-10-28'
status: 'in-progress'
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
- [ ] `pult/Cargo.toml`, `rust-toolchain.toml`, `.gitignore`-дополнения -- воркспейс на 13 крейтов, edition 2024, пины версий AD-20 -- единая точка сборки
- [ ] `pult/platform/` -- конверты событий/ошибок/конфиг -- общий язык доменов
- [ ] `pult/core/*` (7 крейтов) + `pult/conveyor/*` (4 крейта) -- lib + ports.rs заготовки -- AR-2/AR-3
- [ ] `pult/gateway/` -- Axum-бин: `/health`, `/ready`, миграции на старте -- проверяемый жизненный цикл
- [ ] `pult/db/migrations/0001..0004` -- 4 таблицы, FK users→organizations, role_assignments→organizations/users, portal_settings→organizations -- только нужды Story 1.1
- [ ] `pult/deploy/` -- compose + rabbitmq.conf (quorum) -- AC «compose поднимает стек»
- [ ] `pult/platform/scripts/` -- 3 гейт-скрипта -- AD-1/AD-28/AD-30 как исполняемые проверки
- [ ] `pult/shells/bitrix24/` -- SPA-каркас: entry «Начните подключение», health-заглушка, скелетон -- AC SPA < 200 КБ gzip
- [ ] `.github/workflows/ci.yml` -- 4 jobs со всеми гейтами -- AC CI
- [ ] `pult/README.md` -- запуск dev-стека одной командой -- DX следующего агента

**Acceptance Criteria:**
- Given чистая машина с rustup, when `cargo build --workspace` в `pult/`, then сборка проходит без ошибок; `cargo clippy --workspace -- -D warnings` и `cargo test --workspace` чисты.
- Given `docker compose up -d --wait` в `pult/deploy/`, when стек поднялся, then PG 18, RabbitMQ 4.4 (quorum) и Valkey 9.1 (`valkey/valkey`) здоровы; повторный `up` не ломает данные.
- Given gateway запущен с DATABASE_URL на чистую БД, when применены миграции, then существуют organizations, users, role_assignments, portal_settings; `/health` → 200.
- Given CI на push, when все jobs отработали, then build/clippy/test, hexagonal-lint, Valkey-греп, bundle-size (<200 КБ gzip) и integration-проверки зелены; PR с `image: redis` или импортом adapters в core -- красный.
- Given SPA `npm run build`, when замерен критический путь, then gzip JS < 200 000 байт; каркас открывается в iframe-режиме с экраном «Начните подключение».
- Верификация «установка из Маркета Б24» -- внешняя, владелец (нужен dev-аккаунт Б24); код поставляет iframe-вход + локальный dev-режим.

## Implementation Notes

## Spec Change Log

## Review Triage Log

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
