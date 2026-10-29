# MEMORY — постоянные директивы агента

> Этот файл — долговременная память. В КАЖДОЙ новой сессии он читается первым действием,
> вместе с `/home/z/my-project/worklog.md`. Изменять только дополнением/актуализацией.

---

## Директива 1: Актуальность и точность данных и фактов (ПРИОРИТЕТ ВЫСШИЙ)

**Проблема:** знания модели имеют дату среза обучения и могут устареть; часть информации
изменяется во времени и без проверки становится ложной.

**Что чувствительно ко времени (всегда проверять перед использованием):**
- версии пакетов, библиотек, инструментов, ОС и их совместимость
- цены, тарифы, квоты, лимиты сервисов
- даты релизов, статусы проектов, события, новости
- API, команды и синтаксис инструментов (могли измениться)
- законы, требования, стандарты, статистика

**Обязательные правила:**
1. ЗАПРЕЩЕНО выдавать устаревшее как актуальное.
2. ЗАПРЕЩЕНО придумывать факты, цифры, ссылки, цитаты, названия версий (галлюцинации).
3. Любая чувствительная ко времени информация ПРОВЕРЯЕТСЯ через WebSearch/WebFetch
   ДО её использования в ответах, документах и коде.
4. Если проверка невозможна — прямо сообщить пользователю и пометить данные
   как «требует проверки», а не выдавать за достоверные.
5. В ответах указывать дату проверки данных, если они внешние по отношению к проекту.
6. Версии внутри проекта считать актуальными, только если подтверждены в текущей
   сессии (например: `npm view <pkg> version`, `bmad status`, официальный реестр/репозиторий).

---

## Директива 2: Непрерывность работы (сессия может оборваться В ЛЮБОЙ МОМЕНТ)

**Проблема:** сессия может сброситься внезапно. Потеря прогресса недопустима.

**Решение:** трёхуровневое сохранение состояния:
1. `worklog.md` — детальный журнал шагов (дописывается ПОСЛЕ каждого значимого шага, не в конце);
2. раздел «Текущее состояние» ниже — снимок статуса (обновляется в конце сессии и на вехах);
3. `git` — коммиты после каждой вехи (файлы в git надёжнее любых журналов).

**Протокол НАЧАЛА сессии (выполнять по порядку, до ответов пользователю по сути работы):**
1. Прочитать `/home/z/my-project/MEMORY.md` (этот файл).
2. Прочитать `/home/z/my-project/worklog.md` — найти последний Task ID и незавершённое.
3. Выполнить `git -C /home/z/my-project status --short` и `git -C /home/z/my-project log --oneline -5`
   — сверить фактическое состояние файлов с журналом (журнал может оборваться раньше сбоя).
4. Продолжить ровно с места остановки; завершённое НЕ переделывать.

**Протокол ВО ВРЕМЯ работы:**
- После каждого значимого шага (установка, настройка, артефакт, решение, ошибка+обход)
  — немедленно дописывать секцию в `worklog.md` по шаблону (Task ID, Agent, Task, Work Log, Stage Summary).
- После завершения вехи — `git add -A && git commit -m "<веха>"` в `/home/z/my-project`.
- Если сессия прерывается посреди задачи — следующая запись в worklog должна начинаться
  со строки «Возобновлено после сброса сессии; состояние на момент обрыва: …».

**Протокол ЗАВЕРШЕНИЯ сессии/задачи:**
- worklog.md и «Текущее состояние» отражают реальность;
- все изменения закоммичены;
- пользователю сообщён следующий шаг.

---

## Директива 3: Синхронизация с GitHub (единый источник прогресса)

**Репозиторий:** https://github.com/artemiosu/sellom-pult (ПРИВАТНЫЙ), ветка `main`, 
remote `origin` уже настроен в `.git/config` проекта.

**Правила коммитов и пушей (обязательны):**
1. После каждой вехи: `git add -A && git commit -m "<веха>" && git push origin main`
   — пушить СРАЗУ после коммита, не откладывать до конца сессии.
