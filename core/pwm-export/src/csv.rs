//! CSV export for moving to another manager (D24): RFC 4180, UTF-8, every
//! field quoted, CRLF rows, one row per login, note, card and identity that
//! is not in the trash. History, trash, passkeys, custom fields, ids,
//! timestamps, URL match modes and unknown fields are left out; the JSON
//! export is the complete format.
//!
//! Formula characters (`=`, `+`, `-`, `@`) are not escaped: that would change
//! passwords starting with them. The UI warns not to open the file in a
//! spreadsheet. `csv-core` reads and writes through buffers owned here, so
//! they are zeroized.

use csv_core::{QuoteStyle, ReadRecordResult, Reader, Terminator, Writer, WriterBuilder};
use pwm_crypto::Secret;
use pwm_vault::{Card, Folder, Identity, Item, ItemId, ItemKind, Login, LoginUrl};
use zeroize::Zeroizing;

use crate::{ExportError, ExportGrant, ExportItem, ExportVault};

/// The header row, in order.
pub const CSV_COLUMNS: [&str; 27] = [
    "type",
    "title",
    "folder",
    "favorite",
    "tags",
    "notes",
    "url",
    "username",
    "password",
    "totp",
    "card_cardholder",
    "card_number",
    "card_expiry_month",
    "card_expiry_year",
    "card_security_code",
    "card_brand",
    "identity_first_name",
    "identity_middle_name",
    "identity_last_name",
    "identity_email",
    "identity_phone",
    "identity_company",
    "identity_address_lines",
    "identity_city",
    "identity_region",
    "identity_postal_code",
    "identity_country",
];

/// Folder nesting deeper than this is cut off (it also stops parent cycles).
const MAX_FOLDER_DEPTH: usize = 32;

fn column(name: &str) -> usize {
    CSV_COLUMNS
        .iter()
        .position(|c| *c == name)
        .expect("known column name")
}

type Row = Vec<Zeroizing<String>>;

/// Plaintext CSV of every active login, note, card and identity.
pub fn export_csv(
    grant: ExportGrant,
    now_ms: i64,
    vaults: &[ExportVault],
) -> Result<Secret<Vec<u8>>, ExportError> {
    grant.redeem(now_ms)?;
    let mut rows: Vec<Row> = vec![
        CSV_COLUMNS
            .iter()
            .map(|c| Zeroizing::new((*c).to_owned()))
            .collect(),
    ];
    for vault in vaults {
        for entry in &vault.items {
            if let Some(row) = item_row(&entry.item, &vault.items) {
                rows.push(row);
            }
        }
    }
    // Two passes: measure, then write into a buffer reserved once, so no
    // reallocated copy of the plaintext is left behind.
    let mut len = 0;
    encode(&rows, &mut |bytes| len += bytes.len());
    Ok(Secret::<Vec<u8>>::new_with(|buf| {
        buf.reserve_exact(len);
        encode(&rows, &mut |bytes| buf.extend_from_slice(bytes));
    }))
}

fn encode(rows: &[Row], emit: &mut dyn FnMut(&[u8])) {
    let mut writer: Writer = WriterBuilder::new()
        .quote_style(QuoteStyle::Always)
        .terminator(Terminator::CRLF)
        .build();
    let mut small = [0u8; 4];
    for row in rows {
        for (n, cell) in row.iter().enumerate() {
            if n > 0 {
                let (_, written) = writer.delimiter(&mut small);
                emit(&small[..written]);
            }
            // Opening quote plus every byte doubled at most.
            let mut out = Zeroizing::new(vec![0u8; 2 * cell.len() + 2]);
            let (_, read, written) = writer.field(cell.as_bytes(), &mut out);
            debug_assert_eq!(read, cell.len());
            emit(&out[..written]);
        }
        let (_, written) = writer.terminator(&mut small);
        emit(&small[..written]);
    }
}

