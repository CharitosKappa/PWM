//! `pwm-browser-helper` (D69): the browser starts it with the caller in its
//! arguments and one framed message on stdin; it answers once and exits.
//! On Linux it answers `unavailable` until polkit or fprintd (D69 §5).
#![deny(unsafe_code)]

use std::io;
use std::process::ExitCode;

use pwm_browser_helper::{Reply, caller, read_message, write_message};

#[cfg(windows)]
mod front;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let caller = caller(&args);
    let reply = match read_message(io::stdin().lock()) {
        Ok(Some(message)) => answer(&message, caller.as_deref()),
        Ok(None) => return ExitCode::SUCCESS,
        Err(_) => Reply::failed("invalid").to_json(),
    };
    match write_message(io::stdout().lock(), &reply) {
        Ok(()) => ExitCode::SUCCESS,
        Err(_) => ExitCode::FAILURE,
    }
}

#[cfg(windows)]
fn answer(message: &[u8], caller: Option<&str>) -> Vec<u8> {
    use pwm_browser_helper::{Handler, callers};
    let Some(store) = pwm_browser_helper::store::FileStore::new() else {
        return Reply::failed("unavailable").to_json();
    };
    let hello = front::Fronted(pwm_hello::WindowsHello);
    Handler::new(callers(), &hello, &store).handle(message, caller)
}

#[cfg(not(windows))]
fn answer(_message: &[u8], _caller: Option<&str>) -> Vec<u8> {
    Reply::failed("unavailable").to_json()
}
