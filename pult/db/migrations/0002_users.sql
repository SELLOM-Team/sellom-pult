-- Story 1.1 (Structural Seed): users — участники портала.
-- Никаких паролей и собственной аутентификации (NFR-6): вход — только через
-- контур Б24. external_ref — точка интеграции с порталом Б24 (не реализация).
CREATE TABLE users (
    id              uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
    organization_id uuid        NOT NULL REFERENCES organizations (id) ON DELETE CASCADE,
    display_name    text        NOT NULL,
    external_ref    text,
    created_at      timestamptz NOT NULL DEFAULT now(),
    updated_at      timestamptz NOT NULL DEFAULT now(),
    UNIQUE (organization_id, external_ref),
    -- Составная цель для FK из role_assignments: гарантирует на уровне БД,
    -- что роль нельзя выдать пользователю ДРУГОЙ организации
    -- (кросс-орг утечка прав исключена композитным ключом, а не приложением).
    UNIQUE (organization_id, id)
);

CREATE INDEX idx_users_organization ON users (organization_id);
