# Review: актуальность технологического стека (tech-currency)

- **Артефакт:** `ARCHITECTURE-SPINE.md` (раздел Stack, разделы AD-14..AD-21, Structural Seed, Deferred)
- **Ревьюер:** review-subagent (tech-currency verifier)
- **Дата проверки:** 27.10.2026
- **Метод:** независимая веб-верификация каждой привязанной версии через официальные реестры/сайты (PyPI, npm registry, python.org/FTP, postgresql.org, packages.clickhouse.com, rabbitmq.com, Docker Hub library/redis, endoflife.date, docs.yandex.cloud). Директива 1 (MEMORY.md): чувствительные ко времени данные проверяются до использования; ниже все версии — результат проверки сегодняшним числом.
- **Общий вердикт:** стек **в целом подтверждён** — линии выбраны правильно, технологии существуют и подходят под задачу (РФ-хостинг, B2B SaaS, high-load синк, аналитические витрины, команда 5 чел.). Найдено **1 расхождение уровня high** (каталог Yandex Cloud не содержит ClickHouse 26.8), **3 medium** (устаревшая строка Python-патча; исчезновение managed Redis у YC → Valkey; устаревшие линии SQLAlchemy/Alembic) и **3 low** (патч-дрейф ClickHouse/Redis, устаревшая мажорная линия Vite). Ни одна находка не ломает архитектуру; все правки — точечные в строках стека и Deferred.

---

## 1. Позиции стека — построчная проверка

