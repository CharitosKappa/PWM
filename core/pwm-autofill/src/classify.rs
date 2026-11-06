//! Field classification (D33 §6): rules and weighted keywords over a
//! platform's description of the fields. Never values.

use std::sync::LazyLock;

use serde::Deserialize;
use unicode_normalization::UnicodeNormalization;

use crate::SiteRules;

/// What a field is. The names are HTML `autocomplete` tokens.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Class {
    Username,
    Email,
    CurrentPassword,
    NewPassword,
    Totp,
    CcName,
    CcNumber,
    CcExp,
    CcExpMonth,
    CcExpYear,
    CcCsc,
    Name,
    GivenName,
    FamilyName,
    Organization,
    AddressLine1,
    AddressLine2,
    AddressCity,
    AddressRegion,
    AddressPostalCode,
    AddressCountry,
    Tel,
    Ignore,
}

impl Class {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Username => "username",
            Self::Email => "email",
            Self::CurrentPassword => "current-password",
            Self::NewPassword => "new-password",
            Self::Totp => "one-time-code",
            Self::CcName => "cc-name",
            Self::CcNumber => "cc-number",
            Self::CcExp => "cc-exp",
            Self::CcExpMonth => "cc-exp-month",
            Self::CcExpYear => "cc-exp-year",
            Self::CcCsc => "cc-csc",
            Self::Name => "name",
            Self::GivenName => "given-name",
            Self::FamilyName => "family-name",
            Self::Organization => "organization",
            Self::AddressLine1 => "address-line1",
            Self::AddressLine2 => "address-line2",
            Self::AddressCity => "address-level2",
            Self::AddressRegion => "address-level1",
            Self::AddressPostalCode => "postal-code",
            Self::AddressCountry => "country",
            Self::Tel => "tel",
            Self::Ignore => "ignore",
        }
    }

    /// A class from an `autocomplete` token (or our `ignore`); `None` for
    /// tokens that say nothing about the field (`on`, `off`, sections).
    pub fn from_token(token: &str) -> Option<Self> {
        Some(match token.to_ascii_lowercase().as_str() {
            "username" => Self::Username,
            "email" => Self::Email,
            "current-password" => Self::CurrentPassword,
            "new-password" => Self::NewPassword,
            "one-time-code" => Self::Totp,
            "cc-name" => Self::CcName,
            "cc-number" => Self::CcNumber,
            "cc-exp" => Self::CcExp,
            "cc-exp-month" => Self::CcExpMonth,
            "cc-exp-year" => Self::CcExpYear,
            "cc-csc" => Self::CcCsc,
            "name" => Self::Name,
            "given-name" => Self::GivenName,
            "family-name" => Self::FamilyName,
            "organization" => Self::Organization,
            "address-line1" | "street-address" => Self::AddressLine1,
            "address-line2" => Self::AddressLine2,
            "address-level2" => Self::AddressCity,
            "address-level1" => Self::AddressRegion,
            "postal-code" => Self::AddressPostalCode,
            "country" | "country-name" => Self::AddressCountry,
            "tel" => Self::Tel,
            "ignore" => Self::Ignore,
            _ => return None,
        })
    }
}

/// A field as a platform describes it. Never its value.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Field {
    pub tag: String,
    pub r#type: String,
    pub name: String,
    pub id: String,
    pub autocomplete: String,
    pub label: String,
    pub placeholder: String,
    pub aria_label: String,
    pub inputmode: String,
    pub max_length: Option<u32>,
    pub visible: bool,
    /// Which form the field belongs to; fields are classified per form.
    pub form: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FieldClass {
    pub class: Class,
    /// 0 to 1. One-time codes are filled only at 0.8 or more.
    pub confidence: f32,
}

