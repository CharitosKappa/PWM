//! Field classification (D33 §6) on described forms: login, sign-up,
//! password change, multi-step, one-time codes, cards, addresses, honeypots,
//! and site rules.

use pwm_autofill::{Class, Field, FieldRule, Rules, SiteRules, classify};

/// A field from `key=value` pairs, visible by default.
fn field(spec: &str) -> Field {
    let mut f = Field {
        visible: true,
        ..Field::default()
    };
    for pair in spec.split(';').map(str::trim).filter(|p| !p.is_empty()) {
        let (k, v) = pair.split_once('=').unwrap();
        let v = v.to_owned();
        match k {
            "type" => f.r#type = v,
            "name" => f.name = v,
            "id" => f.id = v,
            "autocomplete" => f.autocomplete = v,
            "label" => f.label = v,
            "placeholder" => f.placeholder = v,
            "aria" => f.aria_label = v,
            "inputmode" => f.inputmode = v,
            "maxlength" => f.max_length = Some(v.parse().unwrap()),
            "visible" => f.visible = v == "true",
            "form" => f.form = Some(v.parse().unwrap()),
            other => panic!("unknown key {other}"),
        }
    }
    f
}

fn classes(fields: &[&str]) -> Vec<Class> {
    let fields: Vec<Field> = fields.iter().map(|s| field(s)).collect();
    classify(&fields, None)
        .into_iter()
        .map(|c| c.class)
        .collect()
}

#[test]
fn autocomplete_tokens_decide() {
    let fields: Vec<Field> = [
        "type=text;autocomplete=section-a shipping username",
        "type=password;autocomplete=current-password",
        "type=text;autocomplete=one-time-code",
        "type=text;autocomplete=cc-number",
    ]
    .iter()
    .map(|s| field(s))
    .collect();
    let got = classify(&fields, None);
    assert_eq!(
        got.iter().map(|c| c.class).collect::<Vec<_>>(),
        [
            Class::Username,
            Class::CurrentPassword,
            Class::Totp,
            Class::CcNumber
        ]
    );
    assert!(got.iter().all(|c| c.confidence >= 0.99));
}

#[test]
fn a_login_form() {
    assert_eq!(
        classes(&[
            "type=text;name=login;label=Email or username",
            "type=password;name=pass"
        ]),
        [Class::Username, Class::CurrentPassword]
    );
    assert_eq!(
        classes(&["type=email;id=email", "type=password;id=pw"]),
        [Class::Email, Class::CurrentPassword]
    );
    // An unlabelled text field right before the password is the username.
    assert_eq!(
        classes(&["type=text;name=f1", "type=password;name=f2"]),
        [Class::Username, Class::CurrentPassword]
    );
    // Other languages.
    assert_eq!(
        classes(&[
            "type=text;label=Όνομα χρήστη",
            "type=password;label=Κωδικός πρόσβασης"
        ]),
        [Class::Username, Class::CurrentPassword]
    );
    assert_eq!(
        classes(&[
            "type=text;placeholder=Benutzername",
            "type=password;placeholder=Passwort"
        ]),
        [Class::Username, Class::CurrentPassword]
    );
    assert_eq!(
        classes(&[
            "type=text;label=Nom d'utilisateur",
            "type=password;label=Mot de passe"
        ]),
        [Class::Username, Class::CurrentPassword]
    );
}

#[test]
fn an_email_address_is_not_a_street_address() {
    // D65 §4: DigitalOcean's and ok.ru's login fields, as the pages describe them.
    assert_eq!(
        classes(&[
            "type=text;id=email;name=email;autocomplete=none;label=Email*;placeholder=Enter your email address",
            "type=password;id=password;name=password;autocomplete=none;label=Password*;placeholder=Enter your password",
        ]),
        [Class::Email, Class::CurrentPassword]
    );
    assert_eq!(
        classes(&[
            "type=text;id=field_email;name=st.email;placeholder=Phone number or email address",
            "type=password;id=field_password;name=st.password;placeholder=Password",
        ]),
        [Class::Email, Class::CurrentPassword]
    );
    assert_eq!(classes(&["type=text;label=E-mail address"]), [Class::Email]);
    assert_eq!(
        classes(&["type=text;placeholder=Phone number"]),
        [Class::Tel]
    );
    assert_eq!(classes(&["type=text;label=Mobile number"]), [Class::Tel]);
    // A real address is still one.
    assert_eq!(
        classes(&["type=text;label=Street address"]),
        [Class::AddressLine1]
    );
}

