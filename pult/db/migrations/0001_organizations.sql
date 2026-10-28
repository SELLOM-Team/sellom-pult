-- Story 1.1 (Structural Seed): organizations — корень владения данными.
-- Конвенции spine: id — UUID (приложение генерирует UUIDv7, DDL-дефолт —
-- gen_random_uuid() как fallback); время — timestamptz UTC; forward-only.
CREATE TABLE organizations (
    id          uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
    name        text        NOT NULL,
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz NOT NULL DEFAULT now()
);
