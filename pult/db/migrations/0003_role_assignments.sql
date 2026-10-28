-- Story 1.1 (Structural Seed): role_assignments — привязка ролей команды.
-- Справочник 5 ролей наполняется Story 1.6; здесь — только каркас хранения
-- (role — текстовая метка без бизнес-ограничений).
CREATE TABLE role_assignments (
    id              uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
    organization_id uuid        NOT NULL REFERENCES organizations (id) ON DELETE CASCADE,
    user_id         uuid        NOT NULL,
    role            text        NOT NULL,
    assigned_at     timestamptz NOT NULL DEFAULT now(),
    UNIQUE (organization_id, user_id, role),
    -- Составной FK: (organization_id, user_id) обязан существовать парой в users —
    -- пользователь из другой организации не может получить роль в этом портале.
    FOREIGN KEY (organization_id, user_id)
        REFERENCES users (organization_id, id) ON DELETE CASCADE
);

CREATE INDEX idx_role_assignments_organization ON role_assignments (organization_id);
CREATE INDEX idx_role_assignments_user ON role_assignments (user_id);