/// Keywords per class, normalized (lower case, no accents, letters and
/// digits only). The longest keyword found wins. English, Greek, German,
/// French, Spanish, Italian and Portuguese.
const KEYWORDS: &[(Class, &[&str])] = &[
    (
        Class::Username,
        &[
            "username",
            "user",
            "login",
            "userid",
            "loginid",
            "accountname",
            "benutzername",
            "benutzer",
            "utilisateur",
            "identifiant",
            "usuario",
            "utente",
            "nomeutente",
            "όνομα χρήστη",
            "χρήστης",
            "χρήστη",
        ],
    ),
    (
        Class::Email,
        &[
            "email",
            "mail",
            // Longer than "address", which would win otherwise (D65 §4).
            "emailaddress",
            "mailaddress",
            "correo",
            "courriel",
            "emailadresse",
            "postaelettronica",
            "ηλεκτρονικοταχυδρομειο",
            "διευθυνσηηλεκτρονικουταχυδρομειου",
        ],
    ),
    (
        Class::Totp,
        &[
            "otp",
            "onetime",
            "onetimecode",
            "verificationcode",
            "authcode",
            "authenticationcode",
            "twofactor",
            "2fa",
            "mfa",
            "authenticator",
            "token",
            "code",
            "bestatigungscode",
            "sicherheitscode2fa",
            "codedeverification",
            "codigodeverificacion",
            "codicediverifica",
            "codigodeverificacao",
            "κωδικός επαλήθευσης",
            "κωδικός μίας χρήσης",
        ],
    ),
    (
        Class::CcNumber,
        &[
            "cardnumber",
            "ccnumber",
            "ccnum",
            "cardno",
            "creditcard",
            "creditcardnumber",
            "kartennummer",
            "numerodecarte",
            "numerodetarjeta",
            "numerodellacarta",
            "numerodocartao",
            "αριθμός κάρτας",
        ],
    ),
    (
        Class::CcName,
        &[
            "nameoncard",
            "cardholder",
            "cardholdername",
            "ccname",
            "cardname",
            "karteninhaber",
            "titulaire",
            "titular",
            "titolare",
            "κάτοχος κάρτας",
            "κάτοχος",
        ],
    ),
    (
        Class::CcExp,
        &[
            "expiry",
            "expiration",
            "expirationdate",
            "expdate",
            "mmyy",
            "validthru",
            "ablaufdatum",
            "gultigbis",
            "dateexpiration",
            "vencimiento",
            "scadenza",
            "validade",
            "ληξη",
        ],
    ),
    (
        Class::CcExpMonth,
        &["expmonth", "expirymonth", "expirationmonth", "ccmonth"],
    ),
    (
        Class::CcExpYear,
        &["expyear", "expiryyear", "expirationyear", "ccyear"],
    ),
    (
        Class::CcCsc,
        &[
            "cvc",
            "cvv",
            "csc",
            "cvn",
            "cvv2",
            "securitycode",
            "cardcode",
            "cardverification",
            "prufnummer",
            "cryptogramme",
            "codigodeseguridad",
            "codicedisicurezza",
        ],
    ),
    (
        Class::Name,
        &[
            "fullname",
            "yourname",
            "name",
            "nom",
            "nome",
            "nombre",
            "ονοματεπωνυμο",
            "ονομα",
        ],
    ),
    (
        Class::GivenName,
        &[
            "firstname",
            "givenname",
            "fname",
            "vorname",
            "prenom",
            "primernombre",
            "ονομα1",
        ],
    ),
    (
        Class::FamilyName,
        &[
            "lastname",
            "surname",
            "familyname",
            "lname",
            "nachname",
            "nomdefamille",
            "apellido",
            "apellidos",
            "cognome",
            "sobrenome",
            "επωνυμο",
        ],
    ),
    (
        Class::Organization,
        &[
            "company",
            "organization",
            "organisation",
            "firma",
            "entreprise",
            "empresa",
            "azienda",
            "εταιρεια",
        ],
    ),
    (
        Class::AddressLine1,
        &[
            "address",
            "streetaddress",
            "street",
            "address1",
            "addressline1",
            "addr1",
            "strasse",
            "adresse",
            "direccion",
            "indirizzo",
            "endereco",
            "διευθυνση",
            "οδος",
        ],
    ),
    (
        Class::AddressLine2,
        &[
            "address2",
            "addressline2",
            "addr2",
            "apartment",
            "suite",
            "adresszusatz",
            "complemento",
        ],
    ),
    (
        Class::AddressCity,
        &[
            "city", "town", "stadt", "ville", "ciudad", "citta", "cidade", "πολη",
        ],
    ),
    (
        Class::AddressRegion,
        &[
            "state",
            "province",
            "region",
            "county",
            "bundesland",
            "provincia",
            "estado",
            "νομος",
            "περιοχη",
        ],
    ),
    (
        Class::AddressPostalCode,
        &[
            "zip",
            "zipcode",
            "postalcode",
            "postcode",
            "postleitzahl",
            "codepostal",
            "codigopostal",
            "codicepostale",
            "ταχυδρομικοσκωδικας",
        ],
    ),
    (
        Class::AddressCountry,
        &["country", "pays", "pais", "paese", "χωρα"],
    ),
    (
        Class::Tel,
        &[
            "phone",
            "tel",
            "mobile",
            "phonenumber",
            "mobilenumber",
            "telephone",
            "telefon",
            "telefono",
            "telefone",
            "τηλεφωνο",
            "κινητο",
        ],
    ),
    (
        Class::Ignore,
        &[
            "search",
            "query",
            "suche",
            "recherche",
            "buscar",
            "cerca",
            "pesquisa",
            "αναζητηση",
            "captcha",
            "honeypot",
            "coupon",
            "promocode",
            "gutschein",
        ],
    ),
];

