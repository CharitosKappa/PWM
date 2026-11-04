use std::net::SocketAddr;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use pwm_crypto::{Secret, ServerSecrets};

use crate::ServerError;
use crate::limits::TrustedProxies;
use crate::push::{FcmConfig, PushConfig};

pub const DEFAULT_LISTEN: &str = "127.0.0.1:8080";

/// Settings from the environment. `Debug` never shows the database URL
/// (it holds the database password) or the server secrets.
#[derive(Debug)]
pub struct Config {
    pub database_url: Secret<String>,
    pub listen: SocketAddr,
    pub server_secrets: ServerSecrets,
    /// Proxies whose `X-Forwarded-For` gives the client IP (D29).
    pub trusted_proxies: TrustedProxies,
    /// APNs (D50 §3), from `PWM_APNS_*`; `None` leaves push off.
    pub push: Option<PushConfig>,
    /// FCM (D60), from `PWM_FCM_*`; `None` leaves Android push off.
    pub fcm: Option<FcmConfig>,
}

impl Config {
    pub fn from_env() -> Result<Self, ServerError> {
        Self::from_vars(|name| std::env::var(name).ok())
    }

    /// Reads `PWM_DATABASE_URL` and `PWM_SERVER_SECRETS` (both required),
    /// `PWM_LISTEN` (default [`DEFAULT_LISTEN`]) and `PWM_TRUSTED_PROXIES`
    /// (addresses and CIDR ranges, default none) through `var`, and the
    /// optional `PWM_APNS_*` settings ([`PushConfig::from_vars`]).
    pub fn from_vars(var: impl Fn(&str) -> Option<String>) -> Result<Self, ServerError> {
        let database_url = var("PWM_DATABASE_URL")
            .filter(|url| !url.is_empty())
            .ok_or(ServerError::Config("PWM_DATABASE_URL is not set"))?;
        let listen = var("PWM_LISTEN")
            .unwrap_or_else(|| DEFAULT_LISTEN.to_owned())
            .parse()
            .map_err(|_| ServerError::Config("PWM_LISTEN is not an address like 127.0.0.1:8080"))?;
        let secrets =
            var("PWM_SERVER_SECRETS")
                .filter(|s| !s.is_empty())
                .ok_or(ServerError::Config(
                    "PWM_SERVER_SECRETS is not set (create one with `pwm-server keygen`)",
                ))?;
        let server_secrets = decode_server_secrets(&secrets)?;
        let trusted_proxies = TrustedProxies::parse(
            &var("PWM_TRUSTED_PROXIES").unwrap_or_default(),
        )
        .map_err(|_| {
            ServerError::Config("PWM_TRUSTED_PROXIES is not a list like 10.0.0.0/8, 192.0.2.1")
        })?;
        let push = PushConfig::from_vars(&var)?;
        let fcm = FcmConfig::from_vars(&var)?;
        Ok(Self {
            database_url: Secret::new(database_url),
            listen,
            server_secrets,
            trusted_proxies,
            push,
            fcm,
        })
    }
}

/// New server secrets as base64url, for `pwm-server keygen`.
pub fn generate_server_secrets() -> Result<Secret<String>, ServerError> {
    let secrets = ServerSecrets::generate()
        .map_err(|_| ServerError::Config("the OS random number generator failed"))?;
    let bytes = secrets.to_bytes();
    Ok(Secret::new(URL_SAFE_NO_PAD.encode(bytes.expose_secret())))
}

fn decode_server_secrets(text: &str) -> Result<ServerSecrets, ServerError> {
    let invalid = ServerError::Config("PWM_SERVER_SECRETS is not a value from `pwm-server keygen`");
    let bytes = Secret::new(URL_SAFE_NO_PAD.decode(text.trim()).map_err(|_| invalid)?);
    ServerSecrets::from_bytes(bytes.expose_secret()).map_err(|_| {
        ServerError::Config("PWM_SERVER_SECRETS is not a value from `pwm-server keygen`")
    })
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

    fn secrets() -> String {
        generate_server_secrets().unwrap().expose_secret().clone()
    }

    #[test]
    fn reads_the_settings_and_defaults_the_address() {
        let secrets = secrets();
        let config =
            config(&[("PWM_DATABASE_URL", URL), ("PWM_SERVER_SECRETS", &secrets)]).unwrap();
        assert_eq!(config.database_url.expose_secret(), URL);
        assert_eq!(config.listen, DEFAULT_LISTEN.parse().unwrap());
    }

    #[test]
    fn reads_the_listen_address() {
        let secrets = secrets();
        let config = config(&[
            ("PWM_DATABASE_URL", URL),
            ("PWM_SERVER_SECRETS", &secrets),
            ("PWM_LISTEN", "0.0.0.0:9000"),
        ])
        .unwrap();
        assert_eq!(config.listen, "0.0.0.0:9000".parse().unwrap());
    }

    #[test]
    fn reads_trusted_proxies() {
        let secrets = secrets();
        let base = [
            ("PWM_DATABASE_URL", URL),
            ("PWM_SERVER_SECRETS", secrets.as_str()),
        ];
        assert_eq!(
            config(&base).unwrap().trusted_proxies,
            TrustedProxies::default()
        );
        let with = |value| {
            let mut vars = base.to_vec();
            vars.push(("PWM_TRUSTED_PROXIES", value));
            config(&vars)
        };
        assert_eq!(
            with("10.0.0.0/8, 192.0.2.1").unwrap().trusted_proxies,
            TrustedProxies::parse("10.0.0.0/8,192.0.2.1").unwrap()
        );
        assert!(matches!(with("10.0.0.0/40"), Err(ServerError::Config(_))));
    }

    #[test]
    fn rejects_missing_or_bad_settings() {
        let secrets = secrets();
        for vars in [
            vec![("PWM_SERVER_SECRETS", secrets.as_str())],
            vec![("PWM_DATABASE_URL", ""), ("PWM_SERVER_SECRETS", &secrets)],
            vec![("PWM_DATABASE_URL", URL)],
            vec![
                ("PWM_DATABASE_URL", URL),
                ("PWM_SERVER_SECRETS", "not base64!"),
            ],
            vec![("PWM_DATABASE_URL", URL), ("PWM_SERVER_SECRETS", "AQID")],
            vec![
                ("PWM_DATABASE_URL", URL),
                ("PWM_SERVER_SECRETS", &secrets),
                ("PWM_LISTEN", "localhost"),
            ],
        ] {
            assert!(
                matches!(config(&vars), Err(ServerError::Config(_))),
                "{vars:?}"
            );
        }
    }

    #[test]
    fn debug_hides_the_database_password_and_secrets() {
        let secrets = secrets();
        let config =
            config(&[("PWM_DATABASE_URL", URL), ("PWM_SERVER_SECRETS", &secrets)]).unwrap();
        let debug = format!("{config:?}");
        assert!(!debug.contains("hunter2"));
        assert!(!debug.contains(&secrets[..12]));
    }
}
