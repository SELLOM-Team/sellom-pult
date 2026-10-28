-- Story 1.1 (Structural Seed): portal_settings — редактируемая конфигурация
-- портала (пороги алертов и т.п. — конфигурация, не код; наполнятся Story 1.6+).
-- Настройки хранятся jsonb-блобом 1:1 к организации до появления конкретных
-- конфигурационных сущностей.
CREATE TABLE portal_settings (
    organization_id uuid        PRIMARY KEY REFERENCES organizations (id) ON DELETE CASCADE,
    settings        jsonb       NOT NULL DEFAULT '{}'::jsonb,
    updated_at      timestamptz NOT NULL DEFAULT now()
);