const NEW_PASSWORD: &[&str] = &[
    "new",
    "confirm",
    "repeat",
    "retype",
    "again",
    "verify",
    "neu",
    "nouveau",
    "nuevo",
    "nuovo",
    "novo",
    "bestatigen",
    "wiederholen",
    "confirmer",
    "confirmar",
    "conferma",
    "νέος",
    "νέο",
    "επιβεβαίωση",
    "επανάληψη",
];
const CURRENT_PASSWORD: &[&str] = &[
    "current",
    "old",
    "existing",
    "aktuell",
    "alt",
    "actuel",
    "ancien",
    "actual",
    "attuale",
    "atual",
    "τρέχων",
    "παλιός",
];
const STRONG_TOTP: &[&str] = &[
    "otp",
    "onetime",
    "verificationcode",
    "authenticationcode",
    "twofactor",
    "2fa",
    "mfa",
    "authenticator",
    "bestatigungscode",
    "codedeverification",
    "codigodeverificacion",
    "codicediverifica",
    "codigodeverificacao",
    "κωδικός επαλήθευσης",
    "κωδικός μίας χρήσης",
];
const IGNORED_TYPES: &[&str] = &[
    "hidden", "submit", "button", "reset", "checkbox", "radio", "file", "image", "color", "range",
];

/// Lower case, accents removed, Greek final sigma folded, letters and
/// digits only. Keywords go through the same, so they can be written
/// naturally.
fn normalize(text: &str) -> String {
    text.nfd()
        .filter(|c| !unicode_normalization::char::is_combining_mark(*c))
        .flat_map(char::to_lowercase)
        .map(|c| if c == 'ς' { 'σ' } else { c })
        .filter(|c| c.is_alphanumeric())
        .collect()
}

fn normalized(words: &[&str]) -> Vec<String> {
    words.iter().map(|w| normalize(w)).collect()
}

static KEYWORDS_N: LazyLock<Vec<(Class, Vec<String>)>> =
    LazyLock::new(|| KEYWORDS.iter().map(|(c, w)| (*c, normalized(w))).collect());
static NEW_PASSWORD_N: LazyLock<Vec<String>> = LazyLock::new(|| normalized(NEW_PASSWORD));
static CURRENT_PASSWORD_N: LazyLock<Vec<String>> = LazyLock::new(|| normalized(CURRENT_PASSWORD));
static STRONG_TOTP_N: LazyLock<Vec<String>> = LazyLock::new(|| normalized(STRONG_TOTP));

fn text_of(f: &Field) -> String {
    [&f.name, &f.id, &f.label, &f.placeholder, &f.aria_label]
        .iter()
        .map(|s| normalize(s))
        .collect::<Vec<_>>()
        .join(" ")
}

fn has_any(text: &str, words: &[String]) -> bool {
    words.iter().any(|w| text.contains(w.as_str()))
}

/// The class whose longest keyword appears in `text` (the first listed on
/// a tie).
fn keyword_class(text: &str) -> Option<Class> {
    let mut best: Option<(usize, Class)> = None;
    for (class, words) in KEYWORDS_N.iter() {
        let len = words
            .iter()
            .filter(|w| text.contains(w.as_str()))
            .map(|w| w.chars().count())
            .max();
        if let Some(len) = len
            && best.is_none_or(|(b, _)| len > b)
        {
            best = Some((len, *class));
        }
    }
    best.map(|(_, class)| class)
}