2. В конце каждой сессии: убедиться, что всё закоммичено И запушено.
3. Коммит-месседжи — на английском (conventional: feat/docs/chore/...), содержимое — на русском.

**Токен доступа (PAT fine-grained, права Contents: RW на artemiosu/sellom-pult):**
- Хранится ТОЛЬКО в `.git/config` в URL remote origin (локальный файл, не коммитится).
- ЗАПРЕЩЕНО записывать токен в ЛЮБЫЕ файлы проекта (MEMORY.md, worklog.md, docs/, код,
  скрипты) — они попадают в публичный коммит. Если токен случайно закоммичен — сообщить
  владельцу о необходимости ротации.
- В НОВОМ окружении (если `.git` потерян): запросить у владельца действующий PAT,
  затем `git clone https://<PAT>@github.com/artemiosu/sellom-pult.git /home/z/my-project`
  (или `git remote add origin ...` + fetch/merge).

**Протокол восстановления после потери сессии/окружения (дополняет Директиву 2):**
0. ПОЛНАЯ инструкция по всем сценариям (сброс сессии / смерть инструментов / полная гибель окружения) — `docs/RECOVERY.md` (создана 28.10.2026).
1. Если окружение живо (`.git` на месте): `git fetch origin && git status` — если локальное
   отстаёт от `origin/main`, сначала `git pull --rebase` (взять чужие правки), сверить с worklog.
2. Если окружение умерло: восстановить клона из GitHub (нужен PAT владельца), затем прочитать
   MEMORY.md → worklog.md → продолжить с места, отражённого в разделе «Текущее состояние».
3. Конфликты локального незапушенного и удалённого: приоритет у remote (более свежий пуш);
   незапушенный локал восстановить из worklog-описаний.

---

## Текущее состояние (обновлять на каждой вехе и в конце сессии)

