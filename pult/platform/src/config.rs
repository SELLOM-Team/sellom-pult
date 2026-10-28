//! Базовый конфиг сервиса, читаемый из окружения.
//!
//! Story 1.1: конфиг минимальный (DATABASE_URL, порт, окружение). Никакой
//! бизнес-логики — только чтение и валидация значений.

use std::env::VarError;

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
    /// Строка подключения к PostgreSQL 18 (обязательная `DATABASE_URL`,
    /// сохраняется trim-нутой).
    pub database_url: String,
    /// Порт HTTP-сервиса (`PULT_HTTP_PORT`, по умолчанию 8080; `0` и
    /// не-Unicode значения — ошибка, а не случайный порт).
    pub http_port: u16,
    /// Окружение (`PULT_ENV`, по умолчанию `dev`).
    pub environment: Environment,
}

impl ServiceConfig {
    /// Читает конфиг из переменных окружения.
    ///
    /// - `DATABASE_URL` — обязательна (хранится trim-нутой);
    /// - `PULT_HTTP_PORT` — опциональна, по умолчанию 8080; `0` и
    ///   не-Unicode значения — явная ошибка;
    /// - `PULT_ENV` — опциональна: `dev` (по умолчанию) | `stage` | `prod`.
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_lookup(|name| std::env::var(name))
    }

    /// Тот же разбор, но источник значений подменяемый — детерминированные
    /// юнит-тесты всех веток без мутации общего окружения процесса.
    fn from_lookup(
        get_var: impl Fn(&str) -> Result<String, VarError>,
    ) -> Result<Self, ConfigError> {
        let database_url =
            get_var("DATABASE_URL").map_err(|_| ConfigError::MissingEnv("DATABASE_URL"))?;
        let database_url = database_url.trim().to_owned();
        if database_url.is_empty() {
            return Err(ConfigError::InvalidEnv(
                "DATABASE_URL",
                "пустая строка подключения".to_owned(),
            ));
        }

        let http_port = match get_var("PULT_HTTP_PORT") {
            Ok(raw) => {
                let port = raw
                    .parse::<u16>()
                    .map_err(|err| ConfigError::InvalidEnv("PULT_HTTP_PORT", err.to_string()))?;
                if port == 0 {
                    return Err(ConfigError::InvalidEnv(
                        "PULT_HTTP_PORT",
                        "0 — случайный порт, укажите конкретный".to_owned(),
                    ));
                }
                port
            }
            Err(VarError::NotPresent) => 8080,
            Err(VarError::NotUnicode(_)) => {
                return Err(ConfigError::InvalidEnv(
                    "PULT_HTTP_PORT",
                    "значение не является Unicode".to_owned(),
                ));
            }
        };

        let environment = match get_var("PULT_ENV") {
            Ok(raw) => Environment::parse(&raw)
                .ok_or_else(|| ConfigError::InvalidEnv("PULT_ENV", raw.clone()))?,
            Err(VarError::NotPresent) => Environment::Dev,
            Err(VarError::NotUnicode(_)) => {
                return Err(ConfigError::InvalidEnv(
                    "PULT_ENV",
                    "значение не является Unicode".to_owned(),
                ));
            }
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
    use std::ffi::OsString;

    /// Фейковый источник переменных: без гонок за общее окружение процесса.
    fn lookup<'a>(map: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Result<String, VarError> + 'a {
        move |name: &str| {
            map.iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| (*value).to_owned())
                .ok_or(VarError::NotPresent)
        }
    }

    #[test]
    fn environment_labels_parse() {
        assert_eq!(Environment::parse("dev"), Some(Environment::Dev));
        assert_eq!(Environment::parse("stage"), Some(Environment::Stage));
        assert_eq!(Environment::parse("prod"), Some(Environment::Prod));
        assert_eq!(Environment::parse("production"), None);
    }

    #[test]
    fn defaults_with_only_database_url() {
        let cfg =
            ServiceConfig::from_lookup(lookup(&[("DATABASE_URL", "postgres://pult:pult@db/pult")]))
                .expect("конфиг валиден");
        assert_eq!(cfg.http_port, 8080);
        assert_eq!(cfg.environment, Environment::Dev);
    }

    #[test]
    fn missing_database_url_is_error() {
        let err = ServiceConfig::from_lookup(lookup(&[])).expect_err("DATABASE_URL обязательна");
        assert_eq!(err, ConfigError::MissingEnv("DATABASE_URL"));
    }

    #[test]
    fn blank_database_url_is_error() {
        let err = ServiceConfig::from_lookup(lookup(&[("DATABASE_URL", "   ")]))
            .expect_err("пустая строка подключения невалидна");
        assert_eq!(
            err,
            ConfigError::InvalidEnv("DATABASE_URL", "пустая строка подключения".to_owned())
        );
    }

    #[test]
    fn database_url_is_trimmed() {
        let cfg = ServiceConfig::from_lookup(lookup(&[(
            "DATABASE_URL",
            "  postgres://pult:pult@db/pult \t",
        )]))
        .expect("конфиг валиден");
        assert_eq!(cfg.database_url, "postgres://pult:pult@db/pult");
    }

    #[test]
    fn explicit_port_and_env_are_accepted() {
        let cfg = ServiceConfig::from_lookup(lookup(&[
            ("DATABASE_URL", "postgres://pult:pult@db/pult"),
            ("PULT_HTTP_PORT", "9090"),
            ("PULT_ENV", "prod"),
        ]))
        .expect("конфиг валиден");
        assert_eq!(cfg.http_port, 9090);
        assert_eq!(cfg.environment, Environment::Prod);
    }

    #[test]
    fn invalid_port_is_error() {
        let err = ServiceConfig::from_lookup(lookup(&[
            ("DATABASE_URL", "postgres://pult:pult@db/pult"),
            ("PULT_HTTP_PORT", "abc"),
        ]))
        .expect_err("порт невалиден");
        assert!(matches!(err, ConfigError::InvalidEnv("PULT_HTTP_PORT", _)));
    }

    #[test]
    fn zero_port_is_rejected() {
        let err = ServiceConfig::from_lookup(lookup(&[
            ("DATABASE_URL", "postgres://pult:pult@db/pult"),
            ("PULT_HTTP_PORT", "0"),
        ]))
        .expect_err("порт 0 даёт случайный порт — должен отклоняться");
        assert!(matches!(err, ConfigError::InvalidEnv("PULT_HTTP_PORT", _)));
    }

    #[test]
    fn not_unicode_port_is_error() {
        let err = ServiceConfig::from_lookup(|name| match name {
            "DATABASE_URL" => Ok("postgres://pult:pult@db/pult".to_owned()),
            "PULT_HTTP_PORT" => Err(VarError::NotUnicode(OsString::from("8080"))),
            _ => Err(VarError::NotPresent),
        })
        .expect_err("не-Unicode порт — явная ошибка, а не молчаливый дефолт");
        assert!(matches!(err, ConfigError::InvalidEnv("PULT_HTTP_PORT", _)));
    }

    #[test]
    fn invalid_environment_is_error() {
        let err = ServiceConfig::from_lookup(lookup(&[
            ("DATABASE_URL", "postgres://pult:pult@db/pult"),
            ("PULT_ENV", "production"),
        ]))
        .expect_err("окружение невалидно");
        assert_eq!(
            err,
            ConfigError::InvalidEnv("PULT_ENV", "production".to_owned())
        );
    }
}
