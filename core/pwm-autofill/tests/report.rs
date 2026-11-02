//! "Autofill didn't work here" reports (D38 §3, §4): what a report holds,
//! how text is cleaned, and the limits. Nothing typed and no URL path may
//! ever be in one.

use pwm_autofill::{
    Class, Field, FieldClass, FrameRelation, Problem, ReportError, ReportFrame, ReportInput,
    build_report, clean_text,
};
use serde_json::Value;

fn field(name: &str, label: &str) -> Field {
    Field {
        tag: "input".into(),
        r#type: "text".into(),
        name: name.into(),
        id: name.into(),
        label: label.into(),
        visible: true,
        form: Some(0),
        ..Field::default()
    }
}

fn input(frames: Vec<ReportFrame>) -> ReportInput {
    ReportInput {
        page_url: "https://login.example.com/signin?token=SECRET123&next=/home#frag".into(),
        problem: Problem::NothingFilled,
        frames,
        extension_version: "0.1.0".into(),
        rules_version: 7,
        browser: "Chrome 140".into(),
        matched: Some(true),
    }
}

fn frame(fields: Vec<Field>, fill: Vec<usize>) -> ReportFrame {
    let classes = fields
        .iter()
        .map(|_| FieldClass {
            class: Class::Username,
            confidence: 0.9,
        })
        .collect();
    ReportFrame {
        relation: FrameRelation::Top,
        fields,
        classes,
        fill,
    }
}

fn report(input: ReportInput) -> Value {
    serde_json::from_str(&build_report(&input).unwrap()).unwrap()
}

#[test]
fn text_is_cleaned_of_personal_looking_data() {
    assert_eq!(clean_text("  Hello  ADA,\n Sign In "), "hello ada, sign in");
    assert_eq!(
        clean_text("Email ada@example.com here"),
        "email <email> here"
    );
    // Punctuation around it stays; the address does not.
    assert_eq!(clean_text("(ada.l@x.co.uk)"), "(<email>)");
    assert_eq!(clean_text("Card 4242 4242 4242 4242"), "card # # # #");
    assert_eq!(clean_text("user_12345 or 12 or 007"), "user_# or 12 or #");
    assert_eq!(
        clean_text("session eyJhbGciOiJIUzI1NiJ9abcdef"),
        "session <token>"
    );
    // A long word with no digits is a word, not a token.
    assert_eq!(
        clean_text("Passwortwiederherstellungsfunktion"),
        "passwortwiederherstellungsfunktion"
    );
    // At most 40 characters, cut on a character boundary.
    let long = clean_text(&"ψ".repeat(100));
    assert_eq!(long.chars().count(), 40);
    assert_eq!(clean_text(""), "");
}

#[test]
fn a_report_holds_the_origin_structure_and_decisions_only() {
    let mut user = field("email", "Email address");
    user.autocomplete = "username".into();
    user.max_length = Some(64);
    let mut pass = field("password", "Password");
    pass.r#type = "password".into();
    let mut f = frame(vec![user, pass], vec![0, 1]);
    f.classes[1] = FieldClass {
        class: Class::CurrentPassword,
        confidence: 0.95,
    };
    let r = report(input(vec![f]));
    assert_eq!(r["v"], 1);
    assert_eq!(r["site"], "https://login.example.com");
    assert_eq!(r["problem"], "nothing_filled");
    assert_eq!(r["matched"], true);
    assert_eq!(r["software"]["extension"], "0.1.0");
    assert_eq!(r["software"]["rules"], 7);
    assert_eq!(r["software"]["browser"], "Chrome 140");
    assert!(r["software"]["core"].is_string());
    let fields = &r["frames"][0]["fields"];
    assert_eq!(r["frames"][0]["relation"], "top");
    assert_eq!(fields[0]["autocomplete"], "username");
    assert_eq!(fields[0]["max_length"], 64);
    assert_eq!(fields[0]["class"], "username");
    assert_eq!(fields[0]["fill"], true);
    assert_eq!(fields[1]["type"], "password");
    assert_eq!(fields[1]["class"], "current-password");
    let text = r.to_string();
    for absent in ["SECRET123", "/signin", "next=", "frag", "value"] {
        assert!(!text.contains(absent), "{absent} in {text}");
    }
}

#[test]
fn labels_names_and_ids_are_cleaned() {
    let mut f = field("user_98765", "Hello Ada (ada@example.com), sign in");
    f.placeholder = "e.g. 555-123-4567".into();
    f.aria_label = "token abcdefghij0123456789xyz".into();
    let r = report(input(vec![frame(vec![f], vec![])]));
    let got = &r["frames"][0]["fields"][0];
    assert_eq!(got["name"], "user_#");
    assert_eq!(got["id"], "user_#");
    assert_eq!(got["label"], "hello ada (<email>), sign in");
    assert_eq!(got["placeholder"], "e.g. #-#-#");
    assert_eq!(got["aria_label"], "token <token>");
}

#[test]
fn frames_say_how_they_relate_never_their_url() {
    let mut other = frame(vec![field("q", "")], vec![]);
    other.relation = FrameRelation::OtherSite;
    let mut same = frame(vec![field("q", "")], vec![]);
    same.relation = FrameRelation::SameSite;
    let r = report(input(vec![same, other]));
    assert_eq!(r["frames"][0]["relation"], "same_site");
    assert_eq!(r["frames"][1]["relation"], "other_site");
}

#[test]
fn every_problem_has_a_name() {
    for (problem, name) in [
        (Problem::NothingFilled, "nothing_filled"),
        (Problem::WrongField, "wrong_field"),
        (Problem::NoSaveOffer, "no_save_offer"),
        (Problem::IconInTheWay, "icon_in_the_way"),
        (Problem::Other, "other"),
    ] {
        assert_eq!(problem.as_str(), name);
        assert_eq!(Problem::parse(name), Some(problem));
    }
    assert_eq!(Problem::parse("free text"), None);
    let mut locked = input(vec![]);
    locked.matched = None;
    assert!(report(locked)["matched"].is_null());
}

#[test]
fn limits_keep_reports_small() {
    // 20 frames and 200 inputs at most, and 32 KiB: the rest is left out.
    let many: Vec<ReportFrame> = (0..30)
        .map(|_| {
            frame(
                (0..15).map(|i| field(&format!("f{i}"), "label")).collect(),
                vec![],
            )
        })
        .collect();
    let r = report(input(many));
    let frames = r["frames"].as_array().unwrap();
    assert!(frames.len() <= 20);
    let total: usize = frames
        .iter()
        .map(|f| f["fields"].as_array().unwrap().len())
        .sum();
    assert!(total > 0 && total <= 200, "{total}");
    assert!(build_report(&input(vec![])).unwrap().len() < 32 * 1024);
    assert!(r.to_string().len() <= 32 * 1024);
}

#[test]
fn only_web_pages_can_be_reported() {
    for url in ["chrome://settings", "file:///x.html", "not a url"] {
        let mut bad = input(vec![]);
        bad.page_url = url.into();
        assert_eq!(build_report(&bad), Err(ReportError::InvalidUrl), "{url}");
    }
    let mut local = input(vec![]);
    local.page_url = "http://localhost:8080/login".into();
    assert_eq!(report(local)["site"], "http://localhost:8080");
}
