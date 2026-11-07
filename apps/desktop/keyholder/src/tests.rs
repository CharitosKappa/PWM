//! The key holder over a temporary folder (D71 §1, §8).

use super::*;

struct Temp(PathBuf);

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn holder(name: &str) -> (Holder, Temp) {
    let dir = std::env::temp_dir().join(format!("pwm-keyholder-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    (Holder::at(dir.join("keys")), Temp(dir))
}

const NAME: &str = "pwm-0123456789abcdef0123456789abcdef";

#[test]
fn the_answer_is_fixed_until_the_secret_is_made_again() {
    let (holder, _temp) = holder("fixed");
    let challenge = [0x6D; 32];
    assert_eq!(
        holder.sign(1000, NAME, &challenge).unwrap_err(),
        Error::Unknown
    );
    holder.create(1000, NAME).unwrap();
    let a = holder.sign(1000, NAME, &challenge).unwrap();
    let b = holder.sign(1000, NAME, &challenge).unwrap();
    assert_eq!(a.expose_secret(), b.expose_secret());
    assert_ne!(
        a.expose_secret(),
        holder
            .sign(1000, NAME, &[0x6E; 32])
            .unwrap()
            .expose_secret()
    );
    // Making it again replaces the secret.
    holder.create(1000, NAME).unwrap();
    assert_ne!(
        a.expose_secret(),
        holder.sign(1000, NAME, &challenge).unwrap().expose_secret()
    );
}

#[test]
fn each_user_has_their_own_secrets() {
    let (holder, _temp) = holder("users");
    holder.create(1000, NAME).unwrap();
    assert_eq!(
        holder.sign(1001, NAME, &[1; 32]).unwrap_err(),
        Error::Unknown
    );
    holder.create(1001, NAME).unwrap();
    assert_ne!(
        holder.sign(1000, NAME, &[1; 32]).unwrap().expose_secret(),
        holder.sign(1001, NAME, &[1; 32]).unwrap().expose_secret()
    );
}

#[test]
fn a_damaged_secret_is_unknown() {
    let (holder, temp) = holder("damaged");
    holder.create(1000, NAME).unwrap();
    let file = temp.0.join("keys").join("1000").join(format!("{NAME}.key"));
    std::fs::write(&file, [9u8; 33]).unwrap();
    assert_eq!(
        holder.sign(1000, NAME, &[1; 32]).unwrap_err(),
        Error::Unknown
    );
    std::fs::write(&file, [1u8; 10]).unwrap();
    assert_eq!(
        holder.sign(1000, NAME, &[1; 32]).unwrap_err(),
        Error::Unknown
    );
}

#[cfg(unix)]
#[test]
fn only_root_could_read_them() {
    use std::os::unix::fs::PermissionsExt;
    let (holder, temp) = holder("modes");
    holder.create(1000, NAME).unwrap();
    let mode = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&temp.0.join("keys")), 0o700);
    assert_eq!(mode(&temp.0.join("keys").join("1000")), 0o700);
    assert_eq!(
        mode(&temp.0.join("keys").join("1000").join(format!("{NAME}.key"))),
        0o600
    );
}

#[test]
fn names_are_plain() {
    for good in [NAME, "pwm-browser-0123456789abcdef0123456789abcdef", "a"] {
        assert!(valid_name(good), "{good}");
    }
    for bad in [
        "",
        "../etc/shadow",
        "PWM",
        "a b",
        "a/b",
        ".",
        &"a".repeat(65),
    ] {
        assert!(!valid_name(bad), "{bad}");
    }
}

#[test]
fn the_command_line_and_the_caller() {
    let args = |list: &[&str]| list.iter().map(|a| (*a).to_owned()).collect::<Vec<_>>();
    assert_eq!(
        parse(&args(&["pwm-keyholder", "create", NAME])),
        Some(Command::Create(NAME.to_owned()))
    );
    assert_eq!(
        parse(&args(&["pwm-keyholder", "sign", NAME])),
        Some(Command::Sign(NAME.to_owned()))
    );
    for bad in [
        &["pwm-keyholder"][..],
        &["pwm-keyholder", "delete", NAME],
        &["pwm-keyholder", "sign", "../x"],
        &["pwm-keyholder", "sign", NAME, "extra"],
    ] {
        assert_eq!(parse(&args(bad)), None);
    }
    assert_eq!(caller_uid(Some("1000")), Some(1000));
    for bad in [None, Some(""), Some("-1"), Some("abc"), Some("1000 ")] {
        assert_eq!(caller_uid(bad), None);
    }
}

#[test]
fn the_challenge_and_the_answer_are_hex() {
    let challenge = read_challenge(format!("{}\n", "6d".repeat(32)).as_bytes()).unwrap();
    assert_eq!(challenge, [0x6D; 32]);
    assert!(read_challenge("6d".repeat(31).as_bytes()).is_none());
    assert!(read_challenge("zz".repeat(32).as_bytes()).is_none());
    assert!(read_challenge("6d".repeat(64).as_bytes()).is_none());
    assert_eq!(to_hex(&[0x00, 0xAB, 0xFF]), "00abff");
}
