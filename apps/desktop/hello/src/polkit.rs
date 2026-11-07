//! Unlock on Linux (D71): the root key holder (`pwm-keyholder`) behind
//! polkit, through `pkexec`, as a [`HelloKey`]. Each `create` and `sign`
//! shows polkit's dialog (the login password, or a fingerprint where PAM is
//! set up for it). The answer goes to the core with Linux's labels
//! ([`Signer::LinuxPolkit`]); nothing here keeps it.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use pwm_crypto::Signer;

use crate::{HelloError, HelloKey, Secret};

/// Where the packages install the key holder (D71 §6).
pub const KEYHOLDER: &str = "/usr/libexec/pwm/pwm-keyholder";
pub const PKEXEC: &str = "/usr/bin/pkexec";

/// `pkexec`'s answer when the user dismissed the dialog.
const DISMISSED: i32 = 126;
/// The key holder's answer when there is no secret by that name.
const UNKNOWN: i32 = 2;

/// What a `pkexec` run gave back.
pub struct Ran {
    pub code: Option<i32>,
    pub stdout: Vec<u8>,
}

/// Runs the key holder through `pkexec`; a fake in tests.
pub trait Pkexec: Send + Sync + 'static {
    /// The key holder and `pkexec` are installed, and this isn't a sandbox.
    fn available(&self) -> bool;
    fn run(&self, args: &[&str], input: &[u8]) -> Option<Ran>;
}

/// The computer's `pkexec` and key holder.
pub struct SystemPkexec;

impl Pkexec for SystemPkexec {
    fn available(&self) -> bool {
        Path::new(KEYHOLDER).is_file()
            && Path::new(PKEXEC).is_file()
            && !Path::new("/.flatpak-info").exists()
    }

    fn run(&self, args: &[&str], input: &[u8]) -> Option<Ran> {
        let mut child = Command::new(PKEXEC)
            .arg(KEYHOLDER)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        child.stdin.take()?.write_all(input).ok()?;
        let output = child.wait_with_output().ok()?;
        Some(Ran {
            code: output.status.code(),
            stdout: output.stdout,
        })
    }
}

pub struct PolkitKey<P: Pkexec = SystemPkexec>(pub P);

impl PolkitKey {
    pub fn system() -> Self {
        Self(SystemPkexec)
    }
}

/// The key holder's name for a key: "PWM browser 0123…" becomes
/// "pwm-browser-0123…". Anything else than letters, digits, spaces and
/// hyphens is refused.
pub fn holder_name(name: &str) -> Option<String> {
    let name = name.to_ascii_lowercase().replace(' ', "-");
    let plain = (1..=64).contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    plain.then_some(name)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex_32(text: &[u8]) -> Option<Vec<u8>> {
    let text = text.strip_suffix(b"\n").unwrap_or(text);
    if text.len() != 64 {
        return None;
    }
    text.chunks(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok())
        .collect()
}

fn failure(code: Option<i32>) -> HelloError {
    match code {
        Some(DISMISSED) => HelloError::Cancelled,
        Some(UNKNOWN) => HelloError::Changed,
        _ => HelloError::Unavailable,
    }
}

impl<P: Pkexec> HelloKey for PolkitKey<P> {
    fn supported(&self) -> bool {
        self.0.available()
    }

    fn create(&self, name: &str) -> Result<(), HelloError> {
        let name = holder_name(name).ok_or(HelloError::Unavailable)?;
        let ran = self
            .0
            .run(&["create", &name], b"")
            .ok_or(HelloError::Unavailable)?;
        match ran.code {
            Some(0) => Ok(()),
            code => Err(failure(code)),
        }
    }

    fn sign(&self, name: &str, data: &[u8]) -> Result<Secret<Vec<u8>>, HelloError> {
        let name = holder_name(name).ok_or(HelloError::Unavailable)?;
        if data.len() != 32 {
            return Err(HelloError::Unavailable);
        }
        let input = format!("{}\n", hex(data));
        let ran = self
            .0
            .run(&["sign", &name], input.as_bytes())
            .ok_or(HelloError::Unavailable)?;
        match ran.code {
            Some(0) => unhex_32(&ran.stdout)
                .map(Secret::new)
                .ok_or(HelloError::Unavailable),
            code => Err(failure(code)),
        }
    }

    /// Nothing to do (D71 §1): without what the user's side keeps, the
    /// secret opens nothing, and the packages remove them all on purge.
    fn delete(&self, _: &str) {}

    fn signer(&self) -> Signer {
        Signer::LinuxPolkit
    }
}

#[cfg(test)]
mod tests;
