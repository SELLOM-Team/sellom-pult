-- Story 1.1 (Structural Seed): role_assignments — привязка ролей команды.
-- Справочник 5 ролей наполняется Story 1.6; здесь — только каркас хранения
-- (role — текстовая метка без бизнес-ограничений).
CREATE TABLE role_assignments (
    id              uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
    organization_id uuid        NOT NULL REFERENCES organizations (id) ON DELETE CASCADE,
    user_id         uuid        NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    role            text        NOT NULL,
    assigned_at     timestamptz NOT NULL DEFAULT now(),
    UNIQUE (organization_id, user_id, role)
);

CREATE INDEX idx_role_assignments_organization ON role_assignments (organization_id);
CREATE INDEX idx_role_assignments_user ON role_assignments (user_id);
