/// Normalizes an account email (D26): trimmed and lowercased, at most 254
/// bytes, exactly one `@` with text on both sides, no spaces or control
/// characters. `None` if it is not a usable address.
pub fn normalize_email(email: &str) -> Option<String> {
    let email = email.trim().to_lowercase();
    let (local, domain) = email.split_once('@')?;
    let valid = email.len() <= 254
        && !local.is_empty()
        && !domain.is_empty()
        && !domain.contains('@')
        && !email.chars().any(|c| c.is_whitespace() || c.is_control());
    valid.then_some(email)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trims_and_lowercases() {
        assert_eq!(
            normalize_email("  Alice@Example.COM\n").as_deref(),
            Some("alice@example.com")
        );
        assert_eq!(
            normalize_email("ÉLISE@exemple.fr").as_deref(),
            Some("élise@exemple.fr")
        );
    }

    #[test]
    fn rejects_unusable_addresses() {
        let long = format!("{}@example.com", "a".repeat(243));
        for bad in [
            "",
            "alice",
            "@example.com",
            "alice@",
            "a@b@c",
            "al ice@example.com",
            "alice@exa\u{7}mple.com",
            long.as_str(),
        ] {
            assert_eq!(normalize_email(bad), None, "{bad}");
        }
        assert!(normalize_email(&format!("{}@example.com", "a".repeat(242))).is_some());
    }
}
