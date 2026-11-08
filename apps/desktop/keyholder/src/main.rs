//! `pwm-keyholder` (D71): run only through `pkexec`, as root, for the user
//! in `PKEXEC_UID`. Anything else is refused with exit code 3.
#![forbid(unsafe_code)]

use std::process::ExitCode;

#[cfg(unix)]
fn main() -> ExitCode {
    use pwm_keyholder::{Holder, caller_uid, is_root, parse, run};
    let args: Vec<String> = std::env::args().collect();
    let uid = caller_uid(std::env::var("PKEXEC_UID").ok().as_deref());
    let (Some(command), Some(uid), true) = (parse(&args), uid, is_root()) else {
        return ExitCode::from(3);
    };
    match run(
        &Holder::new(),
        uid,
        &command,
        std::io::stdin().lock(),
        std::io::stdout().lock(),
    ) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => ExitCode::from(error.exit_code()),
    }
}

/// Linux only.
#[cfg(not(unix))]
fn main() -> ExitCode {
    ExitCode::from(3)
}
