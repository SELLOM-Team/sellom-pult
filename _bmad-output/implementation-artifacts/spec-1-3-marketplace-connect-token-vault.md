---
title: 'Story 1.3 — Мастер подключения площадок с token-vault (шаги 1–3)'
type: 'feature'
created: '2026-10-29'
status: 'done'
route: 'dispatch'
review_loop_iteration: 1
baseline_commit: 'f456aba'
story_key: '1-3-мастер-подключения-площадок-с-token-vault-шаги-1-3'
context:
  - '_bmad-output/implementation-artifacts/epic-1-context.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** селлер, прошедший шаг 0, не может подключить кабинеты Wildberries и Ozon: нет мастера ввода токенов с автотестом прав, нет безопасного хранения — токены API в БД и логах, уязвимы к утечке (FR-2, AD-16; без этого Story 1.4 не начинается — синк требует подтверждённые токены WB).

**Approach:** мастер в SPA (шаг 1 — выбор площадки; шаг 2 — токен и схемы; шаг 3 — автотест прав с человекочитаемой диагностикой) + token-vault: gateway принимает токен, пробует права через API площадки (лёгкие запросы-пробы), при успехе шифрует AES-256-GCM (KMS-конверт, AD-16) и сохраняет метаданные подключения; расшифровка в Ф0 нигде не вызывается (write-only vault — единственный будущий потребитель расшифровки — адаптеры Epic 2). Обе площадки подключаются независимо в одном портале.

## Boundaries & Constraints

**Always:**
- Токен в БД ТОЛЬКО шифротекстом (AES-256-GCM, ключ 256 бит, уникальный nonce на запись, key_version для ротации ключа); Ozon Client-Id — не секрет, хранится открыто.
- Автотест прав ДО сохранения: 200 пробы = право подтверждено; 401/403 = человекочитаемая диагностика (какое право не подтвердилось, что сделать); таймаут/недоступность площадки — честная ошибка, не «подключено». Неуспешный автотест токен НЕ сохраняет.
- Вход через gateway (AD-13), контракты `/v1`, error-конверт, budget-паттерн 1.2; идентичность — placement-контекст (dev-фоллбэк как в 1.2). Токен никогда не попадает в логи (уровень warn!/info! — только метаданные и маски).
- Ротация = повторный POST с новым токеном (перезапись шифротекста + verified_at; ключи/версии журналируются без секретов). Повторный POST по той же площадке — перезапись, не дубль (UNIQUE(organization_id, marketplace): Ф0-квота — 1 кабинет на площадку в портале; несвязанные кабинеты — разные порталы).
- Первая UPDATE-сторис (deferred 1.1): механизм updated_at решается здесь — триггер `set_updated_at()` на таблице подключений в миграции.

**Never:**
- Никакого расшифрования в коде Ф0 (нет decrypt-вызовов в проде-пути; decrypt-порт существует для Epic 2 и тестируется юнит-тестами).
- Никаких plaintext-колонок токенов в миграциях (CI-grep) и логов (маскирование).
- Никакого синка/конвейера/RabbitMQ/адаптеров площадок — Story 1.4 и Epic 2. Никакого B24-хендшейка — его нет в epics.md Story 1.3 (write-on-GET defer остаётся открытым до отдельного решения).
- Никакого серверного enforcement ролей (механизм ролей — Story 1.6): зона «только Владелец» маркируется в UI честной пометкой.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Статус подключений | GET /v1/connections | 200 `{connections:[{marketplace, schemes, status, verified_at, rights}]}` — без секретов; пусто до подключения | БД недоступна → 503-конверт |
| Автотест ok | POST /v1/connections `{marketplace:"wildberries", schemes:["fbo","fbs","realfbs"], token:"…"}` | пробы площадки 200 → 200 `{status:"verified", rights:[{name, ok:true}]}`; шифротекст в БД, plaintext — нигде | — |
| Автотест отказ прав | токен жив, но проба 401/403 | 200 `{status:"rejected", checks:[{name, ok:false, hint:"человекочитаемо"}]}`; токен НЕ сохранён | — |
| Неверный формат | пустой токен, unknown marketplace, пустые schemes, Ozon без client_id | 422-конверт (валидация до пробы и БД) | коды: invalid_token/unknown_marketplace/invalid_schemes/invalid_client_id |
| Площадка недоступна | пробы таймаут/5xx | 200 `{status:"probe_unavailable", checks:[{name, ok:false, hint:"площадка не ответила — повторите"}]}`; не сохранено | таймаут пробы 10с |
| Повторное открытие мастера | GET /v1/connections при существующих | мастер показывает статусы обеих площадок; форма не сбрасывает подключённую площадку | — |
| Ротация токена | POST по уже подключённой площадке с новым токеном | автотест → перезапись шифротекста, verified_at, rights | проверка до записи: при отказе прав прежний шифротекст сохраняется |
| Удаление | DELETE /v1/connections/{marketplace} | 200; строка удалена (токен отозван) | — |