| # | Позиция (spine) | Факт на 27.10.2026 | Статус | Источник | Вердикт |
|---|---|---|---|---|---|
| 1 | Python 3.14.x (3.14.8 — «текущая maintenance»; поддержка до 10.2030) | Текущий патч линии — **3.14.10** (3.14.9 и 3.14.10 уже вышли; на python.org/ftp доступны 3.14.0–3.14.10). Поддержка до 31.10.2030 — подтверждена. **Выпущена Python 3.15.0 (09.10.2026)** — 3.14 больше не новейшая линия | **частично устарело** (патч), линия — ок | https://www.python.org/ftp/python/ ; https://endoflife.date/api/python.json | Линия 3.14 правильная (зрелая, полная поддержка до 10.2030, FastAPI/SQLAlchemy её поддерживают); остаться на ней — обоснованно, но «текущую maintenance» заменить на **3.14.10** и упомянуть выход 3.15 |
| 2 | FastAPI 0.162.0 (PyPI, 23.10.2026) | Latest PyPI = **0.162.0**, выложен **2026-10-23T18:47:31Z**. Классификаторы включают Python 3.14 | **подтверждено (точное)** | https://pypi.org/pypi/fastapi/json | Идеальное попадание; для API-шлюза и async-воркеров — адекватный стандарт |
| 3 | SQLAlchemy 2.0.x / Alembic 1.16.x (минор — при init) | Актуальная линия SQLAlchemy — **2.1.x** (2.1.0 вышла 24.09.2026; latest **2.1.6**); последний патч 2.0.x = 2.0.55. Alembic latest = **1.20.1** (в spine назван 1.16.x — 4 минора назад) | **устарело** (названные линии/миноры superseded) | https://pypi.org/pypi/sqlalchemy/json ; https://pypi.org/pypi/alembic/json | Строка «минор — при init репо» сохраняет гибкость, но в строке стека указать актуальное: **SQLAlchemy 2.1.x / Alembic 1.20.x** (проверить при init) |
| 4 | PostgreSQL 18.x (18.6, патч 13.08.2026; «PG 19 — RC, не берём») | PG 18 — current major; последний минор **18.6**, релиз минора **2026-08-13**; EOL 14.11.2030. **PG 19 RC 1 вышел 15.10.2026**, GA «planned for October 2026» — т.е. буквально на днях, но ещё не вышел (versions.json 19 не содержит) | **подтверждено (точное)** | https://www.postgresql.org/versions.json ; https://www.postgresql.org/support/versioning/ ; https://www.postgresql.org/developer/roadmap/ | И версия, и дата патча, и статус PG 19 — точны. Не брать свежий мажор в MVP — правильно |
| 5 | ClickHouse 26.8 LTS (26.8.32.14; поддержка до 08.2027) | Линия **26.8 LTS существует и является текущим LTS** (сборки в официальном LTS-репозитории: 26.8.32.14 есть, свежей является **26.8.34.13**, ~22.10.2026). EOL LTS — 27.08.2027 | **подтверждено** (патч слегка отстает) | https://packages.clickhouse.com/tgz/lts/ ; https://endoflife.date/api/clickhouse.json | Выбор линии правилен; при init взять свежий патч 26.8.x. См. находку F-1 про Yandex Cloud |
| 6 | RabbitMQ 4.4.0 (quorum queues) | **4.4.0 — актуальный релиз**, выпущен **20.10.2026**; community support до 31.07.2027, extended до 31.10.2028. Важно: **с 4.3 quorum-очереди поддерживают строгие приоритеты сообщений (32 уровня)** — комбинация «quorum + приоритеты» в spine корректна | **подтверждено (точное)** | https://www.rabbitmq.com/release-information ; https://www.rabbitmq.com/docs/quorum-queues ; https://endoflife.date/api/rabbitmq.json | Идеальное попадание. Планировать смену release-серии ~раз в год (серия живёт ~12 мес.) |
| 7 | Redis 8.8.x (8.8.3) | В линии 8.8 свежий патч — **8.8.4**; существует более новая линия **8.10 (8.10.3)** — текущая latest. 8.8 не EOL | **частично устарело** (патч + одна линия позади) | https://endoflife.date/api/redis.json ; https://hub.docker.com/v2/repositories/library/redis/tags/ (8.8.4, 8.10.3) | Для кэша/локов/rate-limit достаточно 8.8.x; зафиксировать 8.8.4 или осознанно взять 8.10. См. F-3 (managed Redis у YC не существует) |
| 8 | React 19.x (миноры — при init) | Latest = **19.3.1** (14.10.2026); React 20 в stable не вышел (next/canary — тоже 19.x) | **подтверждено** | https://registry.npmjs.org/react | Линия актуальна, рисков нет |
| 9 | Vite 7.x (миноры — при init) | Актуальная мажорная линия — **8.x** (latest **8.3.5**); 7.x — предыдущая (последний патч 7.3.7) | **устарело** (мажорная линия) | https://registry.npmjs.org/vite | В строке стека заменить «7.x» на «8.x (проверить при init)» либо зафиксировать 7.x как осознанное решение (для SPA-оболочки Б24 разницы мало) |
| 10 | Prometheus + Grafana + Alertmanager («актуальные релизы на дату провижининга») | Версии не привязаны — проверке не подлежат; политика «брать актуальные self-hosted релизы» сама по себе корректна | **подтверждено** (политика) | — | Пункт корректен; при провижининге зафиксировать конкретные версии в `deploy/` |

## 2. Смежные утверждения (Yandex Cloud)

