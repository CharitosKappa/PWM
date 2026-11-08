//! Windows Hello's key (D68), from the `pwm-hello` crate the browser
//! helper (D69) shares.

#[cfg(windows)]
pub use pwm_hello::WindowsHello;
pub use pwm_hello::{FakeHello, FakeState, HelloError, HelloKey, NoHello};
