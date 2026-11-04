//! CSV exports: RFC 4180 through `csv-core` (as `pwm-export`), into a table
//! whose cells are wiped when dropped. The first row is the header.

use csv_core::{ReadRecordResult, Reader};
use zeroize::{Zeroize, Zeroizing};

use crate::ImportError;

/// Columns per row we accept.
const MAX_COLUMNS: usize = 256;

/// A CSV file: its header and rows. Every cell is zeroized on drop.
pub struct Table {
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

impl Drop for Table {
    fn drop(&mut self) {
        for row in &mut self.rows {
            row.iter_mut().for_each(Zeroize::zeroize);
        }
    }
}

impl Table {
    /// The index of the first header named any of `names` (trimmed,
    /// case-insensitive).
    pub fn column(&self, names: &[&str]) -> Option<usize> {
        names.iter().find_map(|name| {
            self.headers
                .iter()
                .position(|h| h.trim().eq_ignore_ascii_case(name))
        })
    }

    /// The cell of `row` in the column named any of `names`, trimmed.
    pub fn get<'a>(&self, row: &'a [String], names: &[&str]) -> &'a str {
        self.column(names)
            .and_then(|i| row.get(i))
            .map_or("", |s| s.trim())
    }
}

pub fn strip_bom(bytes: &[u8]) -> &[u8] {
    bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes)
}

/// Reads the whole file. Rows that are entirely empty are dropped.
pub fn read(bytes: &[u8]) -> Result<Table, ImportError> {
    let mut rows = read_rows(strip_bom(bytes), usize::MAX)?;
    if rows.is_empty() {
        return Err(ImportError::Unreadable);
    }
    let headers = rows.remove(0);
    rows.retain(|r| r.iter().any(|c| !c.trim().is_empty()));
    Ok(Table { headers, rows })
}

/// Only the header row, for detection.
pub fn headers(bytes: &[u8]) -> Option<Vec<String>> {
    let mut rows = read_rows(bytes, 1).ok()?;
    let headers = rows.pop()?;
    (headers.len() > 1).then_some(headers)
}

fn read_rows(bytes: &[u8], max_rows: usize) -> Result<Vec<Vec<String>>, ImportError> {
    let mut reader = Reader::new();
    // A record's unescaped data is never longer than the input.
    let mut out = Zeroizing::new(vec![0u8; bytes.len() + 1]);
    let mut ends = [0usize; MAX_COLUMNS];
    let (mut input, mut out_len, mut ends_len) = (bytes, 0, 0);
    let mut rows = Vec::new();
    loop {
        let (result, read, written, ended) =
            reader.read_record(input, &mut out[out_len..], &mut ends[ends_len..]);
        input = &input[read..];
        out_len += written;
        ends_len += ended;
        match result {
            ReadRecordResult::InputEmpty => {}
            ReadRecordResult::Record => {
                let mut row = Vec::with_capacity(ends_len);
                let mut start = 0;
                for &end in &ends[..ends_len] {
                    let cell = std::str::from_utf8(&out[start..end])
                        .map_err(|_| ImportError::Unreadable)?;
                    row.push(cell.to_owned());
                    start = end;
                }
                rows.push(row);
                if rows.len() >= max_rows {
                    break;
                }
                if rows.len() > crate::MAX_ITEMS + 1 {
                    return Err(ImportError::TooLarge);
                }
                (out_len, ends_len) = (0, 0);
            }
            ReadRecordResult::OutputFull | ReadRecordResult::OutputEndsFull => {
                return Err(ImportError::Unreadable);
            }
            ReadRecordResult::End => break,
        }
    }
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_quotes_newlines_and_a_bom() {
        let table = read(b"\xEF\xBB\xBFname,notes\r\n\"a, b\",\"line 1\nline 2\"\n,\n\"x\"\"y\",z")
            .unwrap();
        assert_eq!(table.headers, ["name", "notes"]);
        assert_eq!(table.rows.len(), 2, "the empty row is dropped");
        assert_eq!(table.rows[0], ["a, b", "line 1\nline 2"]);
        assert_eq!(table.rows[1], ["x\"y", "z"]);
        assert_eq!(table.column(&["NOTES"]), Some(1));
    }

    #[test]
    fn refuses_what_is_not_text() {
        assert!(read(b"a,b\n\xff\xfe,c").is_err());
        assert!(read(b"").is_err());
        assert_eq!(headers(b"just one line"), None);
    }
}