| Утверждение (spine, Deferred) | Факт на 27.10.2026 | Статус | Источник |
|---|---|---|---|
| YC — кандидат РФ-хостинга с **managed PostgreSQL 18** | Managed Service for PostgreSQL поддерживает версии **14, 14-1c, 15, 15-1c, 16, 16-1c, 17, 17-1c, 18, 18-1c** (документация создания кластера); в release notes (декабрь 2025) — «Для PostgreSQL 18 доступны все расширения, которые поддерживаются в 17»; есть пресет «кластер для 1С» на 18-1с. По умолчанию при создании — 17, но 18 выбирается | **подтверждено** | https://yandex.cloud/ru/docs/managed-postgresql/operations/cluster-create (markdown: https://docs.yandex.cloud/ru/ru/managed-postgresql/operations/cluster-create.md) ; https://yandex.cloud/ru/docs/managed-postgresql/release-notes |
| YC — кандидат с **managed ClickHouse 26.8** | В каталоге YC Managed ClickHouse доступны версии: **25.8.24.21 LTS, 26.3.12.3 LTS, 26.4.4.38 и 26.5.2.39**. **26.8 LTS отсутствует**; последняя добавленная — 26.5 (июнь 2026), последний добавленный LTS — 26.3 | **НЕ подтверждено** → F-1 (high) | https://yandex.cloud/ru/docs/managed-clickhouse/operations/cluster-create (markdown: https://docs.yandex.cloud/ru/ru/managed-clickhouse/operations/cluster-create.md) ; https://yandex.cloud/ru/docs/managed-clickhouse/release-notes |
| YC — кандидат с **managed Redis** | Managed Redis у YC **переведён в Managed Service for Valkey™** (docs-путь `managed-redis` редиректит на `managed-valkey`): версии **7.2-valkey, 8.0-valkey, 8.1-valkey, 9.0-valkey**. Redis™ как managed-сервис в каталоге отсутствует | **оспорено/устарело** → F-3 (medium) | https://yandex.cloud/ru/docs/managed-valkey/operations/cluster-create (markdown: https://docs.yandex.cloud/ru/ru/managed-valkey/operations/cluster-create.md) |
| «Две зоны доступности в РФ» реалистичны у провайдера | Официальная документация: «В Yandex Cloud есть **несколько зон доступности**, расположенных в дата-центрах Яндекса»; зоны изолированы друг от друга, мультizonные деплои рекомендуются для отказоустойчивости. Регион «Россия» (ru-central1) исторически даёт 3 зоны; есть отдельный регион Казахстан | **подтверждено** (≥2 зон в РФ-регионе) | https://yandex.cloud/ru/docs/overview/concepts/geo-scope (markdown: https://docs.yandex.cloud/ru/ru/overview/concepts/geo-scope.md) |

## 3. Адекватность выборов (подход под задачу; лучшие альтернативы)

| Домен | Оценка | Обоснование |
|---|---|---|
| **Очередь: RabbitMQ 4.4 quorum vs Kafka vs Redis Streams** | **подтверждено** — выбор адекватен | Требования: per-tenant fair-scheduling (AD-6) + долговечность quorum (AD-14) + приоритеты (заказы вперёд) + команда 5 чел. Kafka даёт партиционно-параллельную модель без per-message приоритетов и требует KRaft-кластер/опс-нагрузку, избыточную на старте; Redis Streams противоречил бы собственному правилу spine «Redis — только эфемерное» (AD-14) и слабее по долговечности. RabbitMQ: quorum = Raft-репликация, с 4.3 строгие приоритеты в quorum-очередях (32 уровня), per-vhost лимиты, prefetch для честной раздачи,成熟 UI. Fair-scheduling по тенантам всё равно реализуется в диспетчере воркеров (spine так и делает) — брокер выбор не сужает |
| **OLAP-витрины: ClickHouse vs PG-только vs Timescale** | **подтверждено** | Витрины с «данными на момент», сверки КС, агрегаты по 10К–100К SKU — профильная задача ClickHouse; PG для этого не предназначен; managed-опция в РФ-облаке существует (см. F-1 про версию) |
| **ORM: SQLAlchemy+Alembic vs SQLModel/Tortoise и пр.** | **подтверждено** (с правкой версии, F-4) | SQLAlchemy 2.x — стандарт де-факто для Python/FastAPI: зрелый async, полный SQL, Alembic для миграций forward-only (соответствует Convention spine). «Общеизвестно лучшей» альтернативы нет; SQLModel — лишь обёртка |
| **Наблюдаемость: Prometheus+Grafana+Alertmanager vs VictoriaMetrics** | **подтверждено, оспоримо слабо** | Для 5-чел. команды и ~десятка сервисов vanilla Prometheus — стандарт с максимальной документируемостью; VictoriaMetrics — лёгкая drop-in альтернатива (ниже расходы на хранение), но не делает выбор Prometheus «ошибкой». Дополнительно: у YC есть Cloud Monitoring с совместимостью (remote write в Prometheus-формате) — можно не поднимать свой TSDB на старте. Решение можно оставить как есть, переоценить при провижининге |
| **SPA-оболочка: React 19 + Vite** | **подтверждено** (с правкой Vite, F-5) | React 19 — текущий; SPA в iframe Б24 — уместно; exact-миноры отложены корректно |

## 4. Findings

