-- Story 1.2 (Шаг 0 — фильтр периметра «ваш бизнес»): вердикты шага 0
-- и ключ реестра установок.
--
-- Вердикт иммутабелен per-user: UNIQUE(user_id) — повторный POST возвращает
-- сохранённый вердикт без перезаписи (гонки параллельных сохранений дают
-- один результат). Шаг 0 собирает ноль PII: только торговая модель и
-- (для селлеров) список площадок.
CREATE TABLE perimeter_verdicts (
    id              uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
    organization_id uuid        NOT NULL,
    user_id         uuid        NOT NULL,
    trade_model     text        NOT NULL,
    marketplaces    jsonb       NOT NULL DEFAULT '[]'::jsonb,
    verdict         text        NOT NULL CHECK (verdict IN ('seller', 'non_seller')),
    created_at      timestamptz NOT NULL DEFAULT now(),
    UNIQUE (user_id),
    -- Составной FK (паттерн ревью 1.1): (organization_id, user_id) обязан
    -- существовать парой в users — вердикт нельзя приписать пользователю
    -- другой организации.
    FOREIGN KEY (organization_id, user_id)
        REFERENCES users (organization_id, id) ON DELETE CASCADE
);

CREATE INDEX idx_perimeter_verdicts_organization ON perimeter_verdicts (organization_id);

-- Реестр установок: реальная установка Б24 отразится здесь при ONAPPINSTALL
-- (Story 1.3+). В 1.2 строка organizations создаётся первым placement-запросом
-- (документированная аппроксимация «установки» для Ф0-пилотов). dev-фоллбэк
-- 'dev-portal' — фикстура, из метрик шага 0 исключается.
ALTER TABLE organizations ADD COLUMN b24_member_id text UNIQUE;
