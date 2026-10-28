# pult/ — монорепа «Пульт селлера» (Structural Seed, Story 1.1)

Стартовый каркас по spine: hexagonal-ядро + событийный конвейер + оболочки.
Стек зафиксирован (AD-20): Rust stable 1.99.1 (`rust-toolchain.toml`), Tokio 1.54.1,
Axum 0.8.9, SQLx 0.9.0; SPA — React 19 + Vite 8 + TypeScript. Кэш — только
Valkey 9.1, образы конкурента запрещены CI-грепом (AD-28).

## Структура

```
pult/
├── gateway/            # бин pult-gateway: /health, /ready, миграции на старте
├── platform/           # общий язык доменов: конверты AD-22/ошибок, конфиг
│   └── scripts/        # CI-гейты: hexagonal-lint, valkey-grep, bundle-size
├── core/               # 7 доменов ядра (AD-1: не зависят от conveyor/shells)
│   ├── sync/  data/  finance/  shturman/  rules/  billing/  identity/
├── conveyor/           # конвейер: adapters, action_bus, reconciliation, workers
├── shells/bitrix24/    # SPA-оболочка Б24: экран «Начните подключение»
├── db/migrations/      # forward-only миграции sqlx migrate (0001..0004)
└── deploy/             # dev-compose: PG 18 + RabbitMQ 4.4 (quorum) + Valkey 9.1
```

## Запуск dev-стека одной командой

```bash
docker compose -f pult/deploy/docker-compose.yml up -d --wait
```

Поднимутся PostgreSQL 18 (`pult/pult`, порт 5432), RabbitMQ 4.4 с quorum-очередями
по умолчанию (AMQP 5672, management UI 15672) и Valkey 9.1 (RESP 6379).
Данные — в именованных томах: повторный `up` данные не ломает.

## Запуск шлюза (миграции применяются автоматически)

```bash
cd pult
DATABASE_URL=postgres://pult:pult@localhost:5432/pult cargo run -p pult-gateway
# проверки:
curl -s localhost:8080/health   # {"status":"ok","version":"0.1.0"}
curl -s localhost:8080/ready    # {"status":"ready",...,"migrations_applied":true}
```

`/health` проверяет БД через sqlx-пул: 200 `ok` или 503 `degraded` (если БД
недоступна — шлюз остаётся живым, миграции повторяются в фоне).

## SPA (оболочка Битрикс24)

```bash
cd pult/shells/bitrix24
npm ci
npm run dev      # локальный dev-режим, :5173 (/health и /ready проксируются на шлюз :8080)
npm run test     # автотесты точки интеграции Б24 (vitest)
npm run build    # только сборка dist/ — гейт размера запускается отдельно
```

Гейт размера критического пути (AD-30) — отдельная команда
`node platform/scripts/bundle-size.mjs shells/bitrix24/dist 200000` (в CI — job spa);
`npm run build` его не запускает.

## Локальные гейты

```bash
cd pult
cargo build --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
node platform/scripts/hexagonal-lint.mjs                              # AD-1
node platform/scripts/valkey-grep.mjs                                 # AD-28
node platform/scripts/bundle-size.mjs shells/bitrix24/dist 200000     # AD-30
```

## Конвенции (spine)

- Деньги — integer-копейки; время — timestamptz UTC; ID — UUIDv7 (слой приложения).
- Ошибки API — конверт `{error:{code,message,request_id,details}}` (`pult-platform::error`).
- События — конверт AD-22 (`pult-platform::events`).
- Миграции — forward-only `sqlx migrate`; таблицы создаёт сторис под собственные
  нужды (Story 1.1 — только organizations, users, role_assignments, portal_settings).
- Никаких паролей/собственной аутентификации — вход только через контур Б24 (NFR-6).
