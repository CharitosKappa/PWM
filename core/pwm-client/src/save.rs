//! Save and update offers after a sign-in (D36 §4, §6, §7): whether to
//! offer saving a new login or updating a saved one, and writing either.
//! Platforms only capture the typed values and show the offer.

use pwm_autofill::{Rules, rank, registrable_domain};
use pwm_store::Store;
use pwm_vault::{Item, ItemId, ItemKind, Login, LoginUrl, UrlMatch};

use crate::client::{parse_id, parse_item_id};
use crate::{Client, ClientError};

/// A saved login an update could go to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveCandidate {
    pub vault_id: String,
    pub item_id: String,
    pub title: String,
    pub username: String,
}

/// What to offer after a sign-in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaveOffer {
    /// Saved as is, nothing typed, or a page that must not get one.
    Nothing,
    /// No saved login for this site and username.
    Save,
    /// New password for these logins (the user picks when there are several).
    Update(Vec<SaveCandidate>),
}

/// A saved login as the decision sees it.
pub(crate) struct Saved<'a> {
    pub candidate: SaveCandidate,
    pub urls: &'a [LoginUrl],
    pub username: &'a str,
    pub password: &'a str,
    pub updated_at: i64,
}

fn web_url(text: &str) -> Result<url::Url, ClientError> {
    let url = url::Url::parse(text).map_err(|_| ClientError::InvalidUrl)?;
    match url.scheme() {
        "https" | "http" if url.host_str().is_some() => Ok(url),
        _ => Err(ClientError::InvalidUrl),
    }
}

fn site(url: &url::Url) -> String {
    let host = url.host_str().unwrap_or_default();
    registrable_domain(host).unwrap_or_else(|| host.to_owned())
}

/// Usernames are compared trimmed and without ASCII case: "Ada@x.com" and
/// "ada@x.com " are the same account on practically every site.
fn same_user(a: &str, b: &str) -> bool {
    a.trim().eq_ignore_ascii_case(b.trim())
}

/// The decision (D36 §4), given the client's rules and saved logins.
pub(crate) fn decide(
    rules: &Rules,
    page_url: &str,
    frame_url: Option<&str>,
    saved: &[Saved<'_>],
    username: &str,
    password: &str,
    new_password: Option<&str>,
) -> Result<SaveOffer, ClientError> {
    let page = url::Url::parse(page_url).map_err(|_| ClientError::InvalidUrl)?;
    let frame = frame_url
        .map(|f| url::Url::parse(f).map_err(|_| ClientError::InvalidUrl))
        .transpose()?;
    let target = frame.as_ref().unwrap_or(&page);
    let web = |u: &url::Url| matches!(u.scheme(), "https" | "http") && u.host_str().is_some();
    if !web(&page) || !web(target) {
        return Ok(SaveOffer::Nothing);
    }
    // Another site's frame never gets an offer, like it never gets a fill.
    if frame.as_ref().is_some_and(|f| site(f) != site(&page)) {
        return Ok(SaveOffer::Nothing);
    }
    if rules.site_for(target).is_some_and(|s| s.never_fill) {
        return Ok(SaveOffer::Nothing);
    }
    let new_password = new_password.filter(|p| !p.is_empty());
    let secret = new_password.unwrap_or(password);
    if secret.is_empty() {
        return Ok(SaveOffer::Nothing);
    }

    // The saved logins this page would fill (any decision but a refusal).
    let keyed: Vec<(usize, &[LoginUrl], i64)> = saved
        .iter()
        .enumerate()
        .map(|(i, s)| (i, s.urls, s.updated_at))
        .collect();
    let matching: Vec<&Saved<'_>> = rank(page_url, frame_url, &keyed, rules)
        .map_err(|_| ClientError::InvalidUrl)?
        .into_iter()
        .filter(|c| !matches!(c.decision, pwm_autofill::Decision::Refuse(_)))
        .filter_map(|c| saved.get(c.key))
        .collect();

    let update = |list: Vec<&Saved<'_>>| {
        SaveOffer::Update(list.into_iter().map(|s| s.candidate.clone()).collect())
    };
    let same: Vec<&Saved<'_>> = matching
        .iter()
        .copied()
        .filter(|s| !username.trim().is_empty() && same_user(s.username, username))
        .collect();
    if same.iter().any(|s| s.password == secret) {
        return Ok(SaveOffer::Nothing);
    }
    if !same.is_empty() {
        return Ok(update(same));
    }
    if new_password.is_some() && !password.is_empty() {
        // A change-password form: the old password says which login it is.
        let by_old: Vec<&Saved<'_>> = matching
            .iter()
            .copied()
            .filter(|s| s.password == password)
            .collect();
        if !by_old.is_empty() {
            return Ok(update(by_old));
        }
    }
    if username.trim().is_empty() && matching.iter().any(|s| s.password == secret) {
        return Ok(SaveOffer::Nothing);
    }
    Ok(SaveOffer::Save)
}