- **Дата обновления:** 2026-10-29 (утро UTC+3), Task 19 закрыт: CI-гигиена (warnings Node 20 + notices Ubuntu 26 устранены, run #23 зелёный без предупреждений), RECOVERY.md дополнен плейбуком подъёма инструментов, rust-окружение песочницы восстановлено (4-й сбой)
- **Проект:** «Пульт селлера» — операционная система селлера маркетплейсов (WB/Ozon/ЯМ), 4 канала (Битрикс24 → amoCRM → retailCRM → standalone). Источник концепции: docs/Pult_Seller_Koncepciya_v1_2.md — ДЕЙСТВУЮЩИЙ ЭТАЛОН v1.2, подтверждён владельцем 27.10.2026; вся проектная работа опирается только на него.
- **GitHub:** artemiosu/sellom-pult (приватный), ветка main, синхронизирован (push после каждой вехи, Директива 3).
- **Стек-мандаты (обязательны ВСЕМ агентам при проектировании/разработке):** бэкенд ядра — Rust stable (Tokio/Axum/SQLx) — AD-20 (пересмотрен 28.10.2026 по требованию владельца: молниеносная скорость, минимальные ресурсы, 1000+ одновременных — AD-29); кэш-слой — **Valkey, НЕ Redis™** — мандат AD-28 (запрет Redis-образов CI-grep'ом; клиент redis-rs/valkey-glide допустим — RESP; сервер — всегда Valkey); фронтенд-бюджеты без фризов — AD-30.
- **Код:** монорепа `pult/` (Structural Seed, Story 1.1, готова — см. Task ID 11): 13 крейтов (gateway+platform+7 core+4 conveyor), миграции 0001..0004, dev-compose (PG 18/RabbitMQ 4.4 quorum/Valkey 9.1), гейт-скрипты platform/scripts, SPA shells/bitrix24, CI .github/workflows/ci.yml. Инструменты dev-окружения: rustup+toolchain 1.99.1 установлены в /home/z (`source ~/.cargo/env`); docker/PG в песочнице НЕТ — integration-верификация только через CI.
- **Выполнено:**
  - Task ID 1: BMAD METHOD v6.12.4 установлен и настроен (модуль bmm, claude-code + codex, языки RU).
  - Task ID 2: постоянная память (этот файл) + протоколы непрерывности.
  - Task ID 3: восстановление после сброса (BMAD-скиллы, .gitignore → `/skills/`); концепция зарегистрирована в docs/; создан Product Brief в _bmad-output/planning-artifacts/briefs/brief-pult-seller-2026-10-27/brief.md.
  - Task ID 4: настроена синхронизация с GitHub (remote origin + токен в .git/config, Директива 3), весь прогресс запушен в main.
  - Task ID 5: BMAD bmad-prd исполнен: PRD «Пульт селлера» Ф0 — статус final (35 FR/6 UJ/12 SM/11 контрактов; 1 critical + 6 high ревью устранены). Артефакты: _bmad-output/planning-artifacts/prds/prd-pult-seller-2026-10-27/ (prd.md, addendum.md, review-rubric.md, .memlog.md). Открытые вопросы PRD §8: глубина чтения Ozon (FBS/FBO), SLA-механика пула Б24, формат виджета, демо-данные, пороги, правила пула, WTP ИИ-ролей.
  - Task ID 6: BMAD bmad-architecture исполнен: ARCHITECTURE-SPINE.md — статус final, 27 AD (11 контрактов эталона [ADOPTED] + 16 архитектурных Winston), парадигма hexagonal + событийный конвейер, стек проверен вебом 27.10.2026 (Python 3.14.x/FastAPI 0.162.0/PG 18/ClickHouse 26.8 LTS/RabbitMQ 4.4.0/Redis 8.8.x, React 19/Vite 8), гейт: lint 0 + 3 параллельных ревьюера (рубрик, tech-currency, adversary) — 3 critical + ~10 high устранены (AD-22..27 добавлены, AD-5/14/15/16 ужесточены). Артефакты: _bmad-output/planning-artifacts/architecture/architecture-pult-seller-2026-10-27/ (ARCHITECTURE-SPINE.md, .memlog.md — 16 записей, reviews/×3).
  - Task ID 7: пересмотр стека spine по вопросам владельца: AD-20 → Rust (1.99.1/Axum 0.8.9/Tokio 1.54.1/SQLx 0.9.0, версии верифицированы 28.10.2026); добавлены AD-28 (Valkey, НЕ Redis — мандат для dev-агентов), AD-29 (ёмкость 1000 одновременных), AD-30 (бюджет производительности фронтенда); Redis→Valkey по всему spine; memlog +7 записей. Spine: 30 AD, status final.
  - Task ID 8: вопрос PRD 8.1 ЗАКРЫТ (владелец делегировал решение архитектору 28.10.2026): Ozon в Ф0 — чтение полной глубины ОБЕИХ схем FBO + FBS/rFBS (заказы, остатки, тарифы, выплаты), без обратной записи; PII FBS — под AD-16; [ASSUMPTION]→[CONFIRMED] в FR-2/FR-16/§6.1/§9, memlog PRD. Создана инструкция восстановления docs/RECOVERY.md (сценарии сбоя сессии/инструментов/окружения; ссылка — в Директиве 3).
  - Task ID 9: BMAD bmad-create-epics-and-stories исполнен (headless, шаги 01–04): _bmad-output/planning-artifacts/epics.md — status final, 9 эпиков / 39 сторис / карта покрытия 35/35 FR / 30 AD → 14 AR в AC. Эпики: 1 Подключение (FR-1..7), 2 Конвейер целостности (FR-8..12), 3 Панель (FR-13..15), 4 Канбан (FR-16..18), 5 Остатки (FR-19..21), 6 Финансы (FR-22..27), 7 Команда (FR-28..30 + роботы FR-18), 8 Штурман-лайт (FR-31..33), 9 Тарифы (FR-34..35). Structural Seed — Story 1.1; forward-зависимостей нет; валидация шага 04 в frontmatter.
  - Task ID 10: BMAD bmad-sprint-planning исполнен (headless): гейт готовности PASS; sprint-status.yaml сгенерирован (9 эпиков / 39 сторис / 9 ретро — все backlog). Артефакт: _bmad-output/implementation-artifacts/sprint-status.yaml.
  - Task ID 11–12: BMAD bmad-build исполнен (Story 1.1 «Стартовый каркас и Structural Seed»): Rust-воркспейс pult/ (13 крейтов: gateway+platform+7 core+4 conveyor, edition 2024, rust-toolchain 1.99.1, пины AD-20), sqlx-миграции 0001–0004 (organizations/users/role_assignments/portal_settings), dev-compose (PG 18 [volume /var/lib/postgresql — breaking change образа, web-верифицировано] + RabbitMQ 4.4 quorum + Valkey 9.1), гейт-скрипты AD-1/28/30, SPA React 19+Vite 8 (69,5 КБ gzip), CI 4 jobs. Ревью 3 слоями: 35 находок → триаж (25 patch / 9 откл / 1 carried), патчи применены. Верификация: fmt/clippy/–locked чисто, 20 Rust + 4 SPA тестов, гейты в обе стороны, CI зелёный. Коммиты 035a781→afd99c5. Спека spec-1-1-structural-seed.md → done. Урок: последовательность «[m» глотается в текстовом выводе инструментов — сверяться od -c.
  - Task ID 11: Story 1.1 «Стартовый каркас приложения и Structural Seed» РЕАЛИЗОВАНА по спеке spec-1-1-structural-seed.md: монорепа pult/ — workspace 13 крейтов (edition 2024, rust-toolchain 1.99.1, пины tokio 1.54.1/axum 0.8.9/sqlx 0.9.0); platform (конверты AD-22/ошибок/конфиг + 8 тестов); 7 core-крейтов с ports.rs-заготовками (AR-2); 4 conveyor-заготовки; gateway pult-gateway (/health 200 ok / 503 degraded с пробой БД под 500мс-таймаутом, /ready, forward-only миграции sqlx::migrate! с ретраем); миграции 0001..0004 (organizations/users/role_assignments/portal_settings, FK по спеке, без паролей — NFR-6); deploy docker-compose (postgres:18 + rabbitmq:4.4-management quorum + valkey/valkey:9.1, healthchecks, тома) + rabbitmq.conf; гейт-скрипты hexagonal-lint/valkey-grep/bundle-size (проверены в обе стороны: PASS на каркасе, FAIL на инъекциях); SPA shells/bitrix24 (Vite 8.3.5/React 19.3.1/TS 5.9.3, экран «Начните подключение», честная заглушка статуса, скелетон, точка интеграции PLACEMENT-параметров; gzip JS 69 534 Б < 200 000 — PASS); CI ci.yml на 4 jobs (rust/spa/gates/integration c проверкой degraded-режима и 4 таблиц через psql); README. Локальная верификация: cargo build/clippy -D warnings/test — чисто (11 тестов), npm run build + гейты PASS; sandbox без docker/PG — happy-path миграций и compose за CI-джобой integration.
  - Task ID 13: формальное код-ревью Story 1.1 (bmad-code-review, 4 слоя: blind-hunter/edge-case/verification-gap/acceptance-auditor; дифф 35c2b77...b0d927a без lock-файлов): 46 находок → 24 patch / 1 defer / 14 rejected. Патчи применены (владелец делегировал): db_reachable Ok(Ok(_)) — быстрая ошибка ≠ «БД жива»; составной FK (organization_id, user_id)→users(organization_id, id) — кросс-орг роли исключены схемой (0002/0003 отредактированы на месте — постоянных развёртываний нет); hex-гейт: скан корневого [workspace.dependencies] (алиас-обход), скип //-комментариев .rs, маркеры gateway/path-форм; valkey-гейт: закомментированные # image:/# FROM ловятся (zero-tolerance); bundle-гейт: отсутствующий чанк = exit 2, query-суффикс резолвится; gateway: connect_lazy-невалидный URL → чистый exit 1 с redact_url; min_connections(1); SPA: только JSON-ответ шлюза = «шлюз жив» (health.ts + 5 тестов); compose-порты 127.0.0.1; CI: HTTP-код-ассерты /health+/ready, sentinel-строка через повторный up, 2 новых негативных самотеста, --locked везде. Верификация: 25 Rust + 9 SPA тестов, все гейты в обе стороны, degraded/невалидный-URL живьём, YAML валиден. Спека: Review Findings + [x]-патчи + review_loop_iteration 1. Коммит a793efd. Sprint: 1-1 → done. Defer: updated_at-механизм (первая UPDATE-сторис обязана решить) — в deferred-work.md.
  - Task ID 14: BMAD bmad-build Story 1.2 «Шаг 0 — фильтр периметра ваш бизнес» (FR-1) полностью исполнена (спека ff6cd78 → имплементация 2b315fc → ревью 3 слоями 31 находка → 22 патча 0778180 → спека done, sprint review): core/identity — первая вертикаль (perimeter.rs: TradeModel/Verdict/decide_verdict + PerimeterStore-порт); gateway /v1 (onboarding/state, perimeter/verdict, metrics/perimeter; error-конверты на 404/405/422/503; бюджеты 5с на БД); миграция 0005 (perimeter_verdicts + organizations.b24_member_id, составной FK); SPA (вопросник/воронка/исходящий экран/заглушка+retry; роутинг через чистые screenForState/screenStateFromError/step0; VITE_PARTNER_*-слот); dev-фоллбэк dev-portal/dev-user вне метрик; CI e2e шага 0 (селлер/не-селлер/иммутабельность/деградация/метрика). 111 тестов (57 Rust + 54 SPA), гейты PASS (72 318 Б gzip). Dev-субагент шага 03 умер по таймауту (~90% готово — дозакрыто оркестратором). СБОЙ СРЕДЫ ~16:10: ~/.cargo, node_modules SPA, _bmad/render стёрты — восстановлены (rustup 1.99.1, npm ci, re-render). Defer +3: write-on-GET до хендшейка 1.3; HTTP-уровень Rust-тестов; «другая площадка» (1.3).
  - Task ID 15: формальный bmad-code-review Story 1.2 исполнен (4 слоя параллельно, модель сессии): 25 уникальных находок → 15 патч-групп применены / 1 defer / 6 rejected; AC-нарушений НЕТ (acceptance-auditor). Ключевые патчи: STORE_BUDGET 5с→2с (3×2с=6с < 8с клиента) + юнит-тест таймаута (tokio start_paused); placement-хардненинг — лимиты DOMAIN≤253/USER_ID≤64 + частичный контекст → 422 invalid_placement (вместо тихой записи в dev-фикстуру) + канонизация хвостовой точки; echo-гигиена (control-символы, кап rejection-текстов); clearTimeout ПОСЛЕ чтения тела; CHECK trade_model в 0005; валидация площадок только для селлеров; CI: 404/405/reserved-ассерты, живой dev-фоллбэк (200+step0+psql), repeat-POST обеих веток, --max-time 10; копирайт-честность (ЯМ «следом», канал партнёра/поддержки Б24). 117 тестов (62 Rust + 55 SPA), гейты PASS (72 568 Б gzip), живьём все конверты + деградация через бюджет. СБОЙ СРЕДЫ повторился (~11:35 29.10, ~/.cargo/~/.rustup стёрты) — восстановлен по RECOVERY.md за ~3 мин. Спека: Review Findings + review_loop_iteration 1; sprint 1-2 → done; deferred-work.md +1 (React-компонентные тесты).
  - Task ID 16: BMAD bmad-build Story 1.3 «Мастер подключения площадок с token-vault (шаги 1–3)» (FR-2, AD-16) исполнен (спека c3e4efe → имплементация → ревью 2 прогона × 3 слоя → патчи → спека done, sprint review): core/identity::vault — домен подключения (Marketplace/Scheme-матрица WB fbo/fbs/realfbs ↔ Ozon fbo/fbs/rfbs, Credentials WB-токен | Ozon client_id+api_key, summarize_probes: verified/rejected/probe_unavailable, пустая матрица ≠ verified); порты TokenCipher/ConnectionStore/RightsProbe/MasterKeyProvider; gateway connections.rs — GET/POST/DELETE /v1/connections, PgConnectionStore (INSERT ON CONFLICT DO UPDATE — ротация), EnvAesGcmCipher (aes-gcm 0.10.3, уникальный nonce, write-only vault), HttpRightsProbe (reqwest+rustls, таймаут 10с, потолок тела 64 КБ, пробы: WB statistics-api/stocks + discounts-prices/list/goods/filter, Ozon /v3/product/list); миграция 0006 (marketplace_connections + триггер set_updated_at — deferred 1.1 решён для этой таблицы; UNIQUE(org,marketplace)); SPA — мастер 3 шагов (connect/step.ts чистые функции + ConnectMaster + статусы на StartConnection; ротация из UI); token-vault Ф0: env-ключ PULT_VAULT_MASTER_KEY (обязательный, hex 32Б), plaintext-колонки запрещены CI-гейтом vault-grep (3 класса детекции) + psql-e2e; Debug ключей выведен вручную (маскирование); CI: override http://127.0.0.1:9 → детерминированный probe_unavailable + ПОЛНЫЙ verified-лайфсайкл через локальную 2xx-заглушку (save/GET/ротация/DELETE + plaintext-не-в-БД). Dev-субагент умер по таймауту после ~100% бэкенда (3-й случай). Ревью: первый прогон по неполному диффу (стейджинг без untracked — исправлено git add -N), второй по полному (4663 строки): 30 находок → 20 патч-групп (Debug-ключей, from_hex-паника, prod-запрет override, бюджет POST 30с, COALESCE, ротация-UI, vault-grep 3 класса, bytea-фикстура...) / 3 отклонено. 132 Rust + 80 SPA, гейты PASS. Урок: дифф обязан включать untracked-файлы.
  - Task ID 17: формальный bmad-code-review Story 1.3 исполнен (4 слоя: blind/edge/VG/acceptance; дифф f456aba..a82287c, 5179 строк): 12 патч-групп / 3 defer (AAD-привязка, zeroize, advisory-lock — Epic 2 hardening) / 4 отклонено. HIGH: CI-шаг лайфсайкла запускался из корня без полного пути к бинарю (pult/target/debug/…). Ключевые: Ozon-нога e2e (AC-3, client_id round-trip), plaintext-греп лога шлюза ×3, chunked-кап 64 КБ, 400-маркер Ozon только на 400, override только origin, vault-grep блочные комментарии, UI-статусы подключения честные, submitGeneration против гонки навигации. 133 Rust + 81 SPA, гейты PASS. Спека done (review_loop_iteration 1), sprint 1-3 done. Коммиты d4b60fd, ffff3c6, 3e5d9d0 (gitignore typo).
  - Task ID 18: красный CI (run 3e5d9d0, integration) диагностирован и починен; ПРОДУКТ НЕ ВИНОВАТ — патч ревью 1.3 добавил Ozon-ногу в verified-лайфсайкл, но оставил финальное ожидание «после DELETE WB список пуст» → Ozon-строка оставалась в списке → финальный grep молча ронял шаг (bash -e, последняя команда AND-списка, ноль stdout); сам исправленный шаг до 3e5d9d0 НИ РАЗУ не гонялся в CI (прогоны d4b60fd/ffff3c6 отменило concurrency). Два пула: 7c677ee — громкие ассерты (check_code/check_body/check_absent + fail с dump_evidence: тела, лог заглушки, лог gateway-3, дамп БД; заглушка логирует запросы) + ВЕСЬ integration-джоб переведён с мягких/молчаливых [ тест ] && grep на || { ::error::FAIL; exit 1; } (закрыт класс «ложное зелёное», подтверждено эмпирически) + evidence-dump-шаг (if: failure()) + хвост всех логов в «остановить шлюз»; 6e5bfaf — фикс хвоста: DELETE обеих площадок → GET пуст (+e2e DELETE существующей Ozon-строки). CI ЗЕЛЁНЫЙ (run 40824505229, все 4 джобы). Уроки: (а) большой патч ревью ушёл в main без CI-прогона из-за cancel-in-progress — при «последнем пуше сессии» ждать полного прогона; (б) ассерты bash -e обязаны быть громкими; (в) песочница 3-й раз стёрла ~/.cargo/~/.rustup — восстановить ПЕРЕД Story 1.4 (rustup, toolchain 1.99.1, docs/RECOVERY.md); (г) диагностика CI-логов через API — scripts/fetch_ci_logs.py (PAT из .git/config, наружу не печатается; /scripts/ в .gitignore).
  - Task ID 19: гигиена CI (владелец: «В actions проблемы» — warnings): checkout@v4→@v5 ×4, setup-node@v4→@v5 ×1, ubuntu-latest→ubuntu-24.04 ×4 (пин: latest мигрирует на Ubuntu 26; апгрейд на 26.04 — отдельная задача); выбран v5 (не v7.0.1/v7.1.0) — минимальный семантический прыжок; комментарий-щит в ci.yml. Run #23 success, все 4 джобы; warnings устранены доказательно (контрольный grep логов #22 vs #23: до — есть, после — 0). СБОЙ ИНСТРУМЕНТОВ в бою (bash/LS ×3, gateway EOF) ПОСЛЕ пуша c32a328 — восстановление на новом ходе владельца (перезапуск недоступен); RECOVERY.md: сценарий C переписан (echo alive → субагент-фоллбэк (PROBE-1: механизм проверен) → новое сообщение владельца; перезапуск НЕ просить), сценарий B + rust-восстановление. Rust env песочницы восстановлен (4-й сбой: rustup 1.99.1+clippy+rustfmt). Коммит c32a328 + docs.
- **В процессе:** нет.
- **Следующий шаг:** BMAD bmad-build Story 1.4 «Первичный синк истории и первый канбан» (FR-3; минимальный вертикальный срез конвейера adapter → RabbitMQ → worker → PG). Rust-окружение песочницы восстановлено (29.10 утро, сборка-верификация идёт). Внешние верификации: живой автотест реальными токенами WB/Ozon (владелец, интернет); установка из Маркета Б24 (dev-аккаунт). Открытые вопросы PRD без блокировки: 8.2 (юрист), 8.3 (Sally — до UI M1), 8.4–8.7.
- **Sprint-трекинг:** _bmad-output/implementation-artifacts/sprint-status.yaml: epic-1 in-progress; 1-1 done; 1-2 done; 1-3 done (формальное ревью закрыто 29.10 вечером, review_loop_iteration 1); 1-4..1-8 и остальные 35 сторис backlog. Спеки 1-1/1-2/1-3 — done. Отложенное: deferred-work.md (updated_at — решён триггером 0006 для этой таблицы; write-on-GET до хендшейка — хендшейк НЕ в 1.3, см. deferred; HTTP-тесты gateway; «другая площадка»; React-компонентные тесты; re-encrypt job; AAD-привязка шифротекста; zeroize; advisory-lock DELETE/POST — последние три Epic 2 hardening). Спеки: spec-1-1/1-2/1-3 — все done (1-3 — с полным циклом: build-ревью 2 прогона). Отложенное: deferred-work.md (updated_at — частично решён триггером 0006; write-on-GET до хендшейка — хендшейк НЕ в 1.3 (epics.md), см. deferred; HTTP-тесты gateway; «другая площадка»; React-компонентные тесты; re-encrypt job Ф1+). Спеки: spec-1-1 (done), spec-1-2 (done, review_loop_iteration 1 — образец полного цикла: build-ревью + формальный 4-слойный code-review). Отложенное: deferred-work.md (updated_at — первой UPDATE-сторис; write-on-GET 1.2; HTTP-тесты gateway; «другая площадка» 1.3; React-компонентные тесты — 1.8/Epic 2 hardening).
- **Правило работы:** вся проектная работа (проектирование, разработка, документация) ведётся по методологии BMAD: агенты/воркфлоу из `_bmad/`, артефакты в `_bmad-output/`, знания проекта в `docs/`. Волатильные факты (налоги, правила площадок, тарифы) — проверять веб-поиском и сверять с протоколом мониторинга гл. 5 эталона.
