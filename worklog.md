# Worklog

---
Task ID: 1
Agent: main (Z.ai)
Task: Установить и настроить BMAD METHOD (https://github.com/bmad-code-org/bmad-method) в неинтерактивном режиме в папке проекта. Все дальнейшие работы по проекту ведутся по методологии BMAD.

Work Log:
- Проверены предпосылки: Node v24.21.0 (требуется 20.12+), Python 3.12.15 (требуется 3.10+), npm 11.19.0.
- Изучена документация и CLI: стабильная ветка v6.12, пакет npm `bmad-method`, последняя стабильная версия 6.12.4.
- Первая попытка `npx bmad-method install` зависла (таймаут 5 мин) — частичная установка успела создаться.
- Создан /home/z/my-project/package.json; bmad-method@6.12.4 установлен локально в проект (node_modules, bin: `bmad`).
- Установка выполнена локальным CLI: `node node_modules/bmad-method/tools/installer/bmad-cli.js install --directory /home/z/my-project --modules bmm --tools claude-code,codex --yes --communication-language Russian --document-output-language Russian` — успешно.
- Верификация: `bmad status` → core 6.12.4 ✓, bmm 6.12.4 ✓, все модули актуальны.
- Исправлена конфигурация языка: в merged config.toml остался English от первой попытки — закреплено через `_bmad/custom/config.toml` (team override, committed) и `_bmad/custom/config.user.toml` (personal override): communication_language=Russian, document_output_language=Russian, user_name=Z.
- Проверена валидность всех 4 TOML-конфигов (tomllib) — все валидны; «odules.bmm]» в выводе терминала оказался артефактом отображения (в файле корректный `[modules.bmm]`).
- Созданы каталоги: docs/ (project_knowledge), _bmad-output/planning-artifacts/, _bmad-output/implementation-artifacts/ (с .gitkeep).
- Обновлён .gitignore: skills/, node_modules/, bmad-install.log.

Stage Summary:
- Установлен BMAD METHOD v6.12.4 (stable), модуль bmm (BMad Method — полный цикл SDLC), интеграция claude-code (.claude/skills, 29 скиллов) + codex (.agents/skills, 29 скиллов).
- Структура: _bmad/ (core, bmm, config.toml, config.user.toml, custom/, scripts/), _bmad-output/ (planning-artifacts, implementation-artifacts), docs/ (project knowledge), package.json с зависимостью bmad-method 6.12.4.
- Языки: общение агентов и документы — Russian (закреплено в override-слоях, переживающих переустановку).
- Ростер агентов BMM: Analyst (Mary), PM (John), UX Designer (Sally), Architect (Winston), Dev (Amelia) + скиллы workflows: product-brief, prd, architecture, create-epics-and-stories, sprint-planning, dev/build, code-review, review, retrospective, qa-generate-e2e-tests, brainstorming, advanced-elicitation, party-mode, help и др.
- Дальнейшая работа по проекту (проектирование, разработка) ведётся по процессам BMAD: фазы Planning (brief/PRD/Architecture/UX) → Solutioning (epics/stories) → Implementation (sprint planning → dev → review → retro).
- Обновление BMAD в будущем: `cd /home/z/my-project && npx bmad install` (или `npm run bmad -- install`), проверка: `npx bmad status`.

---
Task ID: 2
Agent: main (Z.ai)
Task: Добавить в постоянную память две директивы: (1) проверка и актуализация устаревающих данных/фактов, запрет галлюцинаций; (2) устойчивость к внезапному сбросу сессии — не терять прогресс и продолжать с места остановки.

Work Log:
- Спроектировано трёхуровневое решение непрерывности: worklog.md (журнал шагов, дополняется после каждого значимого шага) + раздел «Текущее состояние» в MEMORY.md (снимок статуса) + git-коммиты после каждой вехи.
- Создан /home/z/my-project/MEMORY.md: Директива 1 (актуальность данных: что проверять через WebSearch/WebFetch, запрет выдумывать факты, пометка «требует проверки»), Директива 2 (протоколы начала/во время/завершения сессии, возобновление после сброса).
- Протокол начала каждой сессии: MEMORY.md → worklog.md → git status/log → продолжение с места остановки.
- Сделан контрольный git-коммит установленной конфигурации BMAD и файлов памяти.

Stage Summary:
- Постоянная память формализована в /home/z/my-project/MEMORY.md, читается первым действием в каждой новой сессии.
- Механизм восстановления после сброса: worklog + git. Незавершённые шаги не теряются, завершённое не переделывается.
- Ожидаемый следующий шаг: описание проекта пользователем → BMAD-фаза Planning.

---
Task ID: 3
Agent: main (Z.ai)
Task: Регистрация концепции «Пульт селлера» v1.2 и запуск BMAD-фазы Planning (создание Product Brief).

Work Log:
- Восстановление после сброса сессии: обнаружено исчезновение .claude/ и .agents/ (29+29 скиллов; не были закоммичены — паттерн `skills/` в .gitignore игнорировал подкаталоги на любой глубине). Исправлен .gitignore (`/skills/`), BMAD-скиллы восстановлены повторным запуском установщика, git config core.fileMode false (шум смены прав файлов). Коммит 1a79732.
- Концепция пользователя зарегистрирована: upload/Pult_Seller_Etalonnaya_Koncepciya_v1_2.md → docs/Pult_Seller_Koncepciya_v1_2.md (проектное знание, BMAD project_knowledge).
- Концепция прочитана полностью (17 глав): продукт «Пульт селлера» — операционная система селлера маркетплейсов (WB/Ozon/ЯМ), 4 канала (Б24 → amoCRM → retailCRM → standalone), 11 модулей ядра + пакеты M12/«Ассортимент», SLA «не теряем заказы», финмодель с налоговым радаром (НДС 22%, УСН-НДС 5/7%), ось ИИ-сотрудников «Автопилот» (6 ролей, L0–L4, action-bus, data-flywheel), 11 обязательных контрактов ТЗ MVP (5 v1.1 + 6 v1.2), монетизация 3 ноги (тарифы 2 990/5 990/14 900 ₽), гейты Ф0–Ф4, гейт С4, 38 рисков, комплаенс РФ+СНГ.
- Установлен project_name = "Пульт селлера" (_bmad/custom/config.toml, team override).
- BMAD-воркфлоу bmad-product-brief исполнен (headless create): workspace _bmad-output/planning-artifacts/briefs/brief-pult-seller-2026-10-27/ (run folder pattern из customize.toml), .memlog.md инициализирован (uv run memlog.py init), brief.md написан по шаблону assets/brief-template.md на русском (document_output_language=Russian), memlog-запись decision добавлена.
- Разделы брифа: Executive Summary, Problem, Solution (11 модулей, SLA), Differentiation (7 слоёв канавы), Audience (4 кольца, периметр 15:0), Success Criteria (гейты Ф0–Ф4, North Star), Scope (MVP=Ф0, 11 контрактов, out-of-scope), Business Model, Constraints (комплаенс РФ/СНГ, красные линии), Vision (3 стадии доминирования), Sources (эталон v1.2).

Stage Summary:
- Зарегистрирован действующий эталон продукта: docs/Pult_Seller_Koncepciya_v1_2.md (единственный источник для проектирования/разработки/ТЗ).
- Создан первый BMAD-артефакт планирования: brief.md (draft, ожидает подтверждения владельца) в _bmad-output/planning-artifacts/briefs/brief-pult-seller-2026-10-27/.
- Следующие шаги BMAD-конвейера: bmad-prd (PRD из брифа+эталона) → bmad-architecture (архитектура из гл. 9) → bmad-ux (при необходимости) → bmad-create-epics-and-stories (эпики/сторис MVP Ф0) → bmad-sprint-planning → bmad-build.

