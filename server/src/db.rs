use std::time::Duration;

use sqlx::PgPool;
use sqlx::migrate::Migrator;
use sqlx::postgres::PgPoolOptions;

use crate::ServerError;

/// Schema migrations (`server/migrations/`), embedded at build time.
pub static MIGRATOR: Migrator = sqlx::migrate!();

const ACQUIRE_TIMEOUT: Duration = Duration::from_secs(3);

/// A connection pool. Connections are opened on first use, so the server
/// can start (and `/healthz` reports 503) while Postgres is unreachable.
pub fn connect(database_url: &str) -> Result<PgPool, ServerError> {
    Ok(PgPoolOptions::new()
        .acquire_timeout(ACQUIRE_TIMEOUT)
        .connect_lazy(database_url)?)
}

/// Applies pending migrations under sqlx's advisory lock, so several
/// instances can start at once. Refuses a database with a migration this
/// build doesn't know (migrated by a newer server) or whose applied
/// migration was edited (checksum mismatch).
pub async fn migrate(pool: &PgPool) -> Result<(), ServerError> {
    Ok(MIGRATOR.run(pool).await?)
}

/// Gives the server a new epoch (D30). Run once after restoring the database
/// from a backup: every client then pulls everything again and uploads what
/// the backup lost. Returns the new epoch.
pub async fn mark_restored(pool: &PgPool) -> Result<uuid::Uuid, ServerError> {
    Ok(
        sqlx::query_scalar("UPDATE server_meta SET epoch = gen_random_uuid() RETURNING epoch")
            .fetch_one(pool)
            .await?,
    )
}

/// `true` if the server listening on `listen` answers `/healthz` with 200
/// within a few seconds. For the container health check, which has no curl.
pub fn healthcheck(listen: std::net::SocketAddr) -> bool {
    use std::io::{Read, Write};
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpStream};

    let ip = match listen.ip() {
        IpAddr::V4(ip) if ip.is_unspecified() => IpAddr::V4(Ipv4Addr::LOCALHOST),
        IpAddr::V6(ip) if ip.is_unspecified() => IpAddr::V6(Ipv6Addr::LOCALHOST),
        ip => ip,
    };
    let timeout = Duration::from_secs(3);
    let Ok(mut stream) = TcpStream::connect_timeout(&SocketAddr::new(ip, listen.port()), timeout)
    else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(timeout));
    let request = b"GET /healthz HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n";
    let mut status = [0u8; 12];
    stream.write_all(request).is_ok()
        && stream.read_exact(&mut status).is_ok()
        && status.starts_with(b"HTTP/1.1 200")
}
