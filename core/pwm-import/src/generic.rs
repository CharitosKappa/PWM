//! Any CSV with a header row (D51 §4): the core suggests which column holds
//! what from the header names, and the user can change it. Browser exports
//! (Chrome, Edge, Brave, Opera, Firefox, Safari) and Bitwarden's CSV match by
//! their headers and need no change. Every row becomes a login.

use pwm_vault::UrlMatch;

use crate::build::{self, Builder, LeftOut};
use crate::csv;
use crate::{ImportError, Parsed};

/// Which column (by index) holds each field; `None` leaves it out.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ColumnMapping {
    pub title: Option<usize>,
    pub url: Option<usize>,
    pub username: Option<usize>,
    pub password: Option<usize>,
    pub notes: Option<usize>,
    pub totp: Option<usize>,
    pub folder: Option<usize>,
    pub favorite: Option<usize>,
    pub tags: Option<usize>,
}

/// A CSV's header row, with the suggested mapping.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Columns {
    pub headers: Vec<String>,
    pub suggested: ColumnMapping,
    /// Rows after the header.
    pub rows: usize,
}

const TITLE: &[&str] = &[
    "name",
    "title",
    "account",
    "item name",
    "site name",
    "entry",
];
const URL: &[&str] = &[
    "url",
    "login_uri",
    "website",
    "web site",
    "uri",
    "login url",
    "site",
    "web address",
    "link",
];
const USERNAME: &[&str] = &[
    "username",
    "login_username",
    "user name",
    "login",
    "user",
    "email",
    "e-mail",
    "login name",
];
const PASSWORD: &[&str] = &["password", "login_password", "pass", "passwd"];
const NOTES: &[&str] = &[
    "notes",
    "note",
    "comments",
    "comment",
    "extra",
    "description",
];
const TOTP: &[&str] = &[
    "totp",
    "login_totp",
    "otp",
    "otpauth",
    "one-time code",
    "2fa",
    "otp secret",
    "otpurl",
];
const FOLDER: &[&str] = &["folder", "group", "grouping", "category", "collection"];
const FAVORITE: &[&str] = &["favorite", "favourite", "fav"];
const TAGS: &[&str] = &["tags", "tag", "labels"];

/// The header row and a suggested mapping, for the column picker.
pub fn csv_columns(bytes: &[u8]) -> Result<Columns, ImportError> {
    let table = csv::read(bytes)?;
    Ok(Columns {
        suggested: suggest(&table.headers),
        headers: table.headers.clone(),
        rows: table.rows.len(),
    })
}

fn suggest(headers: &[String]) -> ColumnMapping {
    let mut taken: Vec<usize> = Vec::new();
    let mut find = |names: &[&str]| -> Option<usize> {
        let found = names.iter().find_map(|name| {
            headers
                .iter()
                .enumerate()
                .find(|(i, h)| !taken.contains(i) && h.trim().eq_ignore_ascii_case(name))
                .map(|(i, _)| i)
        });
        if let Some(i) = found {
            taken.push(i);
        }
        found
    };
    // Most specific first: "login_password" before a loose "pass".
    let password = find(PASSWORD);
    let totp = find(TOTP);
    let url = find(URL);
    let username = find(USERNAME);
    let title = find(TITLE);
    ColumnMapping {
        title,
        url,
        username,
        password,
        notes: find(NOTES),
        totp,
        folder: find(FOLDER),
        favorite: find(FAVORITE),
        tags: find(TAGS),
    }
}

pub fn parse(
    bytes: &[u8],
    mapping: Option<&ColumnMapping>,
    now_ms: i64,
) -> Result<Parsed, ImportError> {
    let table = csv::read(bytes)?;
    let suggested;
    let mapping = match mapping {
        Some(m) => m,
        None => {
            suggested = suggest(&table.headers);
            &suggested
        }
    };
    let width = table.headers.len();
    let columns = [
        mapping.title,
        mapping.url,
        mapping.username,
        mapping.password,
        mapping.notes,
        mapping.totp,
        mapping.folder,
        mapping.favorite,
        mapping.tags,
    ];
    if columns.iter().flatten().any(|&i| i >= width) {
        return Err(ImportError::Unreadable);
    }
    let mut b = Builder::new(now_ms);
    for row in &table.rows {
        let cell = |col: Option<usize>| col.and_then(|i| row.get(i)).map_or("", |s| s.trim());
        let (title, url, username, password) = (
            cell(mapping.title),
            cell(mapping.url),
            cell(mapping.username),
            cell(mapping.password),
        );
        let (notes, totp) = (cell(mapping.notes), cell(mapping.totp));
        if [title, url, username, password, notes, totp]
            .iter()
            .all(|v| v.is_empty())
        {
            b.leave_out(LeftOut::Empty);
            continue;
        }
        let mut item = build::new_item(build::login_kind(), title, b.now_ms);
        if let Some(login) = build::login(&mut item) {
            login.username = username.to_owned();
            login.password = password.to_owned();
        }
        // Several URLs in one cell, one per line (as our CSV export).
        for url in url.lines() {
            build::add_url(&mut item, url, UrlMatch::Domain);
        }
        build::set_totp(&mut item, totp);
        build::add_note(&mut item, notes);
        item.favorite = matches!(
            cell(mapping.favorite).to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "x"
        );
        item.tags = cell(mapping.tags)
            .split([',', ';', '\n'])
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(str::to_owned)
            .collect();
        item.folder_id = b.folder_path(cell(mapping.folder), '/')?;
        b.push(item)?;
    }
    Ok(b.finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn browser_exports_map_by_their_headers() {
        // Chrome, Edge, Brave and Opera.
        let chrome = suggest(&headers(&["name", "url", "username", "password", "note"]));
        assert_eq!(
            (
                chrome.title,
                chrome.url,
                chrome.username,
                chrome.password,
                chrome.notes
            ),
            (Some(0), Some(1), Some(2), Some(3), Some(4))
        );
        // Firefox.
        let firefox = suggest(&headers(&[
            "url",
            "username",
            "password",
            "httpRealm",
            "formActionOrigin",
            "guid",
        ]));
        assert_eq!(
            (firefox.url, firefox.username, firefox.password),
            (Some(0), Some(1), Some(2))
        );
        assert_eq!(firefox.title, None);
        // Safari.
        let safari = suggest(&headers(&[
            "Title", "URL", "Username", "Password", "Notes", "OTPAuth",
        ]));
        assert_eq!(
            (safari.title, safari.totp, safari.notes),
            (Some(0), Some(5), Some(4))
        );
        // Bitwarden's CSV.
        let bitwarden = suggest(&headers(&[
            "folder",
            "favorite",
            "type",
            "name",
            "notes",
            "fields",
            "reprompt",
            "login_uri",
            "login_username",
            "login_password",
            "login_totp",
        ]));
        assert_eq!(
            (
                bitwarden.url,
                bitwarden.username,
                bitwarden.password,
                bitwarden.totp,
                bitwarden.folder
            ),
            (Some(7), Some(8), Some(9), Some(10), Some(0))
        );
    }
}