#[test]
fn sign_up_and_change_password_forms() {
    assert_eq!(
        classes(&[
            "type=email;name=email",
            "type=password;name=password",
            "type=password;name=password_confirm"
        ]),
        [Class::Email, Class::NewPassword, Class::NewPassword]
    );
    assert_eq!(
        classes(&[
            "type=password;name=current",
            "type=password;name=new",
            "type=password;name=confirm"
        ]),
        [
            Class::CurrentPassword,
            Class::NewPassword,
            Class::NewPassword
        ]
    );
    assert_eq!(
        classes(&["type=password;label=New password"]),
        [Class::NewPassword]
    );
    // Forms are classified separately: a search box elsewhere does not count.
    assert_eq!(
        classes(&[
            "type=text;name=q;label=Search;form=0",
            "type=text;name=user;form=1",
            "type=password;name=pw;form=1"
        ]),
        [Class::Ignore, Class::Username, Class::CurrentPassword]
    );
}

#[test]
fn multi_step_pages() {
    // Step one: only the username.
    assert_eq!(
        classes(&["type=email;name=identifier;label=Email or phone"]),
        [Class::Email]
    );
    assert_eq!(classes(&["type=text;name=username"]), [Class::Username]);
    // Step two: only the password.
    assert_eq!(
        classes(&["type=password;name=Passwd"]),
        [Class::CurrentPassword]
    );
}

#[test]
fn one_time_codes_need_high_confidence() {
    let fields = [
        field("type=text;inputmode=numeric;maxlength=6;label=Verification code"),
        field("type=text;name=code"),
        field("type=text;autocomplete=one-time-code"),
    ];
    let got = classify(&fields, None);
    assert_eq!(got[0].class, Class::Totp);
    assert!(got[0].confidence >= 0.8, "{}", got[0].confidence);
    // A bare "code" might be anything: low confidence, so it is copied, not filled.
    assert_eq!(got[1].class, Class::Totp);
    assert!(got[1].confidence < 0.8, "{}", got[1].confidence);
    assert_eq!(got[2].confidence, 1.0);
}

#[test]
fn cards_and_addresses() {
    assert_eq!(
        classes(&[
            "type=text;label=Name on card",
            "type=text;inputmode=numeric;label=Card number",
            "type=text;placeholder=MM / YY;label=Expiry",
            "type=text;label=CVC;maxlength=4",
        ]),
        [Class::CcName, Class::CcNumber, Class::CcExp, Class::CcCsc]
    );
    assert_eq!(
        classes(&[
            "type=text;name=street;label=Street address",
            "type=text;label=City",
            "type=text;label=ZIP code",
            "type=tel;label=Phone"
        ]),
        [
            Class::AddressLine1,
            Class::AddressCity,
            Class::AddressPostalCode,
            Class::Tel
        ]
    );
}

#[test]
fn hidden_buttons_and_honeypots_are_ignored() {
    assert_eq!(
        classes(&[
            "type=hidden;name=csrf",
            "type=text;name=username;visible=false",
            "type=submit;name=go",
            "type=checkbox;name=remember",
            "type=text;name=user",
            "type=password;name=pass",
        ]),
        [
            Class::Ignore,
            Class::Ignore,
            Class::Ignore,
            Class::Ignore,
            Class::Username,
            Class::CurrentPassword
        ]
    );
}

#[test]
fn site_rules_override_the_heuristics() {
    let site = SiteRules {
        domain: "example.com".to_owned(),
        fields: vec![
            FieldRule::by_id_equals("acct", Class::Username),
            FieldRule::by_name_contains("pin", Class::Ignore),
        ],
        ..SiteRules::default()
    };
    let fields = [
        field("type=text;id=acct;label=Account"),
        field("type=password;name=user_pin"),
        field("type=password;name=pw"),
    ];
    let got: Vec<Class> = classify(&fields, Some(&site))
        .into_iter()
        .map(|c| c.class)
        .collect();
    assert_eq!(
        got,
        [Class::Username, Class::Ignore, Class::CurrentPassword]
    );
    let _ = Rules::empty();
}

#[test]
fn class_names_follow_autocomplete_tokens() {
    assert_eq!(Class::CurrentPassword.as_str(), "current-password");
    assert_eq!(Class::Totp.as_str(), "one-time-code");
    assert_eq!(Class::from_token("cc-exp-month"), Some(Class::CcExpMonth));
    assert_eq!(Class::from_token("nonsense"), None);
}
