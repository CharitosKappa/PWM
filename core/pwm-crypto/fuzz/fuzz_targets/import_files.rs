//! Importers (D51): never panic on a picked file, whatever format it is read
//! as, and every item they accept is valid and editable. Detection never
//! panics either.
#![no_main]

use libfuzzer_sys::fuzz_target;
use pwm_import::{Format, Options, detect, parse};

fuzz_target!(|data: &[u8]| {
    let _ = detect("file", data);
    let _ = pwm_import::csv_columns(data);
    for format in Format::ALL {
        if let Ok(parsed) = parse(format, data, &Options::default()) {
            for (_, item) in &parsed.items {
                assert!(item.is_editable());
                let json = item.to_json().unwrap();
                assert!(pwm_vault::Item::from_json(json.expose_secret()).is_ok());
            }
        }
    }
});