fn item_row(item: &Item, items: &[ExportItem]) -> Option<Row> {
    if item.is_trashed() {
        return None;
    }
    let mut row: Row = (0..CSV_COLUMNS.len())
        .map(|_| Zeroizing::new(String::new()))
        .collect();
    let mut set = |name: &str, value: String| *row[column(name)] = value;
    match &item.kind {
        ItemKind::Login(login) => {
            set("type", "login".into());
            set("url", join(login.urls.iter().map(|u| u.url.as_str())));
            set("username", login.username.clone());
            set("password", login.password.clone());
            set("totp", login.totp.clone().unwrap_or_default());
        }
        ItemKind::Note => set("type", "note".into()),
        ItemKind::Card(card) => {
            set("type", "card".into());
            set("card_cardholder", card.cardholder.clone());
            set("card_number", card.number.clone());
            set(
                "card_expiry_month",
                card.expiry_month.map(|m| m.to_string()).unwrap_or_default(),
            );
            set(
                "card_expiry_year",
                card.expiry_year.map(|y| y.to_string()).unwrap_or_default(),
            );
            set("card_security_code", card.security_code.clone());
            set("card_brand", card.brand.clone());
        }
        ItemKind::Identity(identity) => {
            set("type", "identity".into());
            set("identity_first_name", identity.first_name.clone());
            set("identity_middle_name", identity.middle_name.clone());
            set("identity_last_name", identity.last_name.clone());
            set("identity_email", identity.email.clone());
            set("identity_phone", identity.phone.clone());
            set("identity_company", identity.company.clone());
            set(
                "identity_address_lines",
                join(identity.address_lines.iter().map(String::as_str)),
            );
            set("identity_city", identity.city.clone());
            set("identity_region", identity.region.clone());
            set("identity_postal_code", identity.postal_code.clone());
            set("identity_country", identity.country.clone());
        }
        ItemKind::Folder(_) | ItemKind::Unknown(_) => return None,
    }
    set("title", item.title.clone());
    set("folder", folder_path(item.folder_id, items));
    set("favorite", item.favorite.to_string());
    set("tags", join(item.tags.iter().map(String::as_str)));
    set("notes", item.notes.clone());
    Some(row)
}

/// Several values in one cell, one per line.
fn join<'a>(values: impl Iterator<Item = &'a str>) -> String {
    values.collect::<Vec<_>>().join("\n")
}

/// `Parent/Child` names of the folder and its ancestors.
fn folder_path(mut next: Option<ItemId>, items: &[ExportItem]) -> String {
    let mut names: Vec<&str> = Vec::new();
    let mut seen: Vec<ItemId> = Vec::new();
    while let Some(id) = next {
        if seen.contains(&id) || seen.len() == MAX_FOLDER_DEPTH {
            break;
        }
        seen.push(id);
        let Some(folder) = items.iter().find(|i| i.id == id) else {
            break;
        };
        let ItemKind::Folder(f) = &folder.item.kind else {
            break;
        };
        names.push(&folder.item.title);
        next = f.parent_id;
    }
    names.reverse();
    names.join("/")
}

/// Reads a CSV written by [`export_csv`] into new items with new ids, created
/// at `now_ms`. Folders are recreated from their paths and come first.
pub fn read_csv(bytes: &[u8], now_ms: i64) -> Result<Vec<ExportItem>, ExportError> {
    let mut reader = Reader::new();
    // A record's unescaped data is never longer than the input.
    let mut out = Zeroizing::new(vec![0u8; bytes.len() + 1]);
    let mut ends = [0usize; CSV_COLUMNS.len() + 1];
    let (mut input, mut out_len, mut ends_len, mut row) = (bytes, 0, 0, 0);
    let mut importer = Importer {
        now_ms,
        folders: Vec::new(),
        items: Vec::new(),
    };
    loop {
        let (result, read, written, ended) =
            reader.read_record(input, &mut out[out_len..], &mut ends[ends_len..]);
        input = &input[read..];
        out_len += written;
        ends_len += ended;
        match result {
            ReadRecordResult::InputEmpty => {}
            ReadRecordResult::Record => {
                row += 1;
                let fields = split_fields(&out[..out_len], &ends[..ends_len], row)?;
                if row == 1 {
                    if fields != CSV_COLUMNS {
                        return Err(ExportError::NotAnExport);
                    }
                } else {
                    importer.add(&fields, row)?;
                }
                (out_len, ends_len) = (0, 0);
            }
            ReadRecordResult::OutputEndsFull if row == 0 => return Err(ExportError::NotAnExport),
            ReadRecordResult::OutputEndsFull | ReadRecordResult::OutputFull => {
                return Err(invalid(row + 1, "wrong number of columns"));
            }
            ReadRecordResult::End => break,
        }
    }
    if row == 0 {
        return Err(ExportError::NotAnExport);
    }
    let Importer { folders, items, .. } = importer;
    Ok(folders
        .into_iter()
        .map(|(_, folder)| folder)
        .chain(items)
        .collect())
}

fn invalid(row: usize, reason: &'static str) -> ExportError {
    ExportError::InvalidCsv { row, reason }
}

fn split_fields<'a>(
    data: &'a [u8],
    ends: &[usize],
    row: usize,
) -> Result<Vec<&'a str>, ExportError> {
    let mut start = 0;
    ends.iter()
        .map(|&end| {
            let field = std::str::from_utf8(&data[start..end])
                .map_err(|_| invalid(row, "text is not UTF-8"))?;
            start = end;
            Ok(field)
        })
        .collect()
}

