//! Digital Asset Links files (D61 Â§4): any file a site serves never panics
//! the reader, lists an app only when a statement names it exactly, and
//! includes only HTTPS addresses on a domain, at most 10. The app path's
//! RP id rules hold for any RP id a request names.
#![no_main]

use libfuzzer_sys::fuzz_target;
use pwm_crypto::Secret;
use pwm_passkey::{
    AndroidCaller, CreateRequest, GetRequest, MAX_INCLUDES, asset_links_includes,
    authenticate_android, read_asset_links, register_android,
};

const PACKAGE: &str = "com.example.app";
const CERT: [u8; 32] = [0xAB; 32];

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    if let Ok(read) = read_asset_links(text, PACKAGE, &CERT) {
        assert!(read.includes.len() <= MAX_INCLUDES);
        assert!(read.includes.iter().all(|u| u.starts_with("https://")));
        assert_eq!(asset_links_includes(text).unwrap(), read.includes);
        // Listing needs the package's name in the file.
        assert!(!read.linked || text.contains(PACKAGE));
    }
    // The text as an RP id, for an app's request.
    let caller = AndroidCaller::App {
        package: PACKAGE,
        cert_sha256: &CERT,
    };
    let create = format!(
        r#"{{"challenge":"AAAA","rp":{{"id":{}}},"user":{{"id":"AA","name":"a"}},"pubKeyCredParams":[{{"type":"public-key","alg":-7}}]}}"#,
        serde_json::json!(text)
    );
    if let Ok(request) = CreateRequest::from_json(&create)
        && let Ok(registration) =
            register_android(caller, &request, &Secret::new([0x42; 32]), [7; 16])
    {
        let get = format!(
            r#"{{"challenge":"AAAA","rpId":{}}}"#,
            serde_json::json!(registration.passkey.rp_id)
        );
        let get = GetRequest::from_json(&get).unwrap();
        assert!(authenticate_android(caller, &get, &registration.passkey).is_ok());
    }
});
