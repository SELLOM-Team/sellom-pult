-- Story 1.3 (мастер подключения площадок с token-vault, FR-2 / AD-16):
-- подключения кабинетов маркетплейсов.
--
-- Инварианты спеки:
-- - токен хранится ТОЛЬКО шифротекстом: пара bytea (token_ciphertext +
--   token_nonce, AES-256-GCM) и key_version для будущей ротации
--   master-ключа (re-encrypt job — Ф1+); plaintext-колонок токенов нет
--   (CI-гейт vault-grep ловит их появление);
-- - Ozon Client-Id — НЕ секрет: хранится открыто (client_id);
-- - Ф0-квота: один кабинет на площадку в портале — UNIQUE(organization_id,
--   marketplace); ротация токена = перезапись этой же строки (INSERT …
--   ON CONFLICT DO UPDATE на стороне шлюза), несвязанные кабинеты —
--   разные порталы;
-- - в Ф0 шлюз пишет только status='verified' (неуспешный автотест прав
--   ничего не сохраняет); остальные статусы CHECK допускает для будущих
--   состояний, но запись их в 1.3 отсутствует.
CREATE TABLE marketplace_connections (
    id               uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
    organization_id  uuid        NOT NULL REFERENCES organizations (id) ON DELETE CASCADE,
    marketplace      text        NOT NULL CHECK (marketplace IN ('wildberries', 'ozon')),
    schemes          jsonb       NOT NULL,
    status           text        NOT NULL CHECK (status IN ('verified', 'rejected', 'probe_unavailable')),
    client_id        text,
    token_ciphertext bytea       NOT NULL,
    token_nonce      bytea       NOT NULL,
    key_version      int         NOT NULL DEFAULT 1,
    rights_snapshot  jsonb,
    verified_at      timestamptz,
    created_at       timestamptz NOT NULL DEFAULT now(),
    updated_at       timestamptz NOT NULL DEFAULT now(),
    UNIQUE (organization_id, marketplace)
);

-- Первая UPDATE-сторис (deferred из ревью Story 1.1): механизм updated_at
-- решён триггером — ЛЮБОЙ UPDATE строки подключения обновляет updated_at
-- (проверка CI-psql в Story 1.3). Функция общая: следующие сторис с
-- updated_at-таблицами переиспользуют её, создавая свои триггеры.
CREATE OR REPLACE FUNCTION set_updated_at() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    NEW.updated_at := now();
    RETURN NEW;
END;
$$;

CREATE TRIGGER trg_marketplace_connections_set_updated_at
    BEFORE UPDATE ON marketplace_connections
    FOR EACH ROW
    EXECUTE FUNCTION set_updated_at();
