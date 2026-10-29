-- Story 1.4 (первичный синк истории и первый канбан, FR-3 / AD-3 / AD-4 / AD-15 / AD-22):
-- 1) sync_runs — циклы синка с чекпоинтами (organization_id, тип цикла) —
--    продолжение после сбоя с места остановки (AD-3);
-- 2) orders — канонические заказы (проекция канбана), ФИЗИЧЕСКИ
--    партиционированы HASH(organization_id) — AD-4 (изоляция PII и нагрузки
--    одним решением);
-- 3) order_items — позиции заказов (партиционирование позиций — deferred:
--    объём Ф0 мал, AD-4 закрыт партициями родителя + составным FK-путём).
--
-- Инварианты спеки:
-- - доставка at-least-once — потребитель идемпотентен (AD-15):
--   UNIQUE(organization_id, idem_key) гасит точный дубль события,
--   UNIQUE(organization_id, marketplace, external_order_id) + ревизионный
--   guard (revision) не даёт откатить заказ на старую ревизию;
-- - idem_key = hex(sha256(marketplace || entity_type || external_id || revision))
--   — детерминированный отпечаток AD-15, виден для диагностики;
-- - канон-колонка канбана — 7 словарь FR-16 (новый→сборка→отгрузка→в пути→
--   доставлен→возврат→риск), нормализация площадок — в core/sync (AD-22),
--   НЕ в БД: CHECK фиксирует словарь, маппер — в коде домена;
-- - сырые ответы площадки — platform_payload jsonb (AD-22), не разбор в SQL.

-- ── sync_runs: один активный цикл на (organization_id, cycle_type) ─────────
CREATE TABLE sync_runs (
    id                uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
    organization_id   uuid        NOT NULL REFERENCES organizations (id) ON DELETE CASCADE,
    cycle_type       text        NOT NULL CHECK (cycle_type IN ('initial_orders')),
    -- pending: создан, окна не начаты; running: окна идут;
    -- succeeded: полный охват окон; failed: неисправимый сбой (диагностика);
    -- needs_retry: сбой с чекпоинтом — продолжаем по retry;
    -- superseded: цикл заменён новым (rotation/truncate) — для будущих сторис.
    status            text        NOT NULL CHECK (status IN ('pending', 'running', 'succeeded', 'failed', 'needs_retry', 'superseded')),
    -- чекпоинт AD-3: {completed_windows:[…], current_window:{from,to}, page: N}
    checkpoint        jsonb       NOT NULL DEFAULT '{}'::jsonb,
    windows_total     int         NOT NULL DEFAULT 0,
    windows_done      int         NOT NULL DEFAULT 0,
    orders_fetched    bigint      NOT NULL DEFAULT 0,
    events_published  bigint      NOT NULL DEFAULT 0,
    error_hint        text,
    started_at        timestamptz,
    finished_at       timestamptz,
    created_at        timestamptz NOT NULL DEFAULT now(),
    updated_at        timestamptz NOT NULL DEFAULT now(),
    UNIQUE (organization_id, cycle_type)
);

CREATE TRIGGER trg_sync_runs_set_updated_at
    BEFORE UPDATE ON sync_runs
    FOR EACH ROW
    EXECUTE FUNCTION set_updated_at();

-- ── orders: партиционированная проекция канбана (AD-4) ─────────────────────
CREATE TABLE orders (
    id                uuid        NOT NULL DEFAULT gen_random_uuid(),
    organization_id   uuid        NOT NULL REFERENCES organizations (id) ON DELETE CASCADE,
    marketplace       text        NOT NULL CHECK (marketplace IN ('wildberries', 'ozon')),
    external_order_id text        NOT NULL,
    idem_key          text        NOT NULL,
    -- ревизия: монотонный маркер изменения заказа у площадки (WB: epoch-ms
    -- lastChangeDate) — барьер откатов при повторной доставке.
    revision          bigint      NOT NULL,
    scheme            text        NOT NULL CHECK (scheme IN ('fbo', 'fbs', 'realfbs', 'rfbs')),
    board_column      text        NOT NULL CHECK (board_column IN
                      ('new', 'assembling', 'shipping', 'in_transit', 'delivered', 'returned', 'risk')),
    -- сырой статус площадки (диагностика; неизвестный код → колонка 'risk'
    -- + событие, ничего не теряем)
    status_code       text,
    warehouse_name    text,
    total_price       numeric(14, 2) NOT NULL DEFAULT 0,
    currency          char(3)     NOT NULL DEFAULT 'RUB',
    is_cancelled      boolean     NOT NULL DEFAULT false,
    order_date        timestamptz,
    last_change_at    timestamptz,
    platform_payload  jsonb,
    created_at        timestamptz NOT NULL DEFAULT now(),
    updated_at        timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (id, organization_id),
    UNIQUE (organization_id, marketplace, external_order_id),
    UNIQUE (organization_id, idem_key)
) PARTITION BY HASH (organization_id);

CREATE TABLE orders_p0 PARTITION OF orders FOR VALUES WITH (MODULUS 4, REMAINDER 0);
CREATE TABLE orders_p1 PARTITION OF orders FOR VALUES WITH (MODULUS 4, REMAINDER 1);
CREATE TABLE orders_p2 PARTITION OF orders FOR VALUES WITH (MODULUS 4, REMAINDER 2);
CREATE TABLE orders_p3 PARTITION OF orders FOR VALUES WITH (MODULUS 4, REMAINDER 3);

-- Канбан-запрос: колонки организации, свежие сверху (gateway board-хендлер).
CREATE INDEX idx_orders_board
    ON orders (organization_id, board_column, last_change_at DESC);

CREATE TRIGGER trg_orders_set_updated_at
    BEFORE UPDATE ON orders
    FOR EACH ROW
    EXECUTE FUNCTION set_updated_at();

-- ── order_items: позиции (партиций нет — deferred, см. шапку) ──────────────
CREATE TABLE order_items (
    id                uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
    order_id          uuid        NOT NULL,
    organization_id   uuid        NOT NULL,
    -- внешний отпечаток позиции: WB nmId (артикул площадки)
    nm_id             bigint,
    supplier_article  text,
    tech_size         text,
    barcode           text,
    quantity          int         NOT NULL DEFAULT 1,
    price             numeric(14, 2) NOT NULL DEFAULT 0,
    total_price       numeric(14, 2) NOT NULL DEFAULT 0,
    discount_percent  int         NOT NULL DEFAULT 0,
    created_at        timestamptz NOT NULL DEFAULT now(),
    FOREIGN KEY (order_id, organization_id)
        REFERENCES orders (id, organization_id) ON DELETE CASCADE
);

CREATE INDEX idx_order_items_order
    ON order_items (order_id, organization_id);