</frozen-after-approval>

## Code Map

- `pult/core/identity/src/vault.rs` (новый) — чистый домен: ConnectionStatus/RightsCheck/правила валидации схем по площадке (WB: fbo/fbs/realfbs; Ozon: fbo/fbs/rfbs); порты в `ports.rs` — TokenCipher (encrypt/decrypt байт + key_version), ConnectionStore (save/load/delete), RightsProbe (по площадке → список проверок).
- `pult/gateway/src/connections.rs` (новый) — роутер `/v1/connections*`, PgConnectionStore (реализует порт; INSERT ON CONFLICT → перезапись), EnvAesGcmCipher (ключ из конфига, hex; nonce сгенерируется), HttpRightsProbe (реализация пробы; URL/методы — конфиг-константы с веб-верификацией при имплементации; кандидаты: WB statistics-api…/api/v1/supplier/stocks?dateFrom=today, discounts-prices-api…/api/v2/list/goods/filter?limit=1; Ozon api-seller.ozon.ru POST /v3/product/list limit=1), маскирование токена (show_last4).
- `pult/platform/src/config.rs` — переменная PULT_VAULT_MASTER_KEY (обязательна: hex 64 символа = 32 байта; невалидная — отказ старта; отсутствие в dev — явная ошибка с инструкцией генерации).
- `pult/db/migrations/0006_marketplace_connections.sql` (новый) — таблица (id uuid, organization_id FK, marketplace CHECK IN(wildberries,ozon), schemes jsonb, status CHECK IN(verified,rejected,probe_unavailable), client_id text NULL, token_ciphertext bytea NOT NULL, token_nonce bytea NOT NULL, key_version int NOT NULL DEFAULT 1, rights_snapshot jsonb, verified_at timestamptz, created_at/updated_at + триггер set_updated_at, UNIQUE(organization_id, marketplace)).
- `pult/shells/bitrix24/src/screens/connect/` (новые) — шаги 1–3 мастера: выбор площадки → форма (WB: токен; Ozon: Client-Id + Api-Key; чекбоксы схем) → результат автотеста (права ok / диагностика / honest-ошибка); статус обеих площадок на StartConnection.
- `pult/shells/bitrix24/src/api.ts` — клиент connections (JSON-семантика 1.2, error-конверт, таймаут).
- `pult/platform/scripts/` — гейт vault-grep: plaintext-колонки токенов в миграциях запрещены (по образцу valkey-grep).
- `.github/workflows/ci.yml` — integration: живая проба наружу невозможна в CI → ассерты: POST невалидного формата → 422 до пробы/БД; POST валидного формата → `probe_unavailable` и GET остаётся пустым (инвариант «неуспешная проба не сохраняет»); DELETE-семантика; PULT_VAULT_MASTER_KEY — фиксированный test-ключ в шагах запуска шлюза (иначе конфиг не пустит старт).
- Продолжение: спека spec-1-2 (done) — паттерны бюджета/конвертов/SPA-тестов; deferred-work.md (updated_at — закрывается для этой таблицы).

## Tasks & Acceptance

