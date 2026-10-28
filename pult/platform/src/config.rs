//! Базовый конфиг сервиса, читаемый из окружения.
//!
//! Story 1.1: конфиг минимальный (DATABASE_URL, порт, окружение). Никакой
//! бизнес-логики — только чтение и валидация значений.

/// Окружение сервиса (метка для логов/метрик; ветвления поведения не задаёт).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Environment {
    /// Локальная разработка (значение по умолчанию).
    Dev,
    /// Промежуточный стенд.
    Stage,
    /// Продакшен.
    Prod,
}

impl Environment {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "dev" => Some(Self::Dev),
            "stage" => Some(Self::Stage),
            "prod" => Some(Self::Prod),
            _ => None,
        }
    }
}

/// Ошибки загрузки конфига.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConfigError {
    /// Обязательная переменная окружения не задана.
    #[error("обязательная переменная окружения {0} не задана")]
    MissingEnv(&'static str),
    /// Значение переменной окружения невалидно.
    #[error("переменная окружения {0} невалидна: {1}")]
    InvalidEnv(&'static str, String),
}

/// Базовый конфиг сервиса «Пульт селлера».
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceConfig {
    /// Строка подключения к PostgreSQL 18 (обязательная `DATABASE_URL`).
    pub database_url: String,
    /// Порт HTTP-сервиса (`PULT_HTTP_PORT`, по умолчанию 8080).
    pub http_port: u16,
    /// Окружение (`PULT_ENV`, по умолчанию `dev`).
    pub environment: Environment,
}

impl ServiceConfig {
    /// Читает конфиг из переменных окружения.
    ///
    /// - `DATABASE_URL` — обязательна;
    /// - `PULT_HTTP_PORT` — опциональна, по умолчанию 8080;
    /// - `PULT_ENV` — опциональна: `dev` (по умолчанию) | `stage` | `prod`.
    pub fn from_env() -> Result<Self, ConfigError> {
        let database_url = std::env::var("DATABASE_URL")
            .map_err(|_| ConfigError::MissingEnv("DATABASE_URL"))?;
        if database_url.trim().is_empty() {
            return Err(ConfigError::InvalidEnv(
                "DATABASE_URL",
                "пустая строка подключения".to_owned(),
            ));
        }

        let http_port = match std::env::var("PULT_HTTP_PORT") {
            Ok(raw) => raw
                .parse::<u16>()
                .map_err(|err| ConfigError::InvalidEnv("PULT_HTTP_PORT", err.to_string()))?,
            Err(_) => 8080,
        };

        let environment = match std::env::var("PULT_ENV") {
            Ok(raw) => Environment::parse(&raw)
                .ok_or_else(|| ConfigError::InvalidEnv("PULT_ENV", raw.clone()))?,
            Err(_) => Environment::Dev,
        };

        Ok(Self {
            database_url,
            http_port,
            environment,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn environment_labels_parse() {
        assert_eq!(Environment::parse("dev"), Some(Environment::Dev));
        assert_eq!(Environment::parse("stage"), Some(Environment::Stage));
        assert_eq!(Environment::parse("prod"), Some(Environment::Prod));
        assert_eq!(Environment::parse("production"), None);
    }
}
