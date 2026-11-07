//! The Linux key holder (D71). `pkexec` runs it as root after polkit's
//! dialog (action `invalid.pwm.unlock`, `auth_self` each time), with the
//! user in `PKEXEC_UID`. For each name it keeps a random 32-byte secret S
//! in `/var/lib/pwm/keys/<uid>/<name>.key`, which only root can read, and
//! it never gives S out:
//!
//! - `create <name>` makes S, or replaces it.
//! - `sign <name>` reads a challenge (64 hex digits) on stdin and writes
//!   the answer, `HKDF-SHA256(S, challenge, "pwm/linux-polkit/response/v1")`
//!   computed by the core, as 64 hex digits.
//!
//! So it stands in for a Windows Hello key: a gesture, then a fixed answer
//! for a fixed challenge, from a secret this user's processes can't read.
//! Exit codes: 0 done, 2 no such secret, 3 a bad request, 4 storage.
#![forbid(unsafe_code)]

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use pwm_crypto::{Secret, linux_polkit_response, linux_polkit_secret};

/// Where the secrets live (D71 §1).
pub const ROOT: &str = "/var/lib/pwm/keys";
/// The format of a secret's file: this byte, then S.
const FILE_VERSION: u8 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Create(String),
    Sign(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// No secret by that name for this user, or a damaged one.
    Unknown,
    /// The folder or the file couldn't be written.
    Storage,
}

impl Error {
    pub fn exit_code(self) -> u8 {
        match self {
            Self::Unknown => 2,
            Self::Storage => 4,
        }
    }
}

/// A name is 1 to 64 of `a-z`, `0-9` and `-`: always a plain file name.
pub fn valid_name(name: &str) -> bool {
    (1..=64).contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// `create <name>` or `sign <name>`, nothing else.
pub fn parse(args: &[String]) -> Option<Command> {
    match args {
        [_, verb, name] if valid_name(name) => match verb.as_str() {
            "create" => Some(Command::Create(name.clone())),
            "sign" => Some(Command::Sign(name.clone())),
            _ => None,
        },
        _ => None,
    }
}

/// The user who asked, from the `PKEXEC_UID` that `pkexec` sets.
pub fn caller_uid(value: Option<&str>) -> Option<u32> {
    let value = value?;
    if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    value.parse().ok()
}

/// A challenge: 64 hex digits, then an optional newline. Reads at most 66
/// bytes.
pub fn read_challenge(input: impl Read) -> Option<[u8; 32]> {
    let mut text = Vec::new();
    input.take(66).read_to_end(&mut text).ok()?;
    let text = text.strip_suffix(b"\n").unwrap_or(&text);
    if text.len() != 64 {
        return None;
    }
    let mut challenge = [0u8; 32];
    for (byte, pair) in challenge.iter_mut().zip(text.chunks(2)) {
        let pair = std::str::from_utf8(pair).ok()?;
        *byte = u8::from_str_radix(pair, 16).ok()?;
    }
    Some(challenge)
}

pub fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Whether this process runs as root: `/proc/self` belongs to the process's
/// effective user.
#[cfg(unix)]
pub fn is_root() -> bool {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata("/proc/self").is_ok_and(|m| m.uid() == 0)
}

pub struct Holder {
    dir: PathBuf,
}

impl Holder {
    pub fn new() -> Self {
        Self::at(PathBuf::from(ROOT))
    }

    pub fn at(dir: PathBuf) -> Self {
        Self { dir }
    }

    fn user_dir(&self, uid: u32) -> PathBuf {
        self.dir.join(uid.to_string())
    }

    fn path(&self, uid: u32, name: &str) -> PathBuf {
        self.user_dir(uid).join(format!("{name}.key"))
    }

    /// Makes a new secret for `name`, replacing any older one.
    pub fn create(&self, uid: u32, name: &str) -> Result<(), Error> {
        let secret = linux_polkit_secret().map_err(|_| Error::Storage)?;
        private_dir(&self.dir)?;
        private_dir(&self.user_dir(uid))?;
        let path = self.path(uid, name);
        let temporary = path.with_extension("tmp");
        let mut contents = Vec::with_capacity(33);
        contents.push(FILE_VERSION);
        contents.extend_from_slice(secret.expose_secret());
        write_private(&temporary, &contents)?;
        std::fs::rename(&temporary, &path).map_err(|_| Error::Storage)
    }

    /// The answer to `challenge` from `name`'s secret.
    pub fn sign(
        &self,
        uid: u32,
        name: &str,
        challenge: &[u8; 32],
    ) -> Result<Secret<[u8; 32]>, Error> {
        let contents = std::fs::read(self.path(uid, name)).map_err(|_| Error::Unknown)?;
        let secret = match contents.split_first() {
            Some((&FILE_VERSION, rest)) if rest.len() == 32 => {
                Secret::<[u8; 32]>::new_with(|s| s.copy_from_slice(rest))
            }
            _ => return Err(Error::Unknown),
        };
        Ok(linux_polkit_response(&secret, challenge))
    }
}

impl Default for Holder {
    fn default() -> Self {
        Self::new()
    }
}

/// Runs one command for `uid`; `input` holds a challenge for `sign`.
pub fn run(
    holder: &Holder,
    uid: u32,
    command: &Command,
    input: impl Read,
    mut output: impl Write,
) -> Result<(), Error> {
    match command {
        Command::Create(name) => holder.create(uid, name),
        Command::Sign(name) => {
            let challenge = read_challenge(input).ok_or(Error::Unknown)?;
            let answer = holder.sign(uid, name, &challenge)?;
            writeln!(output, "{}", to_hex(answer.expose_secret())).map_err(|_| Error::Storage)
        }
    }
}

/// A folder only its owner can open.
fn private_dir(dir: &Path) -> Result<(), Error> {
    std::fs::create_dir_all(dir).map_err(|_| Error::Storage)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
            .map_err(|_| Error::Storage)?;
    }
    Ok(())
}

/// A file only its owner can read, made with that mode from the start.
fn write_private(path: &Path, contents: &[u8]) -> Result<(), Error> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path).map_err(|_| Error::Storage)?;
    file.write_all(contents).map_err(|_| Error::Storage)?;
    file.sync_all().map_err(|_| Error::Storage)
}

#[cfg(test)]
mod tests;
