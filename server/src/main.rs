//! PWM sync server. The axum API, Postgres schema and auth arrive in M3.
#![forbid(unsafe_code)]

fn main() {
    println!("pwm-server {}", env!("CARGO_PKG_VERSION"));
}