| ID | Severity | Позиция | Что найдено | Рекомендация |
|---|---|---|---|---|
| **F-1** | **high** | Deferred: «кандидат Yandex Cloud — managed PG 18/**ClickHouse**/Redis/K8s» + Stack «ClickHouse 26.8 LTS» | Каталог Yandex Cloud **не содержит ClickHouse 26.8** (на 27.10.2026 доступно до 26.5; последние LTS — 25.8 и 26.3). Выбор 26.8 LTS в managed-варианте YC на сегодня невозможен — spine неявно склеил желаемую upstream-версию с недоступным managed-каталогом | В Deferred уточнить: (а) managed YC ClickHouse = 26.3 LTS / 26.5 на старте, (б) self-host 26.8 LTS в docker-compose на managed-VM (соответствует Ф0-плану деплоя spine), или (в) старт на 26.3 LTS в YC с миграцией, когда YC добавит 26.8. Зафиксировать решение при провижининге; upstream-строка стека 26.8 корректна для self-host |
| **F-2** | **medium** | Stack: Python «3.14.8 — текущая maintenance» | Текущий патч линии 3.14 — **3.14.10** (spine на 2 патча позади); **09.10.2026 вышла Python 3.15.0** — 3.14 уже не новейшая линия (но полностью поддерживается до 31.10.2030) | Поправить строку: «3.14.x (текущий патч 3.14.10; линия 3.15 вышла 09.10.2026 — не берём, слишком свежая)». Выбор линии 3.14 оставить — он верный |
| **F-3** | **medium** | Deferred: «managed … Redis …» + Stack «Redis 8.8.x» | Managed Redis в Yandex Cloud **не существует** — сервис мигрирован на **Valkey** (7.2/8.0/8.1/9.0-valkey). Redis 8.8.x доступен только self-hosted (это не блокер: Ф0-план — docker-compose на managed-VM) | В Deferred заменить «Redis» на «Redis self-hosted на VM или managed Valkey»; при провижининге решить: остаться на Redis OSS (8.8.4+) self-hosted или перейти на Valkey (API-совместим для кэша/локов/rate-limit) |
| **F-4** | **medium** | Stack: «SQLAlchemy 2.0.x / Alembic 1.16.x» | Актуальные линии: **SQLAlchemy 2.1.x** (2.1.0 от 24.09.2026, latest 2.1.6) и **Alembic 1.20.x** (1.20.1). Названные в spine — уже superseded | Обновить строку стека: «SQLAlchemy 2.1.x / Alembic 1.20.x (минор — при init)»; при init проверить совместимость 2.1 с целевыми драйверами (asyncpg) |
| **F-5** | **low** | Stack: «Vite 7.x» | Актуальная мажорная линия Vite — **8.x** (latest 8.3.5); 7.x — предыдущая линия (7.3.7) | Заменить на «Vite 8.x (минор — при init)» или зафиксировать 7.x осознанно; влияние на продукт минимально |
| **F-6** | **low** | Stack: патчи ClickHouse/Redis/Python | Патч-дрейф: ClickHouse 26.8.32.14 → **26.8.34.13**; Redis 8.8.3 → **8.8.4** (есть и линия 8.10.3); Python 3.14.8 → 3.14.10 (см. F-2) | Правило «код владеет версиями с первого коммита» (spine) само это закрывает — при init зафиксировать свежие патчи; строку «8.8.3» заменить на «8.8.x (текущий патч при init)» |
| **F-7** | **info** | RabbitMQ 4.4.0 (позитив) | 4.4.0 вышел 20.10.2026 — релиз менее недели от даты spine; с 4.3 quorum-очереди поддерживают строгие приоритеты (32 уровня), т.е. связка «quorum + приоритеты» в mermaid-схеме корректна; community-поддержка серии — до 31.07.2027 | В операционный план: обновление release-серии RabbitMQ ориентировочно раз в год (серия живёт ~12 мес.), учесть в `deploy/` и runbook |

## 5. Что проверено и подтверждено без замечаний