**Execution:**
- [x] `pult/db/migrations/0006_marketplace_connections.sql` — таблица + триггер updated_at + UNIQUE
- [x] `pult/platform/src/config.rs` — PULT_VAULT_MASTER_KEY (валидация hex/32 байта, тесты веток)
- [x] `pult/core/identity/src/vault.rs` + `ports.rs` — домен: валидация схем, статусы, права-проверки; порты Cipher/Store/Probe; юнит-тесты правил
- [x] `pult/gateway/src/connections.rs` — хендлеры GET/POST/DELETE, PgConnectionStore, EnvAesGcmCipher (aes-gcm crate), HttpRightsProbe (reqwest, rustls, таймаут 10с, без логирования секретов), маски show_last4; тесты: шифрование round-trip, уникальный nonce, валидации, конверты, мок-проба (ok/403/таймаут)
- [x] `pult/gateway/src/main.rs` — подключение роутера, конфиг-ключ в AppState
- [x] `pult/shells/bitrix24/src/screens/connect/*` — мастер шагов 1–3 + статус на StartConnection; `api.ts` + тесты; чистые функции мастера (валидация форм, состояние шагов) с vitest
- [x] `pult/platform/scripts/vault-grep.mjs` — гейт: `token_plaintext`/plaintext-паттерны в db/migrations = exit 1; самотест в CI (PASS + FAIL-направление)
- [x] `pult/README.md` — эндпоинты, переменная PULT_VAULT_MASTER_KEY (генерация openssl rand -hex 32), семантика статусов; `.github/workflows/ci.yml` — vault-grep в job gates (PASS+FAIL), CI-ассерты /v1/connections (422; probe_unavailable → GET пусто; DELETE), ключ в шагах запуска шлюза
- [x] Живая проверка шлюза оркестратором: обязательность ключа (чистый exit 1 с инструкцией), 422-валидация без БД/пробы, 503-конверт деградации GET/POST
- [ ] Live-проба прав — внешняя верификация владельца (реальные токены WB/Ozon; CI наружу не ходит)

**Acceptance Criteria:**
- Given токен с правами, when POST /v1/connections, then пробы возвращают список подтверждённых прав, шифротекст в БД, plaintext отсутствует в БД/логах/ответе.
- Given токен без прав, when POST, then `status:"rejected"` с человекочитаемой диагностикой по каждой пробе, в БД изменений нет.
- Given обе площадки, when подключены в одном портале, then GET /v1/connections показывает обе независимо (Ozon — с client_id, схемы fbo/fbs/rfbs).
- Given повторный POST по подключённой площадке (ротация), when автотест ок, then шифротекст/verified_at/rights перезаписаны; при отказе прав — прежние данные целы.
- Given миграция 0006, when любой UPDATE строки, then updated_at обновляется триггером (проверено CI-psql).
- Given CI, when gates/integration, then vault-grep PASS+FAIL направления; 422-валидация, GET, DELETE /v1/connections зелены; гейты 1.1 (hexagonal/valkey/bundle) не сломаны.

## Implementation Notes

- Решения при делегировании владельцем (как 1.2): vault — модуль домена identity (7 крейтов спина не расширяется); KMS-конверт Ф0 = порт KeyProvider + env-ключ (YC KMS-адаптер — deferred до провижининга); расшифровка в проде-пути отсутствует (write-only vault); пробная матрица — конфиг-константы с обязательной веб-верификацией эндпоинтов при имплементации (Директива 1; верифицировано 29.10.2026: WB — категории данных токена по dev.wildberries.ru, Ozon — Client-Id/Api-Key по api-seller.ozon.ru).
- CI не может прогнать живую пробу: e2e внешних вызовов нет — автотест верифицируется моками (юнит) + живьём владельцем. Порядок honest-ответа: сетевые проблемы площадки ≠ отказ прав — отдельный статус probe_unavailable.
- 2026-10-29, имплементация: dev-субагент шага 03 прерван по таймауту сессии после ~100% бэкенда (127 Rust-тестов, гейты); оркестратор дозакрыл SPA (мастер 3 шагов, статусы на StartConnection, api-клиент, 80 SPA-тестов), CI (vault-grep гейт, e2e, ключ) и README. Верификация: 132 Rust + 80 SPA, гейты PASS (74 891 Б gzip), живьём: exit 1 без ключа с инструкцией, 422-валидация без БД/пробы, 503-деградация.

## Spec Change Log

## Review Triage Log

