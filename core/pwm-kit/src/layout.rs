//! Where everything goes on the page. Values that vary (email, Secret Key,
//! server, account id) are in Courier, whose glyphs are all 0.6 em wide, so
//! their width is known without font metrics and long values wrap exactly.

use crate::pdf::{Content, Font};
use crate::qr::Modules;
use crate::{Kit, PRODUCT_NAME, Paper};

const MARGIN: f32 = 56.0;
const QR_SIZE: f32 = 144.0;
const QR_QUIET: usize = 4;
const FOOTER: [&str; 3] = [
    "You need your email, master password and Secret Key to sign in on a new device.",
    "Keep this page somewhere safe and offline, such as with your important papers.",
    "Nobody, including us, can recover your master password or Secret Key for you.",
];
const FOOTER_LEADING: f32 = 14.0;

/// Draws onto a page, measuring from the top edge.
struct Page {
    content: Content,
    height: f32,
    width: f32,
    /// Distance of the next baseline from the top edge.
    y: f32,
}

impl Page {
    fn text(&mut self, font: Font, size: f32, x: f32, text: &str) {
        let y = self.height - self.y;
        self.content.text(font, size, x, y, text);
    }

    /// A label, then a monospaced value wrapped to the page width.
    fn field(&mut self, label: &str, font: Font, size: f32, value: &str) {
        self.content.gray(0.35);
        self.text(Font::HelveticaBold, 9.0, MARGIN, label);
        self.content.gray(0.0);
        let columns = ((self.width - 2.0 * MARGIN) / (0.6 * size)) as usize;
        let chars: Vec<char> = value.chars().collect();
        for line in chars.chunks(columns.max(1)) {
            self.y += size + 4.0;
            let line: String = line.iter().collect();
            self.text(font, size, MARGIN, &line);
        }
        self.y += 26.0;
    }
}

pub(crate) fn page(kit: &Kit<'_>, paper: Paper, qr: &Modules) -> Content {
    let (width, height) = paper.size();
    let mut page = Page {
        content: Content::new(),
        height: height as f32,
        width: width as f32,
        y: MARGIN + 18.0,
    };

    page.text(Font::HelveticaBold, 24.0, MARGIN, "Emergency Kit");
    page.y += 20.0;
    page.content.gray(0.35);
    page.text(
        Font::Helvetica,
        10.0,
        MARGIN,
        &format!("{PRODUCT_NAME} \u{b7} created {}", kit.created),
    );
    page.content.gray(0.0);
    page.y += 40.0;

    page.field("Email", Font::Courier, 12.0, kit.email);
    page.field(
        "Secret Key",
        Font::CourierBold,
        16.0,
        kit.secret_key.to_text().expose_secret(),
    );

    page.content.gray(0.35);
    page.text(Font::HelveticaBold, 9.0, MARGIN, "Master password");
    page.y += 28.0;
    let line_y = page.height - page.y;
    page.content
        .line(MARGIN, line_y, MARGIN + 320.0, line_y, 0.75);
    page.y += 12.0;
    page.text(
        Font::Helvetica,
        8.0,
        MARGIN,
        "Write it here by hand, only if you keep this page offline.",
    );
    page.content.gray(0.0);
    page.y += 30.0;

    if let Some(server) = kit.server {
        page.field("Server", Font::Courier, 12.0, server);
    }
    page.field("Account id", Font::Courier, 9.0, kit.account_id);

    // The QR code, quiet zone included, with its caption beside it.
    let top = page.y - 8.0;
    draw_qr(&mut page, qr, MARGIN, top);
    page.content.gray(0.35);
    let caption = [
        "Scan this in the app when you sign in on a new",
        "device. It fills in your email and Secret Key,",
        "never your master password.",
    ];
    for (i, line) in caption.iter().enumerate() {
        page.y = top + 36.0 + 13.0 * i as f32;
        page.text(Font::Helvetica, 9.0, MARGIN + QR_SIZE + 16.0, line);
    }
    page.content.gray(0.0);
    let bottom = top + QR_SIZE;

    let footer_top = page.height - MARGIN - FOOTER_LEADING * (FOOTER.len() as f32 - 1.0) - 10.0;
    debug_assert!(bottom <= footer_top, "the kit overflows into its footer");
    for (i, line) in FOOTER.iter().enumerate() {
        page.y = page.height - MARGIN - FOOTER_LEADING * (FOOTER.len() - 1 - i) as f32;
        page.text(Font::Helvetica, 10.0, MARGIN, line);
    }
    page.content
}

/// Dark modules as filled squares, merged into runs along each row.
fn draw_qr(page: &mut Page, qr: &Modules, x: f32, top: f32) {
    let module = QR_SIZE / (qr.width + 2 * QR_QUIET) as f32;
    let origin_x = x + module * QR_QUIET as f32;
    for (row, cells) in qr.dark.chunks(qr.width).enumerate() {
        let y = page.height - top - module * (QR_QUIET + row + 1) as f32;
        let mut col = 0;
        while col < cells.len() {
            if !cells[col] {
                col += 1;
                continue;
            }
            let start = col;
            while col < cells.len() && cells[col] {
                col += 1;
            }
            page.content.rect(
                origin_x + module * start as f32,
                y,
                module * (col - start) as f32,
                module,
            );
        }
    }
    page.content.fill();
}

#[cfg(test)]
mod tests {
    use pwm_crypto::{Secret, SecretKey};

    use super::*;
    use crate::qr;

    /// The longest inputs still end above the footer on the shorter page.
    #[test]
    fn the_longest_kit_fits_on_letter() {
        let key = SecretKey::from_bytes(Secret::new([7; 16]));
        let email = format!("{}@example.com", "a".repeat(242));
        let server = format!("https://{}.example.org", "b".repeat(180));
        let kit = Kit {
            email: &email,
            secret_key: &key,
            server: Some(&server),
            account_id: "0f8e2f4c-3a1b-4d5e-9f60-7a8b9c0d1e2f",
            created: "2026-11-02",
        };
        let payload = kit.qr_payload().unwrap();
        let modules = qr::modules(payload.expose_secret()).unwrap();
        // debug_assert in page() checks the footer.
        page(&kit, Paper::Letter, &modules);
    }
}
