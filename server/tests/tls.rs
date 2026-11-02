//! Postgres over TLS (D30): rustls with the system CA roots. Needs a server
//! with TLS on, at `PWM_TEST_TLS_DATABASE_URL` (CI starts one with
//! Postgres's self-signed "snakeoil" certificate). Skipped without it,
//! unless `PWM_REQUIRE_TLS_DB=1`.

fn tls_url() -> Option<String> {
    let url = std::env::var("PWM_TEST_TLS_DATABASE_URL").ok();
    if url.is_none() {
        assert!(
            std::env::var("PWM_REQUIRE_TLS_DB").as_deref() != Ok("1"),
            "PWM_REQUIRE_TLS_DB=1 but PWM_TEST_TLS_DATABASE_URL is not set"
        );
        eprintln!("skipped: set PWM_TEST_TLS_DATABASE_URL to run the TLS test");
    }
    url
}

fn with_mode(url: &str, mode: &str) -> String {
    let separator = if url.contains('?') { '&' } else { '?' };
    format!("{url}{separator}sslmode={mode}")
}

#[tokio::test]
async fn connects_over_tls_and_verifies_the_certificate() {
    let Some(url) = tls_url() else {
        return;
    };
    // Encrypted without checking the certificate: the connection is TLS.
    let pool = pwm_server::connect(&with_mode(&url, "require")).unwrap();
    let encrypted: bool =
        sqlx::query_scalar("SELECT ssl FROM pg_stat_ssl WHERE pid = pg_backend_pid()")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(encrypted);
    pool.close().await;

    // Checking it: a self-signed certificate is not in the system roots.
    let pool = pwm_server::connect(&with_mode(&url, "verify-full")).unwrap();
    let refused = sqlx::query("SELECT 1").execute(&pool).await;
    assert!(refused.is_err(), "an untrusted certificate must be refused");
}
