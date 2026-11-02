//! `pwm-server`: reads its settings from the environment (`PWM_DATABASE_URL`,
//! `PWM_SERVER_SECRETS`, `PWM_LISTEN`, `PWM_TRUSTED_PROXIES`, `PWM_LOG`,
//! `PWM_LOG_FORMAT`), applies pending migrations and serves the API until
//! Ctrl-C or SIGTERM.
//!
//! `pwm-server keygen` prints new server secrets for `PWM_SERVER_SECRETS`.
//! Keep them in a secret manager with a backup: losing them locks every
//! account out.
#![forbid(unsafe_code)]

use std::io::IsTerminal;
use std::net::SocketAddr;
use std::process::ExitCode;

use pwm_server::{
    AppState, Config, LogFormat, ServerError, app, connect, generate_server_secrets,
    install_panic_hook, log_subscriber, migrate,
};
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> ExitCode {
    if std::env::args().nth(1).as_deref() == Some("keygen") {
        return keygen();
    }
    // sqlx logs statements at debug and slow ones at warn. Every line goes
    // through the scrubber (D29).
    let filter = std::env::var("PWM_LOG").unwrap_or_else(|_| "info,sqlx=warn".to_owned());
    let format = LogFormat::from_env(std::io::stdout().is_terminal());
    let subscriber = log_subscriber(format, &filter, std::io::stdout);
    if tracing::subscriber::set_global_default(subscriber).is_err() {
        eprintln!("logging was already set up");
    }
    install_panic_hook();
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            tracing::error!("{err}");
            ExitCode::FAILURE
        }
    }
}

fn keygen() -> ExitCode {
    match generate_server_secrets() {
        Ok(secrets) => {
            println!("{}", secrets.expose_secret());
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("{err}");
            ExitCode::FAILURE
        }
    }
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