fn input_type(f: &Field) -> String {
    let t = f.r#type.to_ascii_lowercase();
    if t.is_empty() { "text".to_owned() } else { t }
}

/// The class from `autocomplete`, which decides on its own when valid.
fn autocomplete_class(f: &Field) -> Option<Class> {
    f.autocomplete
        .split_whitespace()
        .rev()
        .filter(|t| !t.eq_ignore_ascii_case("webauthn"))
        .find_map(Class::from_token)
}

fn totp_confidence(f: &Field, text: &str) -> f32 {
    let base = if has_any(text, &STRONG_TOTP_N) {
        0.7
    } else {
        0.6
    };
    let numeric = f.inputmode.eq_ignore_ascii_case("numeric")
        || matches!(input_type(f).as_str(), "number" | "tel");
    let short = f.max_length.is_some_and(|n| (4..=8).contains(&n));
    if numeric && short { base + 0.25 } else { base }
}

/// Classifies every field; `site` holds the signed per-site overrides.
pub fn classify(fields: &[Field], site: Option<&SiteRules>) -> Vec<FieldClass> {
    let mut out: Vec<Option<FieldClass>> = vec![None; fields.len()];
    let decided = |class, confidence| Some(FieldClass { class, confidence });
    for (i, f) in fields.iter().enumerate() {
        out[i] = if let Some(class) =
            site.and_then(|s| s.fields.iter().find(|r| r.matches(f)).map(|r| r.class))
        {
            decided(class, 1.0)
        } else if !f.visible
            || IGNORED_TYPES.contains(&input_type(f).as_str())
            || f.tag.eq_ignore_ascii_case("button")
        {
            decided(Class::Ignore, 1.0)
        } else {
            autocomplete_class(f).and_then(|c| decided(c, 1.0))
        };
    }

    // The rest, form by form.
    let mut forms: Vec<Option<u32>> = fields.iter().map(|f| f.form).collect();
    forms.sort_unstable();
    forms.dedup();
    for form in forms {
        let members: Vec<usize> = (0..fields.len())
            .filter(|&i| fields[i].form == form)
            .collect();
        let passwords: Vec<usize> = members
            .iter()
            .copied()
            .filter(|&i| out[i].is_none() && input_type(&fields[i]) == "password")
            .collect();
        for (n, &i) in passwords.iter().enumerate() {
            let text = text_of(&fields[i]);
            let structural = match passwords.len() {
                1 => Class::CurrentPassword,
                2 => Class::NewPassword,
                _ if n == 0 => Class::CurrentPassword,
                _ => Class::NewPassword,
            };
            out[i] = if has_any(&text, &CURRENT_PASSWORD_N) {
                decided(Class::CurrentPassword, 0.9)
            } else if has_any(&text, &NEW_PASSWORD_N) {
                decided(Class::NewPassword, 0.9)
            } else {
                decided(structural, 0.8)
            };
        }
        let has_password = members
            .iter()
            .any(|&i| input_type(&fields[i]) == "password");
        for &i in &members {
            if out[i].is_some() {
                continue;
            }
            let f = &fields[i];
            let text = text_of(f);
            out[i] = match (input_type(f).as_str(), keyword_class(&text)) {
                (_, Some(Class::Totp)) => decided(Class::Totp, totp_confidence(f, &text)),
                ("email", _) => decided(Class::Email, 0.9),
                (_, Some(class)) => decided(class, 0.75),
                ("tel", None) => decided(Class::Tel, 0.6),
                _ => None,
            };
        }
        // In a login form, the unclaimed field just before the first
        // password is the username.
        if has_password
            && let Some(&first_pw) = members
                .iter()
                .find(|&&i| input_type(&fields[i]) == "password")
            && let Some(&before) = members.iter().rev().find(|&&i| {
                i < first_pw
                    && out[i].is_none()
                    && matches!(input_type(&fields[i]).as_str(), "text" | "email" | "tel")
            })
        {
            out[before] = decided(Class::Username, 0.6);
        }
    }
    out.into_iter()
        .map(|c| {
            c.unwrap_or(FieldClass {
                class: Class::Ignore,
                confidence: 0.3,
            })
        })
        .collect()
}