/// The title for a new login: the host name without `www.`.
fn default_title(url: &url::Url) -> String {
    let host = url.host_str().unwrap_or_default();
    host.strip_prefix("www.").unwrap_or(host).to_owned()
}

impl<S: Store> Client<S> {
    /// What to offer after a sign-in on `page_url` (and `frame_url` when the
    /// form was in a frame): `new_password` comes from a change-password or
    /// sign-up form. Never fails for pages that must not get an offer; it
    /// answers [`SaveOffer::Nothing`].
    pub fn save_offer(
        &self,
        page_url: &str,
        frame_url: Option<&str>,
        username: &str,
        password: &str,
        new_password: Option<&str>,
    ) -> Result<SaveOffer, ClientError> {
        let engine = self.engine_ref()?;
        let mut items = Vec::new();
        for vault_id in self.vault_ids()? {
            let id = parse_id(&vault_id)?;
            for (item_id, item) in engine.items(&id)? {
                if !item.is_trashed() {
                    items.push((vault_id.clone(), item_id, item));
                }
            }
        }
        let saved: Vec<Saved<'_>> = items
            .iter()
            .filter_map(|(vault_id, item_id, item)| match &item.kind {
                ItemKind::Login(login) => Some(Saved {
                    candidate: SaveCandidate {
                        vault_id: vault_id.clone(),
                        item_id: item_id.to_string(),
                        title: item.title.clone(),
                        username: login.username.clone(),
                    },
                    urls: &login.urls,
                    username: &login.username,
                    password: &login.password,
                    updated_at: item.updated_at,
                }),
                _ => None,
            })
            .collect();
        decide(
            &self.autofill.rules,
            page_url,
            frame_url,
            &saved,
            username,
            password,
            new_password,
        )
    }

    /// Saves a new login for the site of `page_url` (its origin, matched by
    /// domain). An empty `title` becomes the host name. Returns the item id.
    pub fn save_login(
        &mut self,
        vault_id: &str,
        page_url: &str,
        title: &str,
        username: &str,
        password: &str,
        now_ms: i64,
    ) -> Result<String, ClientError> {
        let url = web_url(page_url)?;
        if password.is_empty() {
            return Err(ClientError::InvalidItem);
        }
        let vault = parse_id(vault_id)?;
        let mut login_url = LoginUrl::default();
        login_url.url = url.origin().ascii_serialization();
        login_url.r#match = UrlMatch::Domain;
        let mut login = Login::default();
        login.username = username.trim().to_owned();
        login.password = password.to_owned();
        login.urls = vec![login_url];
        let title = match title.trim() {
            "" => default_title(&url),
            given => given.to_owned(),
        };
        let item = Item::new(ItemKind::Login(login), title, now_ms);
        let item_id = ItemId::generate()?;
        self.engine()?.save_item(&vault, &item_id, &item)?;
        Ok(item_id.to_string())
    }

    /// Replaces a login's password; the previous version goes to history
    /// (D19). Nothing else changes.
    pub fn update_password(
        &mut self,
        vault_id: &str,
        item_id: &str,
        password: &str,
        now_ms: i64,
    ) -> Result<(), ClientError> {
        if password.is_empty() {
            return Err(ClientError::InvalidItem);
        }
        let (vault, id) = (parse_id(vault_id)?, parse_item_id(item_id)?);
        let engine = self.engine()?;
        let mut current = engine.item(&vault, &id)?.ok_or(ClientError::UnknownItem)?;
        let mut edited = current.clone();
        match &mut edited.kind {
            ItemKind::Login(login) => login.password = password.to_owned(),
            _ => return Err(ClientError::InvalidItem),
        }
        current.apply_edit(edited, now_ms)?;
        engine.save_item(&vault, &id, &current)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pwm_autofill::verify_rules;
    use pwm_crypto::{Secret, SignatureContext, public_key_from_seed, sign_with_seed};

    const SEED: [u8; 32] = [7; 32];
    const NOW: i64 = 1_767_225_600_000;

    fn rules_blocking(domain: &str) -> Rules {
        let json = format!(
            r#"{{"v":1,"version":1,"issued_at":{},"expires_at":{},"sites":[{{"domain":"{domain}","never_fill":true}}]}}"#,
            NOW - 1000,
            NOW + 86_400_000
        );
        let message = [&[1u8][..], json.as_bytes()].concat();
        let signature = sign_with_seed(
            &Secret::new(SEED),
            SignatureContext::AutofillRules,
            &message,
        );
        let file = [&[1u8, 1][..], &signature, json.as_bytes()].concat();
        let keys = [(1, public_key_from_seed(&Secret::new(SEED)))];
        verify_rules(&file, &keys, NOW, 0).unwrap()
    }

    #[test]
    fn a_site_marked_never_fill_gets_no_offer() {
        let rules = rules_blocking("blocked.example");
        let offer = |url: &str| decide(&rules, url, None, &[], "ada", "x", None).unwrap();
        assert_eq!(offer("https://login.blocked.example/"), SaveOffer::Nothing);
        assert_eq!(offer("https://open.example/"), SaveOffer::Save);
    }
}