---
Task ID: 4
Agent: main (Z.ai)
Task: Настроить постоянную синхронизацию с GitHub (https://github.com/artemiosu/sellom-pult): remote, токен, протокол восстановления, пуш всего прогресса. Далее — после потери сессии сверяться с репозиторием, в процессе разработки коммитить и пушить всё.

Work Log:
- Протокол начала сессии выполнен: MEMORY.md → worklog.md → git status/log (дерево чистое, HEAD a97ac56, ветка main, remote отсутствовал).
- Проверен доступ: git remote add origin с PAT в URL + git ls-remote — репозиторий доступен и пуст (нет веток).
- Первый push: git push -u origin main — успешно, все 5 коммиттов отправлены (HEAD на remote = a97ac56).
- Проверена публичность: анонимный web-доступ даёт 404 → репозиторий ПРИВАТНЫЙ; клон и пуш в новом окружении требуют PAT владельца.
- В MEMORY.md добавлена Директива 3 (синхронизация с GitHub): правила «коммит+пуш после каждой вехи», хранение токена ТОЛЬКО в .git/config remote URL, запрет записи токена в коммитируемые файлы, протокол восстановления (живое окружение → fetch+pull; мёртвое → клон по PAT + MEMORY/worklog).
- Раздел «Текущее состояние» обновлён: бриф принят (владелец сказал «продолжай»), Task ID 4 завершён, в процессе — Task ID 5 (PRD).
- Коммит + push этой вехи выполнены.

Stage Summary:
- GitHub artemiosu/sellom-pult (приватный) — единственный внешний источник прогресса: ветка main синхронизирована, далее push после каждой вехи.
- Токен хранится только в .git/config (не в файлах проекта); при потере окружения — запросить PAT у владельца и клонировать.
- Конвейер продолжен: следующим шагом выполняется BMAD bmad-prd (PRD из брифа + эталона v1.2).

---
Task ID: 5
Agent: main (Z.ai)
Task: BMAD-воркфлоу bmad-prd: создать PRD «Пульт селлера» (MVP Ф0) из брифа + эталона v1.2, с ревизией по рубрике качества и финализацией.

Work Log:
- Воркфлоу bmad-prd активирован по SKILL.md: resolve_customization → пути (prds/, run folder prd-pult-seller-2026-10-27), шаблон assets/prd-template.md; workspace создан, memlog инициализирован (6 записей).
- Дисциплина «extract, don't ingest»: эталон (344 КБ, 17 глав) НЕ загружен в контекст родителя — субагент 5-extract прочитал его полностью и вернул структурированное извлечение (гл. 3 JTBD/персон, гл. 7 модули M1–M11 + 6 ИИ-ролей + L0–L4, гл. 8 конвейер/SLA/С1–С8/роли, гл. 9 11 контрактов, гл. 10/13 тарифы/онбординг/гейты, гл. 12 комплаенс, гл. 15 риски, гл. 16 красные линии/KPI, гл. 5 волатильные факторы).
- prd.md написан по Essential Spine + Adapt-In Menu (16 разделов, 577+ строк): §0 назначение, §1 видение, §2 JTBD + не-пользователи + 6 UJ (персона Илья, сценарии С1/С2/С4), §3 глоссарий 35+ терминов, §4 функции 4.1–4.9 = 35 FR с проверяемыми следствиями (группы: онбординг, конвейер+SLA, M1, M2, M3, M4, M8-лайт, M10-лайт, M11-дайджест, тарифы/квоты), §5 не-цели, §6 MVP In/Out + 11 контрактов, §7 8 SM + 4 контр-метрики, §8 7 открытых вопросов, §9 индекс предположений; §10 NFR (99,5%/RTO 4ч/RPO 15мин), §11 комплаенс РФ+СНГ+AI, §12 монетизация, §13 интеграции, §14 риски Ф0, §15 GTM, §16 data governance.
- addendum.md написан для архитектора: детализация контрактов A-1…A-11, конвейер 6 стадий с гарантиями, SLA-механика, матрица ролей 5×6, гейты доверия автономии, ритмы цикла, осадок данных.
- Ревизия по рубрике (субагент 5-review → review-rubric.md): вердикты strong×4/adequate×3; находки: 1 critical + 6 high + механические (битые ссылки, глоссарийные пробелы, roundtrip индекса).
- Все critical/high устранены правками: пер-площадочная семантика канбана FR-17/18 + edge case UJ-1 + вопрос 8.1 (Ozon FBS); квоты кабинетов/ролей (Старт 1 кабинет/2 роли, Оператор 1 до 3 площадок, Сеть 5); A-1 = блокинг матчинга (канон гл. 9); индекс здоровья Ф0 = 4 активные зоны; FR-29 роли фазированы + квота «Старт=2 роли»; FR-9 авто-восстановление расхождений (4ч → SLA-кейс); FR-30 методика оценки ущерба по классам событий; глоссарий дополнен (кабинет, витрина, биллинг-контур, M1–M11); битые ссылки §10.3/8.2/8.6 исправлены; inline [ASSUMPTION] ↔ §9 roundtrip замкнут.
- PRD финализирован: frontmatter status=final, memlog-события записаны.

Stage Summary:
- Создан второй BMAD-артефакт планирования: PRD «Пульт селлера» (Ф0) в статусе final — _bmad-output/planning-artifacts/prds/prd-pult-seller-2026-10-27/prd.md + addendum.md + review-rubric.md + .memlog.md.
- PRD готов для нижестоящих фаз: архитектура (Winston) из PRD §4/§10/§13 + addendum A/B/C + эталон гл. 9; UX (Sally) из §2 UJ; эпики (John) из §4 FR/§6.
- Открытые вопросы (7) зарегистрированы: глубина чтения Ozon (FBS vs FBO в канбане), SLA-механика пула Б24, формат виджета, демо-данные, пороги алертов, правила пула, WTP ИИ-ролей — большинство не блокируют архитектуру, но вопрос 8.1 требует решения владельца до разработки адаптера Ozon.
- Следующий шаг: bmad-architecture (архитектура из гл. 9 эталона + addendum + PRD).

---
Task ID: 6
Agent: main (Z.ai)
Task: BMAD-воркфлоу bmad-architecture: создать Architecture Spine «Пульт селлера» (MVP Ф0) из PRD + addendum + эталона гл. 8/9, с верификацией стека вебом и ревьюерским гейтом.

Work Log:
- Воркфлоу bmad-architecture активирован по SKILL.md (fast-path): workspace _bmad-output/planning-artifacts/architecture/architecture-pult-seller-2026-10-27/ создан, memlog init (scope/purpose/altitude), шаблон assets/spine-template.md.
- Точечное извлечение гл. 9 эталона субагентом (6-extract): 6 слоёв, 7 API-поверхностей, 7 доменов ядра, конвейер, PostgreSQL+ClickHouse+token-vault+REST/OAuth2 названы; язык/брокер/провайдер/DevOps — НЕ зафиксированы эталоном (открытые решения Winston).
- Стек проверен вебом (Директива 1), 3 сессии WebSearch: Python 3.14.8 (позже скорректировано на 3.14.10), FastAPI 0.162.0 (PyPI 23.10.2026), PostgreSQL 18.6 (PG 19 — RC), ClickHouse 26.8 LTS (26.8.32.14, поддержка до 08.2027), Redis 8.8.3 (→8.8.4), RabbitMQ 4.4.0.
- ARCHITECTURE-SPINE.md написан по шаблону: парадигма hexagonal (ports-and-adapters) + событийный конвейер; каталоги 6 слоёв; AD-1..AD-21 (11 контрактов [ADOPTED] + граница ядра, единственный вход, владение данными, идемпотентность, token-vault/PII, операционный конверт РФ/RPO 15/RTO 4, наблюдаемость, релизный конвейер 48ч/канарейки, один язык, LLM-периметр); конвенции (integer-копейки, UUIDv7, UTC/ISO-8601, error-конверт); Stack; Structural Seed (3 mermaid: поток конвейера, правило зависимостей, деплой); Capability→Architecture Map (все 35 FR); Deferred (10 позиций).
- Линт lint_spine.py: 0 находок (до и после правок).
- Ревьюерский гейт — 3 параллельных субагента (Task 6-review): (1) рубрик-волкер: 1 critical (владение «инцидент»/«задача») + 3 high (событийный конверт, бизнес-время, штамп витрины); (2) tech-currency: стек подтверждён, 1 high (YC managed ClickHouse нет 26.8 → self-host Ф0) + 4 medium (Python патч 3.14.10, SQLAlchemy 2.1.x/Alembic 1.20.x, YC Redis→Valkey, Vite 8.x); (3) adversary: 16 атак → 12 дыр, 2 critical (квоты vs «ноль тихих потерь», PII в ClickHouse-витринах) + 6 high.
- Применены все critical/high: ужесточены AD-5 (SLA-инвариант главнее квот, счётчик-истина в PG), AD-14 (штамп {org, источник, fetched_at, built_at, cycle_id}; канбан читает PG), AD-15 (деривация ключей WB-поллинга), AD-16 (PII-манифест + CI-запрет PII в CH); добавлены AD-22 (канонические внутренние контракты + событийный конверт JSON), AD-23 (единая модель рабочих элементов, владелец core/rules), AD-24 (бизнес-время IANA), AD-25 (единственный владелец расчёта маржи, ROUND_HALF_UP), AD-26 (SLA-компенсация compensation_entitlement + биллинг-мост), AD-27 (изоляция демо-кабинета); Stack/Deferred скорректированы.
- Spine финализирован: status=final, 27 AD, memlog 16 записей, frontmatter дополнен списком reviews/.

Stage Summary:
- Третий BMAD-артефакт: ARCHITECTURE-SPINE.md (status: final) — контракт согласованности для эпиков Ф0: парадигма, 27 AD (включая 11 обязательных контрактов эталона), конвенции, проверенный стек (27.10.2026), структурный сид (каталог pult/, 3 диаграммы), карта FR→компоненты→AD, Deferred.
- Ключевые решения: Python 3.14/FastAPI — один язык ядра; PG 18 = system of record, ClickHouse 26.8 LTS = только аналитика, RabbitMQ 4.4 quorum = события конвейера; РФ-хостинг (кандидат YC Cloud, 2 зоны), RPO 15 мин/RTO 4 ч; ноль LLM в Ф0.
- Все три ревью (рубрик/тех-актуальность/adversary) в reviews/ — доказательная база решений; lint чист.
- Следующий шаг: bmad-create-epics-and-stories (эпики/сторис Ф0 из PRD FR-1..35 + spine AD-1..27) → bmad-sprint-planning → bmad-build. Вопрос PRD 8.1 (глубина чтения Ozon) решить с владельцем до разработки адаптера.

---
Task ID: 7
Agent: main (Z.ai)
Task: Пересмотр стека по вопросам владельца: (1) Rust вместо Python (скорость/ресурсы/1000 одновременных), (2) масштабируемость и фронтенд-скорость, (3) консистентность Valkey vs Redis во всех документах разработки.

Work Log:
- Протокол начала сессии: MEMORY → worklog → git fetch/pull (origin/main актуален, HEAD ac32fb1).
- Аудит grep по репозиторию: «Redis» фигурировал в живых правилах spine (AD-14, Conventions, deploy-диаграмма, Stack) — Valkey только примечанием в Stack/Deferred → ИИ-агент-разработчик взял бы Redis. PRD и эталон v1.2 привязок к языку/Redis не содержат (проверено).
- Веб-верификация версий (Директива 1, 28.10.2026): crates.io API + GitHub releases + endoflife.date → Rust 1.99.1 (stable), Axum 0.8.9, Tokio 1.54.1, SQLx 0.9.0, Valkey 9.1.3 (release 26.10.2026; managed YC — линии 7.2–9.0-valkey).
- ARCHITECTURE-SPINE.md пересмотрен: AD-20 переписан (Python 3.14/FastAPI → Rust stable + Tokio/Axum/SQLx; ML-контур Ф1+ — отдельный Python-сервис вне ядра; компромиссы зафиксированы честно); добавлен AD-28 (мандат «Valkey, НЕ Redis»: запрет Redis-образов CI-grep'ом, клиент redis-rs/valkey-glide допустим по RESP, сервер — всегда Valkey), AD-29 (модель ёмкости: stateless горизонт, WS-sharding + Valkey pub/sub, PgBouncer+реплика, autoscale воркеров по очередям, backpressure, k6-тест 1000 пользователей), AD-30 (бюджет производительности фронтенда: код-сплиттинг, <200 КБ gzip критический путь, TanStack Query+Virtual, WS-дельты, CWV-гейт в CI). Точечные правки: AD-14 (Redis→Valkey), AD-22 и Conventions (ports.py→ports.rs), Alembic→sqlx migrate, deploy-диаграмма (Valkey ×3), Stack (Valkey 9.1.x — НЕ Redis™), Deferred. Итог: 30 AD, статус final.
- .memlog.md: 7 новых записей (version/decision/change/event), frontmatter updated.
- MEMORY.md: снапшот обновлён (Task 7, стек-мандаты для всех агентов, следующая фаза — эпики).
- Коммит + push вехи в origin/main.

Stage Summary:
- Стек ядра: Rust 1.99.x / Axum 0.8.9 / Tokio 1.54.1 / SQLx 0.9.0 + PG 18 / ClickHouse 26.8 LTS / RabbitMQ 4.4 / **Valkey 9.1 (НЕ Redis)** / React 19 + Vite 8.
- Valkey зафиксирован мандатом AD-28 во всех слоях: живые правила spine (AD-14/22, конвенции, deploy, Stack, Deferred) + memlog + MEMORY — ИИ-агент-разработчик не сможет подставить Redis.
- Масштабируемость 1000 одновременных и запрет фризов фронтенда — архитектурные инварианты (AD-29/AD-30), а не устные обещания.
- Следующий шаг: bmad-create-epics-and-stories (эпики/сторис Ф0 из PRD FR-1..35 + spine AD-1..30).

---
Task ID: 8
Agent: main (Z.ai)
Task: Закрыть открытый вопрос PRD 8.1 (глубина чтения Ozon — FBS/FBO; владелец делегировал решение архитектору) и создать в docs/ инструкцию восстановления после сбоя сессии/инструментов/окружения.

Work Log:
- Протокол начала сессии: MEMORY → worklog → git fetch (origin/main актуален, HEAD af4c56e).
- Экспертное решение 8.1 (архитектор): чтение Ozon ПОЛНОЙ глубины ОБЕИХ схем — FBO + FBS/rFBS (заказы, остатки, тарифы, выплаты), обратной записи в Ф0 нет (FR-12 неизменен). Обоснование: SLA-инвариант «ноль тихих потерь» (AD-5) исключает частичный канбан; FR-16 «все заказы всех схем»; M3 требует остатки обеих схем; честность UJ-1 уже обеспечена пер-площадочной семантикой FR-18 + edge case; обе схемы читаются одним адаптером Ozon Seller API; сверки (FR-9/AD-14) требуют полноты; PII-контакты FBS — только под периметром AD-16.
- PRD обновлён: §8.1 → «РЕШЕНО» (решение + 6 пунктов обоснования), §9 запись → «ПОДТВЕРЖДЕНО», FR-2 и §6.1: [ASSUMPTION] → [CONFIRMED 28.10.2026], FR-16 дополнен явным перечислением схем Ozon FBO/FBS/rFBS; frontmatter updated; memlog PRD +decision.
- docs/RECOVERY.md создана (по указанию владельца «в папке docs есть инструкция на этот случай»): сценарии A (сброс сессии, окружение живо), B (окружение умерло — клон по PAT + восстановление BMAD), C (смерть инструментов — рестарт сессии, не более 2 ретраев); правила защиты прогресса; карта проекта со стек-мандатами. Зарегистрирована в MEMORY (Директива 3, пункт 0).
- Коммит + push вехи в origin/main.

Stage Summary:
- Все блокеры эпиков сняты: 35 FR подтверждены, вопрос 8.1 решён (Ozon обе схемы чтения), стек — Rust/Valkey (AD-20/28/29/30).
- Теперь в PRD 6 открытых вопросов, ни один не блокирует эпики: 8.2 (SLA-механика пула Б24 — юрист), 8.3 (Sally/UX), 8.4 (демо-данные — дизайн-партнёры), 8.5 (пороги — калибровка), 8.6 (правила пула — мониторинг), 8.7 (WTP Ф2–Ф3).
- Следующий шаг: Task ID 9 — bmad-create-epics-and-stories (эпики/сторис Ф0 из PRD FR-1..35 + spine AD-1..30).

---
Task ID: 9
Agent: main (Z.ai) + John (PM, bmad-create-epics-and-stories)
Task: BMAD-воркфлоу bmad-create-epics-and-stories: эпики и сторис MVP Ф0 из PRD (35 FR, статус final) + spine (30 AD) — headless-исполнение (владелец делегировал продолжение).

Work Log:
- Активация скилла: resolve_customization (workflow-блок пуст — prepend/append/persistent_facts отсутствуют) + resolve_config (RU, planning_artifacts). Шаги 01–04 прочитаны полностью и исполнены по порядку.
- Шаг 01 (извлечение): прочтён весь PRD §4 (FR-1..35), §6 (11 контрактов), §7 (SM-метрики), §10 (7 NFR), §11–16; spine (30 AD) — в контексте сессии. Инвентарь: 35 FR (однострочные, с проверяемыми следствиями), 7 NFR, 14 AR (архитектурные требования из AD), UX-раздел (bmad-ux отложен — вопрос 8.3, UI-сторис опираются на FR-следствия + AD-30). inputDocuments: prd.md, addendum.md, ARCHITECTURE-SPINE.md.
- Шаг 02 (эпики): 9 эпиков по USER-VALUE (не техслои): 1 Подключение селлера (FR-1..7), 2 Конвейер «ноль тихих потерь» (FR-8..12), 3 Панель (FR-13..15), 4 Канбан (FR-16..18), 5 Остатки/номенклатура (FR-19..21), 6 Финансы (FR-22..27), 7 Команда/процессы (FR-28..30 + FR-18-роботы), 8 Штурман-лайт (FR-31..33), 9 Тарифы/квоты (FR-34..35). Карта покрытия FR→эпик: 35/35.
- Шаг 03 (сторис): 39 сторис в формате шаблона (As a/I want/So that + Given/When/Then/And), каждая — размер одного dev-агента, ссылки на FR-N и AD-N в AC. Стартовый каркас (Structural Seed spine) — Story 1.1; таблицы БД создаются сторис только под собственные нужды (1.1 — только organizations/users/role_assignments/portal_settings); forward-зависимости устранены дизайн-последовательностью (первый канбан 1.4 — на данных WB, Ozon-заказы после 2.2; SLA-entitlement 2.6 после инцидент-таймера 2.4; задачи-пороги 5.1 → материализация движком 7.1; роботы 7.3 после ручного 4.3).
- Шаг 04 (валидация): покрытие 35/35 FR (grep-проверка по всем FR-1..35 — пропусков нет); starter = Story 1.1 ✓; принцип БД ✓; независимость эпиков ✓; file-churn оценён (FR-18 сплит 4.3/7.3 обоснован: ручное перемещение vs core/rules+action-bus); Redis в документе только в контексте мандата AD-28. Frontmatter: status final, stepsCompleted 01–04, validated-строка.
- Механическая проверка: 9 эпиков + Epic List (rg), 39 сторис, 39 Given-блоков, 35 строк карты.
- Коммит + push вехи в origin/main.

Stage Summary:
- Четвёртый BMAD-артефакт: _bmad-output/planning-artifacts/epics.md (status: final) — 9 эпиков / 39 сторис / 35 FR полностью покрыты / 30 AD внедрены в AC через 14 AR; готов к bmad-sprint-planning.
- Ключевая структура поставки ценности: онбординг → конвейер целостности → панель → канбан → остатки → финансы → команда → штурман → тарифы; каждая эпика автономно поставляет ценность.
- Сторис ссылаются на решение 8.1 (Ozon обе схемы чтения) в 1.3/2.2/4.1/4.2 и мандаты AD-20 (Rust), AD-28 (Valkey), AD-29 (k6-профиль 1000), AD-30 (бюджеты фронтенда).
- Следующий шаг: bmad-sprint-planning (спринты из epics.md) → bmad-build (Amelia, Rust-монорепа pult/). Опционально по запросу владельца: глубокое независимое ревью epics.md (как у PRD/spine).

---
Task ID: 10
Agent: main (Z.ai) + John (PM, bmad-sprint-planning)
Task: BMAD-воркфлоу bmad-sprint-planning (headless): гейт готовности к реализации + генерация sprint-status.yaml из epics.md.

Work Log:
- Протокол начала сессии: MEMORY → worklog → git status/log (HEAD 378d75d, origin/main синхронизирован, дерево чистое). Прочитана docs/RECOVERY.md (сценарий A применён корректно).
- Активация скилла: resolve_customization (пусто — defaults) + resolve_config (Z, RU, Пульт селлера, planning/implementation artifacts разрешены).
- Гейт готовности (readiness-gate.md): инвентарь полный (концепция v1.2 → brief → PRD final 35 FR → spine final 30 AD → epics.md final 9/39); трассировка двусторонняя без сирот (карта 35/35 FR, сторис→FR-N+AD-N через 14 AR); эпики — user value, forward-зависимостей нет (starter 1.1); UX-артефакт отсутствует, но промежуточный UX-контракт ЗАПИСАН в epics.md (§ UX Design Requirements: следствия FR + AD-30 + NFR-3/6, UX-валидация M1/M2 до их спринтов) — решения не «придумываются». Вердикт: PASS.
- Генерация трекинга (generate-tracking.md): эпик-файл один — _bmad-output/planning-artifacts/epics.md (шардированной версии нет). Dry-run: ok, 9 эпиков / 39 сторис, сирот/illegal нет; 2 warning'а — заголовок документа и обзорная секция «Epic List» (не эпики; все 9 настоящих распознаны) — пропуск принят осознанно.
- Боевая генерация: _bmad-output/implementation-artifacts/sprint-status.yaml создан (generated 10-28-2026 13:02): 48 backlog (39 сторис + 9 ретро optional), tracking_system file-system, story_location _bmad-output/implementation-artifacts; пост-валидация скрипта ok.

Stage Summary:
- Пятый BMAD-артефакт: sprint-status.yaml — трекинг 9 эпиков / 39 сторис; гейт готовности PASS.
- Все фазы planning завершены: brief → PRD → spine → epics → sprint-status. Ворота в разработку открыты.
- Следующий шаг: bmad-build — Story 1.1 (Стартовый каркас приложения и Structural Seed spine: Rust-монорепа pult/ по AD-20/28/29/30).

---
Task ID: 11
Agent: dev-субагент (Story 1.1, spec-1-1-structural-seed.md)
Task: Имплементация Story 1.1 «Стартовый каркас приложения и Structural Seed» по спеке (базис 35c2b77).

Work Log:
- Прочитаны спека + epic-1-context.md (контекст из frontmatter). Окружение: cargo/rustc отсутствовали → установлен rustup с toolchain 1.99.1 (совпадает с AD-20; манифест stable 2026-10-15 подтверждает 1.99.1). Docker/PostgreSQL в песочнице нет — integration-AC отдаётся CI (как предписано Design Notes спеки).
- Создана монорепа pult/: workspace на 13 крейтов (resolver 3, edition 2024), rust-toolchain.toml 1.99.1 (+clippy, rustfmt), Cargo.lock закоммичен; workspace-пины AD-20: tokio 1.54.1, axum 0.8.9, sqlx 0.9.0 (features: runtime-tokio, tls-rustls, postgres, migrate).
- pult-platform: событийный конверт AD-22 EventEnvelope (event_id=Uuid::now_v7(), past-tense snake_case-валидация event_type, organization_id, occurred_at Utc, schema_version, payload), конверт ошибок {error:{code,message,request_id,details}}, ServiceConfig::from_env (DATABASE_URL/PULT_HTTP_PORT/PULT_ENV). Крейт без axum/sqlx — зависим для всех слоёв без циклов. 8 юнит-тестов.
- 7 крейтов pult-core-* (sync/data/finance/shturman/rules/billing/identity): lib.rs + ports.rs с заготовкой порта-трейта по AR-2 (SyncPort…IdentityPort); 4 крейта pult-conveyor-* (adapters/action_bus/reconciliation/workers) — пустые заготовки. Зависимости только на platform (граница AD-1).
- pult-gateway: бин pult-gateway (Axum 0.8.9): /health (проба БД SELECT 1 под tokio-timeout 500мс → 200 ok / 503 degraded с версией), /ready (200 ready при применённых миграциях+живой БД, иначе 503 not_ready), connect_lazy-пул (старт без БД), sqlx::migrate!("../db/migrations") в фоне с ретраем 5с (в спеке опечатка «../../» — из pult/gateway вверх до pult/ это один уровень; каноничный путь pult/db/migrations сохранён).
- pult/db/migrations: 0001_organizations, 0002_users (FK→organizations, external_ref — точка интеграции Б24, БЕЗ паролей), 0003_role_assignments (FK→organizations/users), 0004_portal_settings (1:1 jsonb). timestamptz/uuid/gen_random_uuid()-fallback по конвенциям; только нужды Story 1.1.
- pult/deploy: docker-compose.yml (postgres:18, rabbitmq:4.4-management с rabbitmq.conf default_queue_type=quorum, valkey/valkey:9.1; healthchecks; именованные тома; БД pult/pult) — YAML-структура валидирована парсером (docker в песочнице нет).
- pult/platform/scripts: hexagonal-lint.mjs (AD-1: манифесты+исходники core на pult-conveyor-*/pult-shell-*), valkey-grep.mjs (AD-28: image:/FROM в *.yml/*.yaml/Dockerfile* по word-boundary), bundle-size.mjs (AD-30: gzip JS входных чанков из index.html против лимита).
- pult/shells/bitrix24: Vite 8.3.5 + React 19.3.1 + TS 5.9.3 (npm-версии верифицированы реестром; lockfile закоммичен). Экран «Начните подключение», точка интеграции b24.ts (чтение PLACEMENT/PLACEMENT_OPTIONS/LANG — iframe-placement, без аутентификации), честная заглушка статуса (ConnectionStatus: gateway-up/degraded/offline — без бутафорского CTA), скелетон вместо спиннеров.
- .github/workflows/ci.yml: 4 jobs — rust (build/clippy -D warnings/test), spa (npm ci+build+bundle-gate), gates (hexagonal+valkey), integration (compose up --wait → шлюз → /health ok → /ready → стоп PG → /health 503 degraded → восстановление → повторный up → psql ровно 4 таблицы → повторный старт шлюза no-op). Кэши Swatinem/rust-cache + setup-node npm-cache.
- pult/README.md (запуск стека одной командой + гейты + конвенции); .gitignore дополнен (target/, pult/shells/bitrix24/dist/).
- Верификация: cargo build --workspace OK (13 крейтов); cargo clippy --workspace --all-targets -- -D warnings — 0 предупреждений; cargo test --workspace — 11 тестов зелены (8 platform + 3 gateway); npm run build OK (gzip JS 69 534 Б < 200 000 — PASS гейта); hexagonal-lint/valkey-grep PASS + негативные тесты (image:redis в yaml/Dockerfile и dev-dep pult-conveyor-* в core → оба гейта краснеют, перечисляют нарушения); degraded-режим шлюза проверен локально вживую: без PG /health→503 {"status":"degraded","version":"0.1.0"} за ~0.5с, /ready→503 not_ready, ретрай миграций в логе, шлюз жив. Happy-path миграций/таблиц — за CI-джобой integration (нет docker/PG в песочнице).

Stage Summary:
- Structural Seed поставлен целиком по Code Map спеки: pult/ (13 крейтов, gateway+platform+7 core+4 conveyor), db/migrations 0001..0004, deploy-стек, 3 гейт-скрипта, SPA-оболочка Б24, CI на 4 jobs, README.
- Гейты AD-1/AD-28/AD-30 работают как исполняемые проверки (в обе стороны: PASS на каркасе, FAIL на нарушениях).
- Осталось на внешние контуры: CI-прогон на push (нет docker/PG локально), установка из Маркета Б24 (владелец, dev-аккаунт) — помечены как внешние верификации в спеке.

---
Task ID: 12
Agent: dev-субагент (Story 1.1, фикс-итерация ревью)
Task: Устранить находки ревью Story 1.1 (triage-таблица в спеке, маршруты patch) минимальными правками.

Work Log:
- ci.yml: (1) FAIL-направление гейтов — в job gates добавлены самотесты: фикс tmp-dir (yaml с image:redis + Dockerfile FROM → exit 1 valkey-grep), tmp-корень pult (core-крейт с dev-dep pult-conveyor-* и use pult_conveyor_… → exit 1 hex-lint; path-dep с ключом без префикса → exit 1; комментарий в Cargo.toml не триггерит → PASS; пустой core/ → exit 2), в job spa — негатив bundle-гейта (лимит 1000 → exit 1); (2) идемпотентность миграций pinned: после рестарта шлюза цикл /health ok + /ready→ready (читает migrations_applied); в degraded-шаге ассерт /ready→not_ready с "migrations_applied":true; (3) start postgres переставлен ДО финальных ассертов (bash -e больше не обрывает шаг до восстановления БД); (4) permissions: contents: read (top-level) + timeout-minutes: rust 15 / spa 10 / gates 5 / integration 20; (5) cargo fmt --all -- --check и --locked в build/clippy/test (components: clippy, rustfmt); (6) в spa job добавлен шаг npm run test (vitest); (7) ПОПУТНО ЛОВЛЕН И ИСПРАВЛЕН баг триггера: push-фильтр был 'branches: ain]' (объеденный [main] ещё в 035a781) — восстановлен [main], без этого CI не запускался бы по push.
- vite.config.ts: server.proxy /health и /ready → http://localhost:8080 (раньше dev-запрос уходил в SPA-fallback → ложный gateway-up).
- App.tsx: честная семантика статуса — 200→gateway-up, 503→gateway-degraded, любой другой код→offline (раньше любой не-ok → degraded-фикция).
- Skeleton.tsx: clamp ширины Math.max(0, 100 - i*18).
- b24.test.ts (новый, vitest 5.0.5): readPlacementContext — B24-query → embedded:true+поля; пустой/посторонний/пустой-PLACEMENT query → embedded:false; package.json: script "test": "vitest run" + devDep vitest ^5.0.5 (peer vite ^8 совместим; lock обновлён).
- config.rs: NotUnicode PULT_HTTP_PORT → явная InvalidEnv (не дефолт 8080); порт 0 → reject; DATABASE_URL хранится trim-нутым; from_env делегирует в from_lookup (подменяемый источник) — 9 юнит-тестов веток без гонок за окружение (missing/blank/trim URL, defaults, невалидный порт, порт 0, NotUnicode порт, невалидный PULT_ENV, явные порт+env).
- events.rs: доко-контракт приведён к честному — код проверяет только snake_case; past tense — конвенция ревью, не enforce (доки поля/new/enum + текст ошибки).
- hexagonal-lint.mjs: маркер 'conveyor/' (path-dep с переименованным ключом ловится); guard: пустой core/ → exit 2; чистка #-комментариев Cargo.toml перед сопоставлением.
- bundle-size.mjs: fallback «все assets/*.js» заменён на fail(2); regex type/rel с /i; неклассифицируемый ref (внешний URL / не-.js) → fail(2).
- compose: start_period 30s/20s/10s (postgres/rabbitmq/valkey) поверх зафиксированного 8b673dd пути тома PG 18.
- gateway/Cargo.toml: убрана мёртвая зависимость uuid (Cargo.lock перегенерирован).
- README.md: npm run build = только сборка; гейт размера — отдельная команда node platform/scripts/bundle-size.mjs … (и CI job spa).
- Верификация (точечно, только затронутое): гейты в обе стороны локально (hex PASS + 3 негатива + comment-hygiene + empty-core; valkey PASS + негатив; bundle PASS + low-limit→1 + uppercase-TYPE/external/no-refs→2); cargo fmt --all -- --check чисто (заодно прогнан rustfmt по дрейфу main.rs/error.rs/events.rs); cargo test -p pult-platform → 17 зелёных (10 config + 3 error + 4 events); cargo build + clippy -p pult-platform -p pult-gateway --all-targets --locked -D warnings — чисто; npm run test → 4 зелёных; npm run build + bundle-гейт PASS (69 560 < 200 000); YAML-валидация ci.yml (permissions/branches[main]/таймауты) и compose.
- Спека (status in-review + triage-таблица от ревьюера) в коммит не включена — книга ревьюера, лежит в рабочем дереве.

Stage Summary:
- Все patch-находки triage (№1/2/3/6/7/8/9/10-часть/11/12/13-uuid/14/15/17/18/19/20/22/24/25/26/28/29/30/31/32/33) закрыты минимальными правками; отклонённые triage не тронуты (включая graceful shutdown, appendonly, event-Deserialize).
- Дополнительно закрыт не попавший в triage баг: push-триггер ci.yml ('branches: ain]' → '[main]').
- CI-прогон и full-verification — на стороне ревьюера.

---
Task ID: 12 (завершение, оркестратор)
Agent: main (Z.ai) — верификация по диффу, CI, ревью 3 слоями, триаж
Task: Step-03/04/05 bmad-build для Story 1.1: проверка имплементации по диффу, первый CI-прогон, фикс-коммит, ревью, патчи, финальная верификация.

Work Log:
- Верификация по диффу (не по отчёту): полный diff 035a781 прочитан; локально воспроизведены cargo build/clippy/test (11), гейты PASS + негативы exit=1, degraded-режим живьём (503 за 0.5с).
- CI-прогон №1 (035a781): failure ×2 — (1) spa: bundle-гейт резолвил пути от working-directory джобы → шаговый working-directory override; (2) integration: postgres:18 exited(1) — образ сменил каталог данных (web-верифицировано: volume теперь /var/lib/postgresql) → перемонтирован том + диагностический шаг. Фикс-коммит 8b673dd → CI зелёный (run 40699452948).
- Ревью step-04, 3 слоя параллельно (blind-hunter / edge-case-hunter / verification-gap): 35 находок; триаж в спеке (таблица): 25 → patch, 9 отклонены (false/low-критерии/не-intent), 1 carried-закрыт. Группы патчей: гейт-самотесты в CI, dev-proxy Vite, /ready-ассерты, SPA-тест, robustness hex-lint, честность bundle-гейта, config-валидация+тесты, fmt/--locked, permissions/timeout, start_period, uuid-dep, README, clamp.
- Патчи тем же сабагентом (resume): b0d927a. ПУНКТ «бонус: починен push-триггер branches: ain]» — ОШИБКА ИНСТРУМЕНТА РЕНДЕРИНГА ВЫВОДА: последовательность «[m» глоталась в текстовом выводе (grep/diff/cat); od -c доказал, что branches: [main] корректен ВО ВСЕХ коммитах с 035a781. Реального бага не было; правки триггера не требовалось и не вносилось. Будущим агентам: при подозрениях на «битые скобки» в выводе — сверяться od -c / python.
- Полная перепроверка патчей: fmt OK; build --locked; clippy --locked -D warnings 0; Rust-тесты 20 (17 platform + 3 gateway); SPA vitest 4; bundle 69 560 < 200 000; гейты PASS + все негативные направления (exit 1/1/1/2/1) — включая новые самотесты из CI. YAML ci.yml/compose валидны.
- CI на b0d927a: completed success (все 4 jobs, самотесты гейтов зелены).
- Step-05: спека spec-1-1-structural-seed.md → status: done; sprint-status.yaml 1-1 → review (по workflow; next: bmad-code-review → done). Коммит 27f31dc (триаж-лог) запушен.

Stage Summary:
- Story 1.1 «Стартовый каркас приложения и Structural Seed» — реализована, отревьюена 3 слоями, запатчена, верифицирована (локально + CI зелёный). 4 коммита: 035a781, 8b673dd, b0d927a, 27f31dc.
- Внешние верификации остаются за владельцем: установка из Маркета Б24 в тестовый портал (нужен dev-аккаунт); формальный bmad-code-review (перевод сторис review → done).
- Следующий шаг: bmad-code-review для 1-1 → done → Story 1.2 «Шаг 0 — фильтр периметра» (bmad-build).

---
Task ID: 13
Agent: main (Z.ai) + 4 контекстно-свободных ревьюера (bmad-code-review)
Task: Формальное код-ревью Story 1.1 (bmad-code-review, перевод review → done); владелец делегировал выбор и применение патчей («доверяю вашему экспертному мнению»).

Work Log:
- Протокол начала сессии: MEMORY → worklog → git log/status (HEAD a7f05e3, origin/main синхронен; состояние опережало сводку — Story 1.1 уже реализована и в статусе review). Цель ревью определена по Tier 3: единственная стори в review — 1-1.
- Активация скилла: resolve_customization (workflow-блок: 4 слоя, prepend/append пусты) + resolve_config (RU, Пульт селлера). Контекст-док эпика прочитан по frontmatter спеки.
- Шаг 01: diff-источник — baseline_commit из спеки 35c2b77; дифф 35c2b77...b0d927a с исключением lock-файлов (машинная генерация) и журналов — 2671 строк (под порогом чанкования); claims_file — commit-сообщения 035a781/8b673dd/b0d927a; review_mode=full (спека есть) → acceptance-auditor активен.
- Шаг 02: 4 слоя параллельно (general-purpose, контекстно-свободные): blind-hunter (23 находок), edge-case-hunter (13, JSON-формат с guard-сниппетами), verification-gap (4+2 — с эмпирической верификацией: сам запускал vitest/гейты/синтетику; нашёл workspace-алиас-обход hex-гейта воспроизведением), acceptance-auditor (6 + подтверждение: lock-файлы в git, AC нарушений нет).
- Шаг 03 (триаж, каждый вердикт по коду): 46 находок → 24 patch / 1 defer / 14 rejected / несколько false. Ключевые high/medium: db_reachable .is_ok() на timeout — быстрая ошибка запроса = «БД жива» (high, 2 слоя независимо); bundle-size тихо роняет отсутствующие чанки (medium, 4 слоя); hex-гейт слеп к [workspace.dependencies]-алиасам (medium, воспроизведено); CI не ассертит HTTP-коды (medium); кросс-орг role_assignments без составного FK (medium); connect_lazy-паника на невалидном URL (medium); SPA: 200-HTML-фоллбек = ложный gateway-up (medium, нужна проверка Content-Type); compose-порты в LAN (medium). Отклонены с доказательствами: lock-файлы «отсутствуют» (git ls-files опроверг), CSS-бюджет (AC = JS-only), ESLint/audit (вне AC), MSRV (rust-toolchain — единственный пин), from_parts (YAGNI до 1.4) и др.
- Шаг 04: Review Findings записаны в спеку (24 unchecked patch + 1 checked defer + Rejected-приложение); deferred-work.md создан (updated_at — первая UPDATE-сторис обязана решить). Патчи (вариант 1, делегировано): применены все 24 — gateway (matches! Ok(Ok(_)); min_connections(1); connect_lazy → error+exit(1) с redact_url-маскировкой пароля + 2 теста; doc), config (NotUnicode DATABASE_URL → InvalidEnv; трим PORT/ENV; +2 теста), events (первый символ — строчная буква; +1 тест, расширен negative-лист), bundle-size (отсутствующий чанк = fail exit 2; query-суффикс отсекается), hexagonal-lint (скан корневого [workspace.dependencies] на conveyor//shells//gateway-пути; скип построчных //-комментариев .rs; маркеры gateway + path-формы; readOrDie exit 2), valkey-grep (закомментированные # image:/# FROM ловятся — zero-tolerance по I/O-матрице; readOrDie exit 2), миграции 0002/0003 (UNIQUE (organization_id, id) на users + составной FK (organization_id, user_id) — редакция на месте, постоянных развёртываний нет), compose (127.0.0.1-префиксы всех портов), Cargo.toml (убран feature signal; lock перегенерирован: −errno, −signal-hook-registry), README (cwd гейта; таблица env-переменных), SPA (health.ts phaseFromHealthResponse — только JSON = ответ шлюза; 5 тестов; App.tsx на хелпере; doc ConnectionStatus; мёртвое условие b24.ts), CI (--locked в integration; HTTP-код-ассерты во всех шагах /health+/ready; sentinel-строка organizations до/после повторного up; 2 новых негативных самотеста: workspace-алиас hex и отсутствующий чанк bundle).
- Верификация: cargo fmt/build/clippy -D warnings/--locked чисто; 25 Rust-тестов (gateway 5: +2 redact_url; platform 20: +2 config, +1 events) + 9 SPA-тестов (+5 health) зелены; bundle 69 604 < 200 000 PASS; гейты: hex PASS (7 core) + негативы exit 1 (dev-dep, renamed path-dep, workspace-алиас, gateway::) + PASS rs-комментарии + exit 2 (пустой core, EACCES-файл); valkey PASS + негативы exit 1 (активный image, закомментированный image, закомментированный FROM); bundle негативы exit 1 (лимит) / exit 2 (ghost-чанк) + query-resolve PASS; живьём: degraded (503/not_ready, шлюз жив с min_connections(1) и мёртвой БД), невалидный URL → exit 1 + лог с *** (не паника); YAML ci.yml/compose валидны (4 jobs; loopback-порты).
- Шаг 04.6: спека — 24 патча отмечены [x], review_loop_iteration: 1, Implementation Notes дополнены; sprint-status.yaml: 1-1 → done (14:45 → 15:20). Коммит a793efd + push. CI-прогон на GitHub — внешняя верификация (gh в песочнице нет).

Stage Summary:
- Story 1.1 полностью закрыта: реализация → ревью-петля bmad-build (35 находок) → формальный bmad-code-review (46 находок, 4 слоя) → 24 патча → полная локальная верификация зелёная → статус done в спринте.
- Гейты AD-1/AD-28/AD-30 стали существенно прочнее (алиас-обход, закомментированные образы, тихие дыры бюджета — закрыты с CI-самотестами в обе стороны); БД-слой получил композитную кросс-орг гарантию на уровне схемы.
- Следующий шаг: Task ID 14 — bmad-build Story 1.2 «Шаг 0 — фильтр периметра ваш бизнес» (первая пользовательская ценность: FR-1, не-селлер получает честный отказ).

---
Task ID: 14
Agent: main (Z.ai) + dev-субагент (прерван) + 3 контекстно-свободных ревьюера (bmad-build)
Task: BMAD bmad-build Story 1.2 «Шаг 0 — фильтр периметра "ваш бизнес"» (FR-1): спека → имплементация → ревью → патчи → review-статус.

Work Log:
- Шаг 01: интент — Story 1.2 (эпик 1, стори 2); epic-1-context валиден (mtime 13:07 > epics.md 12:49); континуитет spec-1-1 (done); story_key 1-2-шаг-0-фильтр-периметра-ваш-бизнес; дерево чистое, main; single-goal.
- Шаг 02: исследованы PRD FR-1/UJ-1/NFR (§10), spine (AD-1/13/16, карта FR→компоненты: gateway+shells+core/identity), состояние core/identity (пустой домен+IdentityPort) и gateway. Решения (делегировано владельцем, записаны в спеке): dev-фоллбэк идентичности до хендшейка 1.3 (dev-portal/dev-user, вне метрик); партнёрский слот VITE_PARTNER_* с честным дефолтом (рев-шер ≥30% — бизнес-условие владельца при публикации); вопросник 2 вопроса (модель торговли 4 варианта → вердикт; площадки для селлеров); метрика = SQL-агрегат (AD-22-событие — 1.4). Спека spec-1-2-perimeter-filter.md (dispatch, baseline 2e02d8f), Open Questions нет → Checkpoint 1 пройден делегированием → ready-for-dev (ff6cd78).
- Шаг 03: sprint 1-2 → in-progress; dev-субагент запущен с хендоффом — ПРЕРВАН по таймауту сессии после ~90% (код/тесты/SPA готовы; README и CI не тронуты). Оркестратор: аудит диффа пофайлово (артефакт «[m»-глотания в выводе — код валиден, подтверждено python-строкой), дозакрыл README (эндпоинты/VITE_PARTNER/dev-фоллбэк) и CI (3 e2e-шага шага 0; YAML-ловушка «: » в именах шагов закавычена). Верификация: 51 Rust (identity 5 + gateway 26 + platform 20) + 44 SPA тестов; гейты PASS (72 318 Б gzip); живая деградация /v1 (503-конверты, 422-валидация без БД, /health не тронут). Коммит 2b315fc.
- СБОЙ СРЕДЫ (~16:10 UTC+8): ~/.cargo и ~/.rustup исчезли, node_modules SPA снесён, _bmad/render и tool-results стёрты (git/node/сеть живы). Восстановление: rustup 1.99.1 + rustfmt/clippy, npm ci, re-render скилла. Урок — в Implementation Notes спеки; Git-репо единственная константа.
- Шаг 04: спека in-review; дифф 2e02d8f (2807 строк); 3 слоя параллельно: blind-hunter 17, edge-case-hunter 10, verification-gap 4 (pre-verified). Триаж: 0 intent_gap / 0 bad_spec (лупбэка нет); 22 patch / 3 defer / 3 false-отклонения (мёртвый metrics-клиент — таск спеки; save_verdict-избыточность — один вызов; см. таблицу в спеке). Патчи применены оркестратором (субагент не re-engageable): клиентский таймаут 8с (requestJson AbortController) + серверный бюджет 5с (budgeted_sqlx/store); площадки — закрытый список + lowercase + dedup + 422 unknown_marketplace; dev-portal — bind константы + CI no-query curl доказывает исключение из метрик; /v1-деградация закреплена в CI (422 invalid_trade_model и 503 database_unavailable при остановленном PG); 404/405 под error-конвертом (fallback + method_not_allowed_fallback); DOMAIN to_lowercase + 422 reserved_member_id на литерал dev-portal; эхо 422 ≤64 симв.; метрика → PgPerimeterStore; request_id в warn!-лог; replay-лог стора; чистые screenStateFromError (4 теста) и step0.ts canSubmitStep0 (5 тестов); кнопка «Повторить»; честный копирайт ConnectionStatus/ExitScreen; partnerSlot через new URL; CI grep-якоря; psql 5 таблиц; README (коды ошибок/семантика счётчиков/same-origin). НЮАНС: http:///path валиден по WHATWG (hostname='path') — исправлено ожидание теста, не код. Верификация: 57 Rust + 54 SPA зелены, гейты PASS, живьём reserved 422 / 404 / 405 конверты, fmt/clippy/YAML чисто. Коммит 0778180 + push.
- Шаг 05: спека → done (review_loop_iteration 0 — лупбэков не было); sprint 1-2 → review (17:20); deferred-work.md +3 записи (write-on-GET до хендшейка; HTTP-уровень Rust-тестов; «другая площадка»).

Stage Summary:
- Story 1.2 реализована и отревьюена: фильтр периметра шага 0 (FR-1) — вопросник без PII, вердикт core/identity (PerimeterStore-порт), /v1-контракты шлюза, SPA-роутинг (вопросник/воронка/исходящий экран/честная заглушка), миграция 0005 (вердикты + b24_member_id), CI e2e. Метрика UJ-1: installs/step0_completed/sellers/share.
- 111 тестов (57 Rust + 54 SPA), гейты в обе стороны, деградация честная. CI-прогон 0778180 — внешняя верификация (GitHub).
- Следующий шаг: формальный bmad-code-review 1-2 (review → done) → Story 1.3 «Мастер подключения с token-vault» (шаги 1–3).

---
Task ID: 15
Agent: main (Z.ai) + 4 контекстно-свободных ревьюера (bmad-code-review)
Task: Формальный bmad-code-review Story 1.2 (перевод review → done); применение патчей делегировано владельцем («продолжайте»); новый PAT записан в .git/config (только туда), доступ к remote проверен ls-remote.

Work Log:
- Протокол начала сессии: MEMORY → worklog → git (HEAD 24e544b, origin/main синхронен; docs/RECOVERY.md перечитана). Скилл bmad-code-review активирован по протоколу: resolve_customization (4 слоя, prepend/append пусты) + resolve_config (RU, Z). Цель по Tier 3: единственная стори review = 1-2; spec_file + context-док (epic-1-context) загружены, review_mode=full (acceptance-auditor активен).
- Дифф 2e02d8f...0778180 (baseline спеки → последний код-коммит; lock-файлы исключены): 3195 строк / 31 файл / +2757−82 (выше порога ~3000 — обработан целиком, прецеденты 2671/2807); claims = сообщения 3 коммитов. Файлы диффа/claims в scripts/review-1-2/ (не коммитятся).
- Шаг 02: 4 слоя параллельно (general-purpose, контекстно-свободные, модель сессии): blind-hunter 16 находок (floor N=10 по формуле скилла), edge-case-hunter 9 (JSON c guard-сниппетами), verification-gap 5 (3 pre-verified с демонстрацией: удаление фолбэков 404/405 не роняет ни один тест; dev-проба -o /dev/null отбрасывает ответ; STORE_BUDGET-ветка не покрывается CI-стопом-PG), acceptance-auditor 0 нарушений AC + 5 наблюдений (все AC прослежены до кода/CI, frozen-границы целы: ноль PII, hexagonal-граница, NFR-6).
- Шаг 03 (триаж, каждый вердикт проверен по коду — onboarding.rs/api.ts/appState.ts/App.tsx/экраны/step0/b24/миграция 0005/ci.yml/README перечитаны): 25 уникальных → 15 patch / 1 defer / 6 rejected (+3 слияния дубликатов между слоями). Ключевые medium: составной бюджет 3×5с=15с > 8с клиента (B13/E1/A4); 404/405/reserved без CI-ассертов (B6/VG1, pre-verified); dev-фоллбэк проверен наполовину (VG2); безлимитные DOMAIN/USER_ID → постоянные строки-гиганты (B4/E6); частичный placement-контекст молча → dev-фикстура (A1). Отклонены с доказательствами: request_id-клиент (нет канала поддержки в Ф0), e2e для 2 избыточных кодов, retry на reserved-422, статус-метаданные (переходное состояние workflow), дубли query-ключей (URL генерирует Б24), сырые 431/413 (транспорт вне контракта).
- Шаг 04 (патчи применены оркестратором, 15 групп): STORE_BUDGET 2с + доко-математика; юнит-тест таймаута бюджета (pending-future, #[tokio::test(start_paused)], dev-dep tokio/test-util); CI: +3 ассерта 404/405/reserved, dev-фоллбэк → 200+step0_required+psql dev-portal, repeat-POST обеих веток (state:funnel / non_seller+exit_screen), --max-time 10 на все 20 curl; placement-identity: PartialContext/Oversized → 422 invalid_placement (253/64), канонизация trim_end_matches('.'), placement_error_response; echo() на уровень модуля + фильтр control-символов + кап rejection-текстов; валидация площадок только для селлерских моделей (не-селлерам список очищается — контракт без односторонних 422); api.ts: clearTimeout в finally ПОСЛЕ чтения тела; api.test.ts: +1 тест таймаута (fake timers + AbortController); миграция 0005: CHECK trade_model (in-place, прецедент 13); ExitScreen («Яндекс Маркет — следом», партнёр/поддержка Б24 как канал), ConnectionStatus (ЯМ — следом); README: invalid_placement + internal_error + правила частичного контекста.
- НЮАНСЫ: (1) echo(rejection.body_text()) — body_text() возвращает String, нужны .as_str() (3 вызова, поймано clippy); (2) тест whitespace_values_… ожидал фоллбэк для пустого DOMAIN+USER_ID=42 — обновлён под новый контракт PartialContext; (3) при вставке Review Findings дублировался заголовок Spec Change Log вместо Design Notes — исправлено сразу.
- СБОЙ СРЕДЫ повторился (~11:35 UTC+8 29.10): ~/.cargo и ~/.rustup стёрты между сессиями; восстановлены rustup 1.99.1 + clippy/rustfmt; node_modules цел (npm ci не потребовался... фактически запускался npm ci — чисто).
- Верификация: 62 Rust-теста (+5: oversized, trailing_dot, echo_strips, budgeted_store_timeout, non_seller_discard; whitespace переписан), 55 SPA (+1 таймаут), fmt/clippy -D warnings --locked чисто, гейты PASS (72 568 Б gzip), ci.yml YAML валиден, живой шлюз: reserved/invalid_placement/404/405 конверты + 503-деградация через бюджет (~2с).
- Шаги 04.6–06: спека — Review Findings (15 [x] patch + defer + Rejected-приложение + верификация), review_loop_iteration 0→1, Spec Change Log +1; sprint-status.yaml 1-2 → done, last_updated 10-29-2026; deferred-work.md +1 (React-компонентные тесты → 1.8/Epic 2 hardening); MEMORY.md «Текущее состояние» актуализирован. Коммит + push.

Stage Summary:
- Story 1.2 «Шаг 0 — фильтр периметра» ПОЛНОСТЬЮ закрыта: реализация → build-ревью (3 слоя) → формальный bmad-code-review (4 слоя, 25 находок) → 15 патч-групп → 62+55 тестов, все гейты зелены → статус done в спринте. Итог по эпику 1: 1-1 done, 1-2 done, 1-3..1-8 backlog.
- Контракт /v1 стал строже и честнее: invalid_placement (частичный/сверхдлинный placement), внутренние бюджеты согласованы с клиентскими, конверт на любом маршруте подтверждён CI, реестр установок защищён от мусора на уровне схемы+валидации.
- Урок среды закреплён: второй полный сбой toolchain — восстановление по docs/RECOVERY.md занимает ~3 мин (rustup + components), репо — единственная константа.
- Следующий шаг: BMAD bmad-build Story 1.3 «Мастер подключения площадок с token-vault (шаги 1–3)» (FR-2; внешняя зависимость — регистрация приложения в Маркете Б24, владелец).

---
Task ID: 16
Agent: main (Z.ai) + dev-субагент (прерван) + 3 контекстно-свободных ревьюера ×2 прогона (bmad-build)
Task: BMAD bmad-build Story 1.3 «Мастер подключения площадок с token-vault (шаги 1–3)» (FR-2, AD-16): спека → имплементация → ревью → патчи → done.

Work Log:
- Шаги 01–02: интент — Story 1.3 (epic-1-context валиден, континуитет spec-1-2 done). Извлечение требований субагентом (epics.md: полный текст 1.3; PRD FR-2/addendum §B/spine AD-16/эталон L565-L1101: AES-256-GCM KMS-конверт, автотест прав, пауза вместо полурабочего). Веб-верификация (Директива 1, 29.10.2026): WB — категории данных токена (dev.wildberries.ru, 16.09.2026), Ozon — Client-Id/Api-Key + /v3/product/list. Решения (делегировано): vault — модуль core/identity (спин 7 крейтов не растём); KMS Ф0 = порт KeyProvider + env-ключ PULT_VAULT_MASTER_KEY (hex 32Б, обязательный); write-only vault (decrypt — только порт Epic 2); проба прав = конфиг-матрица лёгких запросов; updated_at — триггер (первая UPDATE-сторис); UNIQUE(org, marketplace) — Ф0-квота; роли — честная UI-пометка (enforcement — 1.6). Open Questions нет → Checkpoint 1 делегированием → ready-for-dev (c3e4efe).
- Шаг 03: sprint 1-3 → in-progress; dev-субагент прерван таймаутом после ~100% бэкенда (vault.rs домен + ports + connections.rs хендлеры/стор/шифратор/проба + миграция 0006 с триггером + vault-grep.mjs + config). Оркестратор дозакрыл: SPA (connect/step.ts + ConnectMaster 3 шага + StartConnection статусы/мастер + api-клиент + стили), тесты (step.test, api.test +5 групп), CI (vault-grep PASS+FAIL, e2e подключений, PULT_VAULT_MASTER_KEY), README. Верификация: 127+5 Rust, 76 SPA, гейты PASS, живьём exit-1-без-ключа/422/503. Коммит спеки → имплементация без коммита (пре-ревью).
- Шаг 04, прогон 1: дифф 1554 строки — ДЕФЕКТ СТЕЙДЖИНГА: untracked-файлы (vault.rs, connections.rs, миграция, SPA) не вошли; blind-hunter поймал. Дифф пересобран (4663 строки / 23 файла / +4592) — оба прогона зафиксированы в триаж-логе.
- Шаг 04, прогон 2 (полный дифф), 3 слоя: blind 15 / edge 10 / VG 3+2 pre-verified (VG сам проверил 401 от живого WB из своей среды!) → 30 находок → триаж: 20 патч-групп / 3 отклонено / 7 слияний; 0 intent_gap / 0 bad_spec — лупбэка нет. Патчи применены оркестратором (субагент не re-engageable): Debug-маскирование MasterKey/VaultMasterKey (derive печатал байты — утечка в любой {:?}); from_hex в символах+ASCII (64 байта многобайтовой строки паниковали индексацией пар); PULT_PROBE_HOST_OVERRIDE запрещён в prod (реальные токены ушли бы в заголовках на чужой хост) + доко; summarize(пусто) → probe_unavailable (иначе пустая матрица = Verified без прав); COALESCE(rights_snapshot,'[]') (NULL-строка роняла весь GET с 500); key_version без тихого unwrap_or(i32::MAX); потолок чтения тела пробы (Content-Length > 64 КБ → Unreachable); клиентский бюджет POST 30с (server worst = 2×10с проб) + тест-паритет + тест «ответ на 12-й секунде доезжает»; ротация из UI (выбор подключённой = «обновить токен»; была disabled-недостижима) + кнопка «Вернуться к статусам» из шага 1 (юзер был заперт до успеха); честная пометка при неудачном refresh статусов; мёртвый AbortController → cancelled-флаг (сигнал не передавался в fetch); SPA-парсер принимает 3 статуса домена (verified-only ронял бы список на будущей строке); vault-grep: блочные комментарии + нормализация строк + CI-негатив всех 3 классов (был 1 из 3); CI: PULT_PROBE_HOST_OVERRIDE=http://127.0.0.1:9 на job-уровне (VG pre-verified: живой WB отвечает 401 → rejected → ассерт probe_unavailable зависел от сети раннера), ПОЛНЫЙ verified-лайфсайкл через локальную 2xx-заглушку (restart шлюза с override на :18081; save → GET → ротация → ровно 1 строка → DELETE → GET пусто; psql: plaintext-подстроки токенов НЕ в token_ciphertext/nonce), 422 invalid_client_id, 503-деградация /v1/connections, валидный bytea-литерал в фикстуре триггера (repeat('\x00',12) — не bytea-формат), job-level env (дубль ключа в двух шагах). Отклонены: CI-проверка отсутствия ключа (юнит+live достаточно), правка спеки (GET-форма client_id), хардкод цвета (паттерн 1.2).
- Верификация после патчей: 132 Rust + 80 SPA, fmt/clippy --locked -D warnings чисто, гейты PASS (74 891 Б gzip), YAML валиден, живьём: prod+override → отказ старта, многобайтовый ключ → честный InvalidEnv (36 симв., не паника).
- Шаг 05: спека → done (review_loop_iteration 0 — лупбэков не было); sprint 1-3 → review (ожидает формального bmad-code-review); deferred-work.md +2 (re-encrypt job Ф1+; B24-хендшейк НЕ в epics.md 1.3 — write-on-GET defer остаётся открытым до отдельной сторис).

Stage Summary:
- Story 1.3 реализована и отревьюена: мастер подключения (шаги 1–3) + token-vault (AES-256-GCM, write-only в Ф0, plaintext запрещён CI-гейтом и psql-e2e), автотест прав пробами API площадок (WB: Статистика + Цены и скидки; Ozon: /v3/product/list), honest-статусы verified/rejected/probe_unavailable. 132 Rust + 80 SPA тестов, 4 CI-гейта (новый vault-grep), verified-лайфсайкл исполняется CI автоматически.
- Урок: стейджинг диффа обязан включать untracked-файлы (git add -N) — первый прогон ревью прошёл по половине истории; второй прогон по полному диффу дал основной объём находок.
- Следующий шаг: формальный bmad-code-review Story 1.3 (review → done) → Story 1.4 «Первичный синк истории и первый канбан». Внешние верификации: CI-прогон на push; живой автотест реальными токенами WB/Ozon (владелец, интернет).

---
Task ID: 17
Agent: main (Z.ai) + 4 контекстно-свободных ревьюера (bmad-code-review)
Task: Формальный bmad-code-review Story 1.3 (перевод review → done); патчи применены оркестратором.

Work Log:
- Цель по Tier 3: единственная в review — 1-3; spec_file + epic-1-context, review_mode=full. Дифф f456aba..a82287c (5179 строк / 26 файлов, lock-файлы исключены) + claims (2 коммита).
- 4 слоя параллельно: blind 15 (floor 10) / edge 10 (JSON) / VG 3+2 pre-verified (демонстрация: подмена client_id на None и маски на сырой секрет не ломают ни один тест; подмена бинаря) / acceptance-auditor: 0 нарушений frozen-интента, 5 аддитивных отклонений приняты (client_id в GET, override-конфиг, Ozon 400-маркер, идемпотентный DELETE, WB-игнор client_id), 5 verified-conformant блоков.
- Триаж по коду: 12 патч-групп / 3 defer / 4 отклонено / слияния. HIGH: CI-шаг лайфсайкла исполнял nohup target/debug/pult-gateway из КОРНЯ репо (бинарь в pult/target/) — persistence-верификация не исполнялась бы вовсе; фикс: полный путь pult/target/debug/... (working-directory нельзя — compose-пути шага от корня). Ключевые medium: plaintext токенов не грепался в логе шлюза (+3 негативных grep в CI); Ozon verified-путь (client_id round-trip, AC-3) не исполнялся e2e (+Ozon-нога лайфсайкла: POST → GET client_id=424242 + schemes fbo/rfbs); chunked-ответ площадки без Content-Length буферизовался целиком (content_length()-чек) — заменён порционным чтением с капом 64 КБ; UI звал любую строку «подключена» (теперь verified → «подключена», rejected/probe_unavailable → честные метки); Ozon 400-маркер применялся к любому статусу (5xx с «Invalid Api-Key» = ложный rejected — маркер только на 400); PULT_PROBE_HOST_OVERRIDE принимал путь/query/userinfo (отказ: чистый origin, тесты); vault-grep: многострочные /* */ срезаются по всему файлу (ложный FAIL), составные имена (ozon_api_key text) ловятся, .SQL по lowercase; клиентский паритет валидации токена (≤1024 + control-символы — предотвращение заведомых 422) + verified_at string|null; гонка навигации мастера (submitGeneration — ответ после «Назад» не перебрасывает на шаг 3); apiError чистится на «Назад»; readiness-полл заглушки проб; мёртвый CSS .connect-choice:disabled удалён. Отклонено: глубокая валидация элементов SPA-парсера (same-origin, серверные тесты), catch-all mismatched-credentials (недостижим по построению), 500-на-повреждённую-строку (дизайн 1.2), CI-клинап заглушки (свежий раннер). Defer: AAD-привязка, zeroize, advisory-lock гонка DELETE/POST (все — Epic 2 hardening; retrofit через re-encrypt).
- Верификация: 133 Rust + 81 SPA, fmt/clippy --locked -D warnings чисто, гейты PASS (vault-grep: блочный коммент. не ловится, ozon_api_key ловится), YAML валиден. Спека: Review Findings + review_loop_iteration 1; deferred-work +3. Коммит + push.

Stage Summary:
- Story 1.3 полностью закрыта: build (2 прогона ревью) + формальный 4-слойный code-review; 133 Rust + 81 SPA, 4 CI-гейта, CI исполняет verified-лайфсайкл с plaintext-инвариантами в БД и логах.
- Прецедент: HIGH-находка стейджинга CI-шага поймана формальным ревью до первого пуша в CI — цена пропуска была бы «вечно зелёный CI без главной проверки».
- Следующий шаг: bmad-build Story 1.4 «Первичный синк истории и первый канбан» (FR-3, минимальный вертикальный срез конвейера adapter → RabbitMQ → worker → PG). Внешние верификации: CI-прогон; живой автотест токенов (владелец).

---
Task ID: 18
Agent: main (Z.ai)
Task: Диагностика и починка красного CI: integration-джоба падала на шаге verified-лайфсайкла 1.3 (run 3e5d9d0).

Work Log:
- Окружение живо (Сценарий A docs/RECOVERY.md): bash/git ок, HEAD 3e5d9d0 = origin/main, рабочее дерево чистое; ~/.cargo и ~/.rustup стёрты песочницей (3-й случай — не блокирует CI-диагностику, Rust не нужен; восстановить перед Story 1.4).
- Диагностика по API-логам (scripts/fetch_ci_logs.py — PAT из .git/config, наружу не печатается): run 3e5d9d0 → integration failed на шаге 23 «verified-лайфсайкл через заглушку», 22 предыдущих шага зелёные. Шаг умер за ~3.1с БЕЗ единой строки stdout.
- Ретроспектива прогонов: a82287c — единственный прогон с этим шагом ДО патча ревью, падал громко («шлюз не поднялся» 33с: голый target/debug из корня); патч ревью (d4b60fd) НИ РАЗУ не проверялся CI — оба его прогона отменены concurrency cancel-in-progress. 3e5d9d0 — первый полный прогон исправленного шага.
- Стат-анализ (classify_probe_response 2xx→Confirmed, summarize_probes, save-INSERT против схемы 0006, ensure_organization, override-планинг) — логика корректна; упавший шаг не оставля улик: тела ответов не печатались, gateway-3.log не хвостился, /tmp раннера стирается.
- КОРНЕВОЙ дефект CI (независимо от причины падения): ассерты [ тест ] && grep при bash -e дают ДВА плохих режима — (а) отказ последней команды AND-списка = МОЛЧАЛИВАЯ смерть шага (именно это видно в 3e5d9d0: ни одного байта stdout), (б) отказ НЕ последней команды = «мягкий» пропуск — шаг продолжает работу при проваленной проверке (ЛОЖНОЕ ЗЕЛЁНОЕ). Подтверждено эмпирически локально (bash -e -c …): старый стиль exit=0 при проваленном grep, новый — exit=1 + сообщение.
- Патч ci.yml (минимальная семантика ожиданий сохранена, изменена только громкость): шаг лайфсайкла переписан на громкие check_code/check_body/check_absent + fail() с dump_evidence (тело ответа, лог заглушки, лог gateway-3); заглушка логирует каждый запрос (STUB <- METHOD /path — видно, достигали ли пробы); psql-проверки печатают фактические значения; НОВЫЙ шаг «evidence dump при сбое e2e» (if: failure()): все /tmp/*.json тела, логи, дамп marketplace_connections (секреты скрыты — длины bytea) и organizations; «остановить шлюза» теперь хвостит и gateway-3.log + probe-stub.log; ВСЕ мягкие ассерты integration-джобы (шаги 0, деградация, подключения, sentinel, 6 таблиц, метрика, триггер) переведены в громкий режим || { ::error::FAIL …; exit 1; } — закрыт риск ложного зелёного CI.
- Верификация: YAML валиден (4 джобы), bash -n всех 24 run-блоков integration — чисто; gitignore: /scripts/ (диагностика сессии не коммитится).

Stage Summary:
- Причина падения шага 23 пока не установлена окончательно (улик шаг не оставлял); устранён сам класс дефекта — молчаливые/мягкие ассерты bash -e — и добавлен полный evidence-dump. Следующий прогон CI либо зелёный, либо однозначно назовёт причину (код+тело+логи).
- Кандидаты по таймингу (~0.3-0.9с после здорового /health): пробы не получили 200 от заглушки (probe_unavailable → мягкий пропуск → смерть на bare-grep ротации) либо рантайм-ошибка save-пути (первое исполнение INSERT против живого PG).
- ИТОГ ДИАГНОСТИКИ (прогон 7c677ee с громкими ассертами): корневая причина красного integration — сам CI-шаг, НЕ продукт. Патч ревью 1.3 (d4b60fd) добавил Ozon-ногу лайфсайкла, но хвост шага остался от версии с одной WB: после DELETE wildberries в списке оставалась Ozon-строка, финальный grep «connections":[]» молча ронял шаг (bash -e, последняя команда AND-списка) — сигнатура 3e5d9d0 (смерть ~3с, ноль stdout). Evidence из прогона: v5.json = {"connections":[{ozon…}]}, дамп БД — ровно 1 строка ozon; ВЕСЬ остальной поток идеален (v0/v2/v6 verified, ротация, client_id=424242, ct_len 32/nonce 12, plaintext-инварианты, строк WB=1).
- Фикс: DELETE обеих строк (wildberries + ozon) → GET пуст → заодно e2e теперь покрывает DELETE существующей Ozon-строки (ранее — только идемпотентное удаление отсутствующей). Продуктовый код НЕ менялся (он корректен).
