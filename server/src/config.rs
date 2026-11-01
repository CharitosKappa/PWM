use std::net::SocketAddr;

use pwm_crypto::Secret;

use crate::ServerError;

pub const DEFAULT_LISTEN: &str = "127.0.0.1:8080";

/// Settings from the environment. `Debug` never shows the database URL,
/// which holds the database password.
#[derive(Debug)]
pub struct Config {
    pub database_url: Secret<String>,
    pub listen: SocketAddr,
}

impl Config {
    pub fn from_env() -> Result<Self, ServerError> {
        Self::from_vars(|name| std::env::var(name).ok())
    }

    /// Reads `PWM_DATABASE_URL` (required) and `PWM_LISTEN` (default
    /// [`DEFAULT_LISTEN`]) through `var`.
    pub fn from_vars(var: impl Fn(&str) -> Option<String>) -> Result<Self, ServerError> {
        let database_url = var("PWM_DATABASE_URL")
            .filter(|url| !url.is_empty())
            .ok_or(ServerError::Config("PWM_DATABASE_URL is not set"))?;
        let listen = var("PWM_LISTEN")
            .unwrap_or_else(|| DEFAULT_LISTEN.to_owned())
            .parse()
            .map_err(|_| ServerError::Config("PWM_LISTEN is not an address like 127.0.0.1:8080"))?;
        Ok(Self {
            database_url: Secret::new(database_url),
            listen,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const URL: &str = "postgres://pwm:hunter2@db/pwm";

    fn config(vars: &[(&str, &str)]) -> Result<Config, ServerError> {
        Config::from_vars(|name| {
            vars.iter()
                .find(|(n, _)| *n == name)
                .map(|(_, v)| (*v).to_owned())
        })
    }

    #[test]
    fn reads_the_database_url_and_defaults_the_address() {
        let config = config(&[("PWM_DATABASE_URL", URL)]).unwrap();
        assert_eq!(config.database_url.expose_secret(), URL);
        assert_eq!(config.listen, DEFAULT_LISTEN.parse().unwrap());
    }

    #[test]
    fn reads_the_listen_address() {
        let config = config(&[("PWM_DATABASE_URL", URL), ("PWM_LISTEN", "0.0.0.0:9000")]).unwrap();
        assert_eq!(config.listen, "0.0.0.0:9000".parse().unwrap());
    }

    #[test]
    fn rejects_a_missing_url_or_a_bad_address() {
        assert!(matches!(config(&[]), Err(ServerError::Config(_))));
        assert!(matches!(
            config(&[("PWM_DATABASE_URL", "")]),
            Err(ServerError::Config(_))
        ));
        assert!(matches!(
            config(&[("PWM_DATABASE_URL", URL), ("PWM_LISTEN", "localhost")]),
            Err(ServerError::Config(_))
        ));
    }

    #[test]
    fn debug_hides_the_database_password() {
        let config = config(&[("PWM_DATABASE_URL", URL)]).unwrap();
        assert!(!format!("{config:?}").contains("hunter2"));
    }
}
