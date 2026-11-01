//! Export readers (D24): never panic on a picked file. Accepted JSON items
//! survive another round trip; accepted CSV rows give valid items. `.pwmx`
//! files are fuzzed up to the header: past it, Argon2id runs per input.
#![no_main]

use libfuzzer_sys::fuzz_target;
use pwm_crypto::ExportHeader;
use pwm_export::{read_csv, read_json};
use pwm_vault::Item;

fuzz_target!(|data: &[u8]| {
    let _ = ExportHeader::parse(data);
    if let Ok(export) = read_json(data) {
        for entry in export.vaults.iter().flat_map(|v| &v.items) {
            let json = entry.item.to_json_with_id(&entry.id).unwrap();
            let (id, item) = Item::from_json_with_id(json.expose_secret()).unwrap();
            assert!(id == entry.id && item == entry.item);
        }
    }
    if let Ok(items) = read_csv(data, 0) {
        for entry in &items {
            entry.item.validate().unwrap();
            assert!(entry.item.is_editable());
        }
    }
});
