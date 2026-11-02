//! `pwm-server`: reads its settings from the environment (`PWM_DATABASE_URL`,
//! `PWM_SERVER_SECRETS`, `PWM_LISTEN`, `PWM_TRUSTED_PROXIES`, `PWM_LOG`,
//! `PWM_LOG_FORMAT`), applies pending migrations and serves the API until
//! Ctrl-C or SIGTERM.
//!
//! Subcommands:
//! - `keygen` prints new server secrets for `PWM_SERVER_SECRETS`;
//!   `keygen --env <file>` writes them as `PWM_SERVER_SECRETS=...` to a new
//!   file (and leaves an existing one alone). Keep them in a secret manager
//!   with a backup: losing them locks every account out.
//! - `mark-restored` gives the server a new epoch; run it once after
//!   restoring the database from a backup (D30).
//! - `healthcheck` exits 0 if the server on `PWM_LISTEN` answers
//!   `/healthz` (the container health check).
//! - `reports export [--since YYYY-MM-DD]` writes the autofill failure
//!   reports (D38) to stdout as JSON lines, for triage.
#![forbid(unsafe_code)]

use std::io::{IsTerminal, Write};
use std::net::SocketAddr;
use std::path::Path;
use std::process::ExitCode;

use pwm_server::{
    AppState, Config, DEFAULT_LISTEN, LogFormat, ServerError, app, connect,
    generate_server_secrets, install_panic_hook, log_subscriber, migrate,
};
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("keygen") => return keygen(args.get(1).map(String::as_str), args.get(2)),
        Some("healthcheck") => return healthcheck(),
        Some("reports") => return reports(&args[1..]).await,
        Some("mark-restored") | None => {}
        Some(_) => {
            eprintln!(
                "usage: pwm-server [keygen [--env <file>] | mark-restored | healthcheck | reports export [--since YYYY-MM-DD]]"
            );
            return ExitCode::FAILURE;
        }
    }
    let restore = args.first().map(String::as_str) == Some("mark-restored");
    // sqlx logs statements at debug and slow ones at warn. Every line goes
    // through the scrubber (D29).
    let filter = std::env::var("PWM_LOG").unwrap_or_else(|_| "info,sqlx=warn".to_owned());
    let format = LogFormat::from_env(std::io::stdout().is_terminal());
    let subscriber = log_subscriber(format, &filter, std::io::stdout);
    if tracing::subscriber::set_global_default(subscriber).is_err() {
        eprintln!("logging was already set up");
    }
    install_panic_hook();
    let result = if restore {
        mark_restored().await
    } else {
        run().await
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            tracing::error!("{err}");
            ExitCode::FAILURE
        }
    }
}

fn keygen(flag: Option<&str>, path: Option<&String>) -> ExitCode {
    let secrets = match generate_server_secrets() {
        Ok(secrets) => secrets,
        Err(err) => {
            eprintln!("{err}");
            return ExitCode::FAILURE;
        }
    };
    match (flag, path) {
        (None, _) => {
            println!("{}", secrets.expose_secret());
            ExitCode::SUCCESS
        }
        (Some("--env"), Some(path)) => match write_env(Path::new(path), secrets.expose_secret()) {
            Ok(true) => {
                eprintln!("wrote PWM_SERVER_SECRETS to {path}");
                ExitCode::SUCCESS
            }
            Ok(false) => {
                eprintln!("{path} already exists; keeping it");
                ExitCode::SUCCESS
            }
            Err(err) => {
                eprintln!("cannot write {path}: {err}");
                ExitCode::FAILURE
            }
        },
        _ => {
            eprintln!("usage: pwm-server keygen [--env <file>]");
            ExitCode::FAILURE
        }
    }
}

/// Creates `path` with the secrets, readable by the owner only on Unix.
/// `false` if it already exists, so a second run never replaces secrets.
fn write_env(path: &Path, secrets: &str) -> std::io::Result<bool> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    match options.open(path) {
        Ok(mut file) => {
            writeln!(file, "PWM_SERVER_SECRETS={secrets}")?;
            Ok(true)
        }
        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
        Err(err) => Err(err),
    }
}

fn healthcheck() -> ExitCode {
    let listen = std::env::var("PWM_LISTEN").unwrap_or_else(|_| DEFAULT_LISTEN.to_owned());
    match listen.parse() {
        Ok(addr) if pwm_server::healthcheck(addr) => ExitCode::SUCCESS,
        _ => ExitCode::FAILURE,
    }
}

/// `reports export [--since YYYY-MM-DD]` (D38 §6).
async fn reports(args: &[String]) -> ExitCode {
    let since = match args {
        [cmd] if cmd == "export" => None,
        [cmd, flag, date] if cmd == "export" && flag == "--since" => Some(date.as_str()),
        _ => {
            eprintln!("usage: pwm-server reports export [--since YYYY-MM-DD]");
            return ExitCode::FAILURE;
        }
    };
    let result = async {
        let config = Config::from_env()?;
        let pool = connect(config.database_url.expose_secret())?;
        migrate(&pool).await?;
        let lines = pwm_server::export_reports(&pool, since).await?;
        pool.close().await;
        Ok::<_, ServerError>(lines)
    }
    .await;
    match result {
        Ok(lines) => {
            let mut out = std::io::stdout().lock();
            for line in lines {
                if writeln!(out, "{line}").is_err() {
                    return ExitCode::FAILURE;
                }
            }
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("{err}");
            ExitCode::FAILURE
        }
    }
}

async fn mark_restored() -> Result<(), ServerError> {
    let config = Config::from_env()?;
    let pool = connect(config.database_url.expose_secret())?;
    migrate(&pool).await?;
    let epoch = pwm_server::mark_restored(&pool).await?;
    tracing::info!(%epoch, "new server epoch: every client will pull again and upload what the backup lost");
    pool.close().await;
    Ok(())
}

async fn run() -> Result<(), ServerError> {
    let config = Config::from_env()?;
    let pool = connect(config.database_url.expose_secret())?;
    migrate(&pool).await?;
    tracing::info!("database schema is up to date");

    let listener = TcpListener::bind(config.listen).await?;
    tracing::info!(
        "pwm-server {} listening on {}",
        env!("CARGO_PKG_VERSION"),
        listener.local_addr()?
    );
    let state = AppState::new(pool.clone(), config.server_secrets)
        .with_trusted_proxies(config.trusted_proxies);
    // The peer address is needed for per-IP rate limits.
    axum::serve(
        listener,
        app(state).into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown())
    .await?;
    pool.close().await;
    tracing::info!("stopped");
    Ok(())
}

async fn shutdown() {
    let ctrl_c = async {
        // If the handler can't be installed, rely on SIGTERM alone.
        if tokio::signal::ctrl_c().await.is_err() {
            std::future::pending::<()>().await;
        }
    };
    #[cfg(unix)]
    let terminate = async {
        use tokio::signal::unix::{SignalKind, signal};
        match signal(SignalKind::terminate()) {
            Ok(mut sigterm) => {
                sigterm.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        () = ctrl_c => {}
        () = terminate => {}
    }
    tracing::info!("shutting down");
}
