//! A minimal PDF 1.4 writer for one page: text in the standard Helvetica and
//! Courier fonts (WinAnsi encoding, nothing embedded), lines and filled
//! rectangles. Uncompressed and without dates, so the output is deterministic.

use pwm_crypto::Secret;
use zeroize::Zeroizing;

/// Enough for the largest page (a QR code of the longest inputs), so the
/// buffers that hold the Secret Key never reallocate and leave copies behind.
const CAPACITY: usize = 512 * 1024;

#[derive(Clone, Copy)]
pub(crate) enum Font {
    Helvetica,
    HelveticaBold,
    Courier,
    CourierBold,
}

impl Font {
    const ALL: [Font; 4] = [
        Font::Helvetica,
        Font::HelveticaBold,
        Font::Courier,
        Font::CourierBold,
    ];

    fn resource(self) -> &'static str {
        match self {
            Font::Helvetica => "F1",
            Font::HelveticaBold => "F2",
            Font::Courier => "F3",
            Font::CourierBold => "F4",
        }
    }

    fn base_font(self) -> &'static str {
        match self {
            Font::Helvetica => "Helvetica",
            Font::HelveticaBold => "Helvetica-Bold",
            Font::Courier => "Courier",
            Font::CourierBold => "Courier-Bold",
        }
    }
}

/// A page's content stream, in PDF coordinates (points, origin bottom left).
pub(crate) struct Content(Zeroizing<Vec<u8>>);

impl Content {
    pub fn new() -> Self {
        Self(Zeroizing::new(Vec::with_capacity(CAPACITY)))
    }

    fn put(&mut self, s: &str) {
        self.0.extend_from_slice(s.as_bytes());
    }

    fn num(&mut self, v: f32) {
        let s = format!("{v:.2}");
        let s = s.trim_end_matches('0').trim_end_matches('.');
        self.put(if s == "-0" { "0" } else { s });
    }

    pub fn text(&mut self, font: Font, size: f32, x: f32, y: f32, text: &str) {
        self.put("BT /");
        self.put(font.resource());
        self.put(" ");
        self.num(size);
        self.put(" Tf ");
        self.num(x);
        self.put(" ");
        self.num(y);
        self.put(" Td (");
        for c in text.chars() {
            match c {
                '(' | ')' | '\\' => {
                    self.0.push(b'\\');
                    self.0.push(c as u8);
                }
                ' '..='~' => self.0.push(c as u8),
                // Latin-1 is the same in WinAnsi; written as octal to keep the file ASCII.
                '\u{a0}'..='\u{ff}' => self.put(&format!("\\{:03o}", c as u32)),
                _ => self.0.push(b'?'),
            }
        }
        self.put(") Tj ET\n");
    }

    /// The fill color for what follows, from 0 (black) to 1 (white).
    pub fn gray(&mut self, level: f32) {
        self.num(level);
        self.put(" g\n");
    }

    pub fn line(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, width: f32) {
        self.num(width);
        self.put(" w ");
        self.num(x1);
        self.put(" ");
        self.num(y1);
        self.put(" m ");
        self.num(x2);
        self.put(" ");
        self.num(y2);
        self.put(" l S\n");
    }

    pub fn rect(&mut self, x: f32, y: f32, w: f32, h: f32) {
        for v in [x, y, w, h] {
            self.num(v);
            self.put(" ");
        }
        self.put("re\n");
    }

    /// Fills the rectangles added since the last fill.
    pub fn fill(&mut self) {
        self.put("f\n");
    }
}

/// The whole file for one page of `size` points.
pub(crate) fn document(size: (u32, u32), content: &Content) -> Secret<Vec<u8>> {
    let mut out = Zeroizing::new(Vec::with_capacity(CAPACITY + 4096));
    let mut offsets = Vec::new();
    // The comment of four high bytes marks the file as binary for transfer tools.
    out.extend_from_slice(b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n");

    let fonts: String = Font::ALL
        .iter()
        .enumerate()
        .map(|(i, f)| format!("/{} {} 0 R ", f.resource(), 4 + i))
        .collect();
    let mut objects: Vec<Vec<u8>> = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {} {}] /Resources << /Font << {fonts}>> >> /Contents 8 0 R >>",
            size.0, size.1
        )
        .into_bytes(),
    ];
    for f in Font::ALL {
        objects.push(
            format!(
                "<< /Type /Font /Subtype /Type1 /BaseFont /{} /Encoding /WinAnsiEncoding >>",
                f.base_font()
            )
            .into_bytes(),
        );
    }
    let info = format!(
        "<< /Title (Emergency Kit) /Producer ({}) >>",
        crate::PRODUCT_NAME
    )
    .into_bytes();

    for (i, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    // Object 8, the content stream, is written straight from its buffer.
    offsets.push(out.len());
    out.extend_from_slice(
        format!("8 0 obj\n<< /Length {} >>\nstream\n", content.0.len()).as_bytes(),
    );
    out.extend_from_slice(&content.0);
    out.extend_from_slice(b"\nendstream\nendobj\n");
    offsets.push(out.len());
    out.extend_from_slice(b"9 0 obj\n");
    out.extend_from_slice(&info);
    out.extend_from_slice(b"\nendobj\n");

    let xref = out.len();
    out.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len() + 1).as_bytes(),
    );
    for offset in &offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R /Info 9 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            offsets.len() + 1
        )
        .as_bytes(),
    );
    Secret::new(std::mem::take(&mut *out))
}