struct Importer {
    now_ms: i64,
    /// Folder path and folder item, in creation order.
    folders: Vec<(Zeroizing<String>, ExportItem)>,
    items: Vec<ExportItem>,
}

impl Importer {
    fn add(&mut self, fields: &[&str], row: usize) -> Result<(), ExportError> {
        if fields.len() != CSV_COLUMNS.len() {
            return Err(invalid(row, "wrong number of columns"));
        }
        let get = |name: &str| fields[column(name)];
        let kind = match get("type") {
            "login" => {
                let mut login = Login::default();
                login.urls = lines(get("url"))
                    .map(|url| {
                        let mut u = LoginUrl::default();
                        u.url = url.to_owned();
                        u
                    })
                    .collect();
                login.username = get("username").to_owned();
                login.password = get("password").to_owned();
                login.totp = Some(get("totp"))
                    .filter(|t| !t.is_empty())
                    .map(str::to_owned);
                ItemKind::Login(login)
            }
            "note" => ItemKind::Note,
            "card" => {
                let mut card = Card::default();
                card.cardholder = get("card_cardholder").to_owned();
                card.number = get("card_number").to_owned();
                card.expiry_month = optional(get("card_expiry_month"))
                    .map(|m| m.parse().ok().filter(|m| (1..=12).contains(m)))
                    .map(|m| m.ok_or(invalid(row, "card expiry month must be 1-12")))
                    .transpose()?;
                card.expiry_year = optional(get("card_expiry_year"))
                    .map(|y| {
                        y.parse()
                            .map_err(|_| invalid(row, "card expiry year must be a number"))
                    })
                    .transpose()?;
                card.security_code = get("card_security_code").to_owned();
                card.brand = get("card_brand").to_owned();
                ItemKind::Card(card)
            }
            "identity" => {
                let mut identity = Identity::default();
                identity.first_name = get("identity_first_name").to_owned();
                identity.middle_name = get("identity_middle_name").to_owned();
                identity.last_name = get("identity_last_name").to_owned();
                identity.email = get("identity_email").to_owned();
                identity.phone = get("identity_phone").to_owned();
                identity.company = get("identity_company").to_owned();
                identity.address_lines = lines(get("identity_address_lines"))
                    .map(str::to_owned)
                    .collect();
                identity.city = get("identity_city").to_owned();
                identity.region = get("identity_region").to_owned();
                identity.postal_code = get("identity_postal_code").to_owned();
                identity.country = get("identity_country").to_owned();
                ItemKind::Identity(identity)
            }
            _ => return Err(invalid(row, "unknown item type")),
        };
        let favorite = match get("favorite") {
            "true" => true,
            "false" | "" => false,
            _ => return Err(invalid(row, "favorite must be true or false")),
        };
        let mut item = Item::new(kind, get("title"), self.now_ms);
        item.notes = get("notes").to_owned();
        item.favorite = favorite;
        item.tags = lines(get("tags")).map(str::to_owned).collect();
        item.folder_id = self.folder(get("folder"))?;
        self.items.push(ExportItem {
            id: ItemId::generate()?,
            item,
        });
        Ok(())
    }

    /// The folder for `Parent/Child`, creating missing levels.
    fn folder(&mut self, path: &str) -> Result<Option<ItemId>, ExportError> {
        let mut parent = None;
        let mut prefix = Zeroizing::new(String::new());
        for name in path.split('/').filter(|n| !n.is_empty()) {
            if !prefix.is_empty() {
                prefix.push('/');
            }
            prefix.push_str(name);
            if let Some((_, existing)) = self.folders.iter().find(|(p, _)| **p == *prefix) {
                parent = Some(existing.id);
                continue;
            }
            let mut folder = Folder::default();
            folder.parent_id = parent;
            let id = ItemId::generate()?;
            let item = Item::new(ItemKind::Folder(folder), name, self.now_ms);
            self.folders.push((
                Zeroizing::new(prefix.as_str().to_owned()),
                ExportItem { id, item },
            ));
            parent = Some(id);
        }
        Ok(parent)
    }
}

fn optional(value: &str) -> Option<&str> {
    Some(value).filter(|v| !v.is_empty())
}

/// Non-empty lines of a multi-value cell (a trailing `\r` is dropped).
fn lines(value: &str) -> impl Iterator<Item = &str> {
    value
        .split('\n')
        .map(|l| l.strip_suffix('\r').unwrap_or(l))
        .filter(|l| !l.is_empty())
}