Ревью 29.10.2026, 2 прогона × 3 слоя (первый — по неполному диффу без untracked-файлов, дефект стейджинга оркестратора, исправлен; итог — по полному диффу 4663 строки): blind-hunter 15 / edge-case-hunter 10 / verification-gap 3+2 (pre-verified) → 30 уникальных находок → **20 патч-групп применены / 3 отклонено / 7 слияний дубликатов**. Лупбэков нет (0 intent_gap / 0 bad_spec). Отклонения:

- Нет CI-проверки «ключ не задан → exit 1» — отклонено: покрыто юнит-тестами config (MissingVaultKey) + живой проверкой оркестратора (записана в задачах); CI-негатив старта — дрейф-риск ради маржинальной ценности.
- Spec GET-форма без client_id / пустой Spec Change Log — отклонено: фикс = правка спеки (запрещено правилом триажа); README/ConnectionDto — действующий контракт, client_id задокументирован в Code Map.
- Хардкод #b3261e в styles.css — отклонено (false): тот же литерал в .form-error с Story 1.2 — существующий паттерн, не регрессия.

Ключевые патчи (полный список — Implementation Notes): Debug-маскирование ключей; from_hex ASCII; prod-запрет override; summarize(пусто); COALESCE; бюджет POST 30с; ротация/выход из мастера; vault-grep 3 класса; CI-лайфсайкл с заглушкой проб.


## Review Findings (формальный bmad-code-review, 2026-10-29, вечер)

4 слоя (blind 15 / edge 10 / VG 3+2 pre-verified / acceptance-auditor: 0 нарушений frozen-интента + 5 низкосеверити наблюдений и 5 «принятых аддитивных отклонений») → 30+ находок, после вычета уже-применённых build-патчей и слияний → **12 патч-групп применены / 3 defer / 4 отклонено**. Ключевые: CI-шаг лайфсайкла без резолва бинаря из корня (HIGH — вся верификация persistence-пути не исполнялась бы); plaintext-токены не грепались в логе шлюза; Ozon verified-путь (client_id round-trip) не исполнялся e2e (AC-3); chunked-ответ площадки буферизовался целиком (cap только при Content-Length); UI звал любую строку «подключена» (честность статусов); Ozon 400-маркер на 5xx = ложный rejected; override принимал путь/query/userinfo; клиентский паритет валидации (длина/control); многострочные блочные комментарии и составные имена (ozon_api_key) в vault-grep; гонка навигации мастера (generation); apiError после «Назад»; readiness-полл заглушки. Отклонено: глубокая валидация элементов парсера (same-origin доверие, серверные тесты фиксируют форму); catch-all mismatched-credentials (недостижим по построению connect_request_from_body); 500 на единственную повреждённую строку GET (дизайн CorruptData 1.2); CI-клинап заглушки (свежий раннер на прогон). Defer: AAD-привязка шифротекста к (org, marketplace) — до Epic 2, retrofit через re-encrypt; zeroize ключей; advisory-lock на POST/DELETE гонку (ресуррект отозванного токена).

## Design Notes

- Шифротекст/nonce — bytea парой; key_version → будущая ротация master-ключа без перешифрования всех строк (re-encrypt job — Ф1+, записано в deferred).
- Проба возвращает список проверок (по одной на право-категорию), а не булев флаг — UI показывает таблицу прав, отказ — конкретную подсказку («в кабинете WB пересоздайте токен с категорией „Цены и скидки"»).
- Депс: aes-gcm (RustCrypto), reqwest (default-features=false, rustls-tls, json) — только в gateway.

## Verification

**Commands:**
- `cargo test --workspace && cargo clippy --workspace --all-targets --locked -- -D warnings` (в `pult/`) -- expected: чисто; новые тесты identity/gateway зелены
- `npm run test && npm run build` (в `pult/shells/bitrix24/`) -- expected: vitest зелены, dist собран
- `node platform/scripts/vault-grep.mjs` + гейты 1.1 -- expected: PASS; FAIL-направления exit 1
- Живой шлюз без БД: /v1/connections → 503-конверт; валидация 422 без касания БД/пробы

**Manual checks (owner/CI):**
- Живой автотест прав реальными токенами WB/Ozon (внешняя верификация, требует выход в интернет)
- После живого подключения: psql — введённая подстрока токена НЕ встречается в token_ciphertext/token_nonce; в логах шлюза подстроки нет