- **PostgreSQL 18.6**: current major, минор от 13.08.2026, EOL 14.11.2030; «PG 19 — RC, не берём» — точно (RC 1 от 15.10.2026, GA в октябре).
- **FastAPI 0.162.0**: latest PyPI, дата совпадает до дня.
- **RabbitMQ 4.4.0**: latest, quorum-модель соответствует AD-14.
- **React 19.x**: текущая линия (19.3.1).
- **Yandex Cloud managed PostgreSQL 18**: подтверждено документацией (версии 14–18 + 1С-варианты).
- **«Две зоны доступности в РФ» у YC**: подтверждено — «несколько зон доступности» в ДЦ Яндекса, мультizonные деплои официально рекомендуются; prod-план spine (2 зоны) реалистичен.
- **Общая архитектура владения данными** (PG=истина, CH=производные витрины, очередь=журнал, Redis=эфемерное, AD-14): выбранная связка технологий эту модель реализует без противоречий.

## 6. Ссылки (полный список источников)

1. https://www.python.org/ftp/python/ — реальные релизы 3.14.0–3.14.10, 3.15.0–3.15.2
2. https://endoflife.date/api/python.json — 3.14.10 (EOL 31.10.2030), 3.15.2 (release 09.10.2026)
3. https://pypi.org/pypi/fastapi/json — 0.162.0 (2026-10-23)
4. https://pypi.org/pypi/sqlalchemy/json — latest 2.1.6; 2.1.0 от 24.09.2026; 2.0.55
5. https://pypi.org/pypi/alembic/json — 1.20.1
6. https://www.postgresql.org/versions.json — 18 current, latestMinor 6, минор 2026-08-13
7. https://www.postgresql.org/support/versioning/ — 18.6, EOL 14.11.2030
8. https://www.postgresql.org/developer/roadmap/ — «October 15, 2026: PostgreSQL 19 RC 1 Released!»; GA 19 — planned October 2026
9. https://packages.clickhouse.com/tgz/lts/ — официальные LTS-сборки; 26.8.32.14 и 26.8.34.13 присутствуют (HEAD 200)
10. https://endoflife.date/api/clickhouse.json — 26.8 latest 26.8.34.13 (22.10.2026), EOL 27.08.2027
11. https://www.rabbitmq.com/release-information — 4.4.0 (20.10.2026), community до 31.07.2027, extended до 31.10.2028
12. https://www.rabbitmq.com/docs/quorum-queues — строгие приоритеты quorum с 4.3 (32 уровня)
13. https://endoflife.date/api/rabbitmq.json — 4.4.0
14. https://endoflife.date/api/redis.json — 8.8.4; 8.10.3
15. https://hub.docker.com/v2/repositories/library/redis/tags/ — 8.8.4, 8.10.3 (official image)
16. https://registry.npmjs.org/react — latest 19.3.1 (14.10.2026)
17. https://registry.npmjs.org/vite — latest 8.3.5; 7.x последний патч 7.3.7
18. https://yandex.cloud/ru/docs/managed-postgresql/operations/cluster-create (markdown: https://docs.yandex.cloud/ru/ru/managed-postgresql/operations/cluster-create.md) — версии PG: 14…18 (+1С)
19. https://yandex.cloud/ru/docs/managed-postgresql/release-notes — PG 18: паритет расширений (декабрь 2025)
20. https://yandex.cloud/ru/docs/managed-clickhouse/operations/cluster-create (markdown: https://docs.yandex.cloud/ru/ru/managed-clickhouse/operations/cluster-create.md) — версии CH: 25.8.24.21 LTS, 26.3.12.3 LTS, 26.4.4.38, 26.5.2.39
21. https://yandex.cloud/ru/docs/managed-clickhouse/release-notes — «Доступна новая версия ClickHouse®: 26.5» (июнь 2026); «26.3 LTS и 26.4» (2026)
22. https://yandex.cloud/ru/docs/managed-valkey/operations/cluster-create (markdown: https://docs.yandex.cloud/ru/ru/managed-valkey/operations/cluster-create.md) — Valkey: 7.2/8.0/8.1/9.0-valkey (managed-redis редиректит сюда)
23. https://yandex.cloud/ru/docs/overview/concepts/geo-scope (markdown: https://docs.yandex.cloud/ru/ru/overview/concepts/geo-scope.md) — «несколько зон доступности» в ДЦ Яндекса

---

*Проверка выполнена 27.10.2026 в соответствии с Директивой 1 (MEMORY.md). Все версии выше — результат живых запросов к реестрам в день проверки.*
