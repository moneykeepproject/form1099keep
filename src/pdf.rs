//! Recipient copies as PDF (form1099keep.md "Recipient copies"), following Pub 1179
//! Part 4 for substitute statements: year, form number and title together at the
//! upper right; box numbers and captions as on the official form; the OMB number;
//! the Copy B legend; the payer's telephone number; instructions for the recipient.
//! Black text only — Copy A (red dropout ink) is never produced.
//!
//! The PDF writer is small and self-contained: the standard Helvetica fonts with
//! WinAnsi encoding, uncompressed content streams.

// Page geometry is in points (f32). Money never passes through a float here: amounts
// are formatted from `Decimal` before they reach the page.
#![allow(clippy::float_arithmetic, clippy::cast_precision_loss)]

use mk_core::Decimal;

use crate::boxes::{self, BoxDef, BoxKind};
use crate::check::money;
use crate::model::{Form, FormKind, Payer, Recipient};
use crate::tin;

// ----- copies ---------------------------------------------------------------------------

/// Which copy to print.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Copy {
    B,
    One,
    Two,
    C,
}

impl Copy {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_uppercase().as_str() {
            "B" => Some(Self::B),
            "1" => Some(Self::One),
            "2" => Some(Self::Two),
            "C" => Some(Self::C),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Copy::B => "Copy B",
            Copy::One => "Copy 1",
            Copy::Two => "Copy 2",
            Copy::C => "Copy C",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Copy::B => "For Recipient",
            Copy::One => "For State Tax Department",
            Copy::Two => "To be filed with recipient's state income tax return, when required.",
            Copy::C => "For Payer",
        }
    }
}

/// What to print.
#[derive(Debug, Clone)]
pub struct Options {
    pub copies: Vec<Copy>,
    /// Show only the last four digits of the recipient's TIN (Pub 1179 §2.2.1).
    pub truncate_recipient_tin: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            copies: vec![Copy::B],
            truncate_recipient_tin: true,
        }
    }
}

/// One form with its parties.
#[derive(Debug, Clone)]
pub struct Item<'a> {
    pub form: &'a Form,
    pub payer: &'a Payer,
    pub recipient: &'a Recipient,
}

/// A PDF with one page per form and copy, in the order given.
pub fn render(items: &[Item<'_>], opts: &Options) -> Vec<u8> {
    let mut doc = Doc::default();
    let title = match items {
        [one] => format!(
            "{} {} — {}",
            one.form.kind.label(),
            one.form.tax_year,
            one.recipient.display_name()
        ),
        _ => format!("Forms 1099 ({} forms)", items.len()),
    };
    for it in items {
        for &c in &opts.copies {
            let mut p = Page::default();
            face(&mut p, it, c, opts);
            instructions(&mut p, it.form.kind, it.form.tax_year, c);
            doc.pages.push(p);
        }
    }
    doc.finish(&title)
}

// ----- the form face ---------------------------------------------------------------------

const LEFT: f32 = 36.0;
const RIGHT: f32 = 576.0;
const MID: f32 = 306.0;
const TOP: f32 = 756.0;
const CAP: f32 = 5.6; // caption size
const VAL: f32 = 9.0; // value size

fn face(p: &mut Page, it: &Item<'_>, copy: Copy, opts: &Options) {
    let f = it.form;
    let kind = f.kind;

    // Upper right: identification block.
    let id_top = TOP;
    let id_h = 92.0;
    p.rect(MID, id_top - id_h, RIGHT - MID, id_h, 0.6);
    let mut y = id_top - 12.0;
    if f.corrected {
        p.text(MID + 6.0, y, 8.0, Font::Bold, "CORRECTED");
    }
    p.text_right(
        RIGHT - 6.0,
        y,
        6.5,
        Font::Regular,
        &format!("OMB No. {}", boxes::omb(kind)),
    );
    y -= 18.0;
    p.text(
        MID + 6.0,
        y,
        15.0,
        Font::Bold,
        &format!("Form {}", kind.label()),
    );
    p.text_right(RIGHT - 6.0, y, 12.0, Font::Bold, &f.tax_year.to_string());
    y -= 11.0;
    p.text(MID + 6.0, y, 6.5, Font::Regular, "(Rev. December 2026)");
    p.text_right(RIGHT - 6.0, y, 6.5, Font::Regular, "For calendar year");
    y -= 14.0;
    p.text(MID + 6.0, y, 10.0, Font::Bold, kind.title());
    y -= 13.0;
    p.text(MID + 6.0, y, 8.5, Font::Bold, copy.name());
    let label_lines = wrap(copy.label(), 7.0, Font::Regular, RIGHT - MID - 70.0);
    let mut ly = y;
    for l in &label_lines {
        p.text(MID + 48.0, ly, 7.0, Font::Regular, l);
        ly -= 8.0;
    }
    p.text(
        MID + 6.0,
        id_top - id_h + 5.0,
        5.6,
        Font::Regular,
        "Department of the Treasury - Internal Revenue Service",
    );

    // Upper left: payer.
    let payer_h = id_h;
    p.rect(LEFT, TOP - payer_h, MID - LEFT, payer_h, 0.6);
    p.caption(
        LEFT + 3.0,
        TOP - 7.0,
        "PAYER'S name, street address, city or town, state or province, country, ZIP",
    );
    p.caption(
        LEFT + 3.0,
        TOP - 13.5,
        "or foreign postal code, and telephone no.",
    );
    let mut lines: Vec<String> = vec![it.payer.name.clone()];
    if !it.payer.name2.trim().is_empty() {
        lines.push(it.payer.name2.trim().to_owned());
    }
    lines.extend(it.payer.address.street_lines());
    lines.push(it.payer.address.city_line());
    lines.push(it.payer.phone.clone());
    let mut y = TOP - 26.0;
    for l in lines.iter().filter(|l| !l.trim().is_empty()).take(6) {
        p.text(
            LEFT + 6.0,
            y,
            VAL,
            Font::Regular,
            &fit(l, VAL, Font::Regular, MID - LEFT - 12.0),
        );
        y -= 11.0;
    }

    // Left column, below the payer.
    let mut y = TOP - payer_h;
    let row = |p: &mut Page, y: &mut f32, h: f32, cells: &[(f32, f32, &str, String)]| {
        for (x0, x1, cap, val) in cells {
            p.rect(*x0, *y - h, x1 - x0, h, 0.6);
            p.caption(x0 + 3.0, *y - 7.0, cap);
            p.text(
                x0 + 6.0,
                *y - h + 6.0,
                VAL,
                Font::Regular,
                &fit(val, VAL, Font::Regular, x1 - x0 - 12.0),
            );
        }
        *y -= h;
    };
    let rtin = if opts.truncate_recipient_tin {
        tin::mask(it.recipient.tin_type, &it.recipient.tin)
    } else {
        tin::display(it.recipient.tin_type, &it.recipient.tin)
    };
    let half = (LEFT + MID) / 2.0;
    row(
        p,
        &mut y,
        26.0,
        &[
            (
                LEFT,
                half,
                "PAYER'S TIN",
                tin::display(Some(it.payer.tin_type), &it.payer.tin),
            ),
            (half, MID, "RECIPIENT'S TIN", rtin),
        ],
    );
    // Recipient name: up to two lines.
    let names = it.recipient.name_lines();
    let h = 34.0;
    p.rect(LEFT, y - h, MID - LEFT, h, 0.6);
    p.caption(LEFT + 3.0, y - 7.0, "RECIPIENT'S name");
    let mut ny = y - 18.0;
    for n in names.iter().take(2) {
        p.text(
            LEFT + 6.0,
            ny,
            VAL,
            Font::Regular,
            &fit(n, VAL, Font::Regular, MID - LEFT - 12.0),
        );
        ny -= 10.0;
    }
    y -= h;
    let a = &it.recipient.address;
    let street = a.street_lines().join(", ");
    row(
        p,
        &mut y,
        26.0,
        &[(LEFT, MID, "Street address (including apt. no.)", street)],
    );
    row(
        p,
        &mut y,
        26.0,
        &[(
            LEFT,
            MID,
            "City or town, state or province, country, and ZIP or foreign postal code",
            a.city_line(),
        )],
    );
    let acct_x = MID - 62.0;
    row(
        p,
        &mut y,
        26.0,
        &[
            (
                LEFT,
                acct_x,
                "Account number (see instructions)",
                f.account_number.clone(),
            ),
            (
                acct_x,
                MID,
                "2nd TIN not.",
                if f.second_tin_notice {
                    "X".into()
                } else {
                    String::new()
                },
            ),
        ],
    );
    let left_end = y;

    // Right column: the boxes, two per row.
    let defs: Vec<&BoxDef> = boxes::layout(kind).iter().collect();
    let col_w = (RIGHT - MID) / 2.0;
    let bh = 28.0;
    let mut y = TOP - id_h;
    for pair in defs.chunks(2) {
        for (i, b) in pair.iter().enumerate() {
            let x0 = MID + col_w * if i == 0 { 0.0 } else { 1.0 };
            draw_box(p, x0, y, col_w, bh, b, f);
        }
        y -= bh;
    }
    let right_end = y;

    // State lines, full width.
    let mut y = left_end.min(right_end);
    let (bw, bn, bi) = boxes::state_boxes(kind);
    let third = (RIGHT - LEFT) / 3.0;
    let heads = [
        format!("{bw} State tax withheld"),
        format!("{bn} State/Payer's state no."),
        format!("{bi} State income"),
    ];
    let sh = 24.0;
    for (i, h) in heads.iter().enumerate() {
        let x0 = LEFT + third * i as f32;
        p.rect(x0, y - sh, third, sh, 0.6);
        p.caption(x0 + 3.0, y - 7.0, h);
        let rows: Vec<String> = f
            .states
            .iter()
            .take(2)
            .map(|s| match i {
                0 => dollars(s.tax_withheld),
                1 => format!("{} {}", s.state.trim(), s.payer_state_no.trim())
                    .trim()
                    .to_owned(),
                _ => dollars(s.income),
            })
            .collect();
        let mut vy = y - 14.0;
        for r in rows {
            if i == 1 {
                p.text(x0 + 6.0, vy, 7.5, Font::Regular, &r);
            } else {
                p.text_right(x0 + third - 6.0, vy, 7.5, Font::Regular, &r);
            }
            vy -= 8.0;
        }
    }
    y -= sh;

    // Legend (Pub 1179 §4.4.2) and form footer.
    y -= 9.0;
    let legend = match copy {
        Copy::B => {
            "This is important tax information and is being furnished to the IRS. If you are \
             required to file a return, a negligence penalty or other sanction may be imposed \
             on you if this income is taxable and the IRS determines that it has not been reported."
        }
        Copy::C => "Keep this copy with your records.",
        Copy::One | Copy::Two => "",
    };
    for l in wrap(legend, 6.5, Font::Bold, RIGHT - LEFT) {
        p.text(LEFT, y, 6.5, Font::Bold, &l);
        y -= 7.5;
    }
    p.text(
        LEFT,
        y - 2.0,
        6.0,
        Font::Regular,
        &format!("Form {} (Rev. 12-2026)", kind.label()),
    );
    let url = match kind {
        FormKind::Nec => "www.irs.gov/Form1099NEC",
        FormKind::Misc => "www.irs.gov/Form1099MISC",
    };
    p.text_right(RIGHT, y - 2.0, 6.0, Font::Regular, url);
    let footer_y = y - 12.0;
    p.dashed(LEFT, footer_y, RIGHT, footer_y);
    p.cursor = footer_y - 12.0;
}

fn draw_box(p: &mut Page, x: f32, y: f32, w: f32, h: f32, b: &BoxDef, f: &Form) {
    p.rect(x, y - h, w, h, 0.6);
    let num = b.number();
    let font = if b.bold { Font::Bold } else { Font::Regular };
    let cap_x = if num.is_empty() {
        x + 3.0
    } else {
        p.text(x + 3.0, y - 7.0, 6.0, Font::Bold, num);
        x + 4.0 + width(num, 6.0, Font::Bold) + 2.0
    };
    let cap_lines = wrap(b.caption, CAP, font, x + w - cap_x - 3.0);
    let mut cy = y - 7.0;
    let shown = match b.kind {
        BoxKind::Check => 3,
        _ => 2,
    };
    for l in cap_lines.iter().take(shown) {
        p.text(cap_x, cy, CAP, font, l);
        cy -= 6.2;
    }
    match b.kind {
        BoxKind::Amount => {
            let a = f.amount(b.key);
            let s = if a.is_zero() {
                String::new()
            } else {
                dollars(a)
            };
            p.text_right(x + w - 6.0, y - h + 5.0, VAL, Font::Regular, &s);
        }
        BoxKind::Check => {
            let bx = x + w - 13.0;
            let by = y - h + 4.0;
            p.rect(bx, by, 8.0, 8.0, 0.6);
            if f.checked(b.key) {
                p.text(bx + 1.6, by + 1.4, 7.5, Font::Bold, "X");
            }
        }
        BoxKind::Codes => {
            p.text(x + 8.0, y - h + 5.0, VAL, Font::Regular, &f.ttoc.join("  "));
        }
    }
}

fn dollars(d: Decimal) -> String {
    format!("$ {}", money(d))
}

// ----- instructions -----------------------------------------------------------------------

fn instructions(p: &mut Page, kind: FormKind, year: i32, copy: Copy) {
    let mut y = p.cursor;
    let title = format!(
        "Instructions for Recipient — Form {} ({year})",
        kind.label()
    );
    p.text(LEFT, y, 8.5, Font::Bold, &title);
    y -= 12.0;
    let paras: Vec<(&str, String)> = recipient_text(kind)
        .iter()
        .map(|(h, t)| (*h, (*t).to_owned()))
        .collect();
    let note = match copy {
        Copy::B => None,
        _ => Some((
            "",
            "These instructions are printed for reference; the recipient's statement is Copy B."
                .to_owned(),
        )),
    };
    let size = 6.3;
    let lead = 7.4;
    let gutter = 14.0;
    let col_w = (RIGHT - LEFT - gutter) / 2.0;
    let top = y;
    let bottom = 30.0;
    let mut col = 0usize;
    let mut x = LEFT;
    let mut cy = top;
    for (head, text) in paras
        .iter()
        .map(|(h, t)| (*h, t.as_str()))
        .chain(note.as_ref().map(|(h, t)| (*h, t.as_str())))
    {
        // A bold run-in heading, then the paragraph.
        let full = if head.is_empty() {
            text.to_owned()
        } else {
            format!("{head} {text}")
        };
        let lines = wrap(&full, size, Font::Regular, col_w);
        for (i, l) in lines.iter().enumerate() {
            if cy < bottom {
                if col == 0 {
                    col = 1;
                    x = LEFT + col_w + gutter;
                    cy = top;
                } else {
                    break;
                }
            }
            if i == 0 && !head.is_empty() && l.starts_with(head) {
                p.text(x, cy, size, Font::Bold, head);
                let rest = l.get(head.len()..).unwrap_or_default();
                p.text(
                    x + width(head, size, Font::Bold),
                    cy,
                    size,
                    Font::Regular,
                    rest,
                );
            } else {
                p.text(x, cy, size, Font::Regular, l);
            }
            cy -= lead;
        }
        cy -= 2.5;
    }
}

/// Recipient instructions, written to match the official form's back (Pub 1179
/// §4.1.3 asks for instructions similar to the official ones).
fn recipient_text(kind: FormKind) -> &'static [(&'static str, &'static str)] {
    const COMMON_HEAD: [(&str, &str); 2] = [
        (
            "Recipient's taxpayer identification number (TIN).",
            "For your protection, this form may show only the last four digits of your TIN \
             (social security number (SSN), individual taxpayer identification number (ITIN), \
             adoption taxpayer identification number (ATIN), or employer identification number \
             (EIN)). However, the payer has reported your complete TIN to the IRS.",
        ),
        (
            "Account number.",
            "May show an account or other unique number the payer assigned to distinguish \
             your account.",
        ),
    ];
    const NEC: &[(&str, &str)] = &[
        COMMON_HEAD[0],
        COMMON_HEAD[1],
        (
            "Box 1a.",
            "Shows your total nonemployee compensation. If the amount in this box is \
             self-employment (SE) income, report it on Schedule C or F (Form 1040) if you are a \
             sole proprietor, or on Form 1065 and Schedule K-1 (Form 1065) if a partnership, and \
             complete Schedule SE (Form 1040). If you are not an employee of the payer and the \
             amount is not SE income (for example, from a hobby or a one-time activity), report \
             it as other income on Schedule 1 (Form 1040). If you believe you are an employee \
             and cannot get the payer to correct this form, report the amount as wages and see \
             Form 8919 and Pub. 1779.",
        ),
        (
            "Box 1b.",
            "Shows the total cash tips included in box 1a. Use it when working out any \
             deduction for qualified tips; see box 1c.",
        ),
        (
            "Box 1c.",
            "Shows up to two Treasury Tipped Occupation Codes for the tips in box 1b. If the \
             only code shown is 000, the tips are not qualified tips.",
        ),
        (
            "Box 1d.",
            "Shows the total qualified overtime compensation included in box 1a. Use it when \
             working out any deduction for qualified overtime compensation.",
        ),
        (
            "Box 2.",
            "If checked, the payer made direct sales of consumer products totaling $5,000 or \
             more to you for resale. Any income from the sale of these products is generally SE \
             income; report it on Schedule C (Form 1040).",
        ),
        (
            "Box 3.",
            "Shows excess golden parachute payments, subject to a 20% excise tax. See your tax \
             return instructions for where to report.",
        ),
        (
            "Box 4.",
            "Shows backup withholding. A payer must withhold if you did not give your TIN. See \
             Form W-9 and Pub. 505. Include this amount on your income tax return as tax withheld.",
        ),
        ("Boxes 5–7.", "State income tax withheld reporting boxes."),
        (
            "Future developments.",
            "For the latest information about developments related to Form 1099-NEC and its \
             instructions, such as legislation enacted after they were published, go to \
             www.irs.gov/Form1099NEC.",
        ),
        (
            "Free File Program.",
            "Go to www.irs.gov/FreeFile to see if you qualify for no-cost online federal tax \
             preparation, e-filing, and direct deposit or payment options.",
        ),
    ];
    const MISC: &[(&str, &str)] = &[
        COMMON_HEAD[0],
        COMMON_HEAD[1],
        (
            "Amounts shown may be subject to self-employment (SE) tax.",
            "Individuals should see the Instructions for Schedule SE (Form 1040). Corporations, \
             fiduciaries, or partnerships must report these amounts on the appropriate line of \
             their tax returns.",
        ),
        (
            "FATCA filing requirement.",
            "If checked, the payer is reporting on this Form 1099 to satisfy its account \
             reporting requirement under chapter 4 of the Internal Revenue Code.",
        ),
        (
            "Box 1.",
            "Report rents from real estate on Schedule E (Form 1040). However, report rents on \
             Schedule C (Form 1040) if you provided significant services to the tenant, sold \
             real estate as a business, or rented personal property as a business.",
        ),
        (
            "Box 2.",
            "Report royalties from oil, gas, or mineral properties, copyrights, and patents on \
             Schedule E (Form 1040). However, report payments for a working interest as \
             explained in the Schedule E instructions. For royalties on timber, coal, and iron \
             ore, see Pub. 544.",
        ),
        (
            "Box 3.",
            "Generally, report this amount on the \"Other income\" line of Schedule 1 (Form \
             1040) and identify the payment. If it is trade or business income, report it on \
             Schedule C or F (Form 1040).",
        ),
        (
            "Box 4.",
            "Shows backup withholding or withholding on Indian gaming profits. Generally, a \
             payer must backup withhold if you did not furnish your TIN. See Form W-9 and Pub. \
             505. Report this amount on your income tax return as tax withheld.",
        ),
        (
            "Box 5.",
            "Shows the amount paid to you as a fishing boat crew member by the operator, who \
             considers you to be self-employed. Self-employed individuals must report this \
             amount on Schedule C (Form 1040).",
        ),
        (
            "Box 6.",
            "For individuals, report on Schedule C (Form 1040).",
        ),
        (
            "Box 7.",
            "If checked, consumer products totaling $5,000 or more were sold to you for resale, \
             on a buy-sell, a deposit-commission, or other basis. Generally, report any income \
             from your sale of these products on Schedule C (Form 1040).",
        ),
        (
            "Box 8.",
            "Shows substitute payments in lieu of dividends or tax-exempt interest received by \
             your broker on your behalf as a result of a loan of your securities. Report on the \
             \"Other income\" line of Schedule 1 (Form 1040).",
        ),
        ("Box 9.", "Report this amount on Schedule F (Form 1040)."),
        (
            "Box 10.",
            "Shows gross proceeds paid to an attorney in connection with legal services. Report \
             only the taxable part as income on your return.",
        ),
        (
            "Box 11.",
            "Shows the amount of cash you received for the sale of fish if you are in the trade \
             or business of catching fish.",
        ),
        (
            "Box 12.",
            "May show current year deferrals as a nonemployee under a nonqualified deferred \
             compensation (NQDC) plan that is subject to the requirements of section 409A.",
        ),
        (
            "Boxes 13a and 13b.",
            "Box 13a shows cash tips; box 13b shows up to two Treasury Tipped Occupation Codes \
             for them (000 alone means the tips are not qualified tips).",
        ),
        (
            "Box 14.",
            "Shows qualified overtime compensation. Use it when working out any deduction for \
             qualified overtime compensation.",
        ),
        (
            "Box 15.",
            "Shows income as a nonemployee under an NQDC plan that does not meet the \
             requirements of section 409A. Any amount included in box 12 that is currently \
             taxable is also included in this box. Report this amount as income on your tax \
             return. This income is also subject to a substantial additional tax.",
        ),
        (
            "Boxes 16–18.",
            "Show state or local income tax withheld from the payments.",
        ),
        (
            "Future developments.",
            "For the latest information about developments related to Form 1099-MISC and its \
             instructions, such as legislation enacted after they were published, go to \
             www.irs.gov/Form1099MISC.",
        ),
    ];
    match kind {
        FormKind::Nec => NEC,
        FormKind::Misc => MISC,
    }
}

// ----- a minimal PDF writer ----------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Font {
    Regular,
    Bold,
}

impl Font {
    fn res(self) -> &'static str {
        match self {
            Font::Regular => "F1",
            Font::Bold => "F2",
        }
    }
}

#[derive(Debug, Default)]
struct Page {
    ops: Vec<u8>,
    /// Where the next block starts (from the top down).
    cursor: f32,
}

impl Page {
    fn text(&mut self, x: f32, y: f32, size: f32, font: Font, s: &str) {
        if s.is_empty() {
            return;
        }
        self.ops.extend(
            format!("BT /{} {} Tf {} {} Td (", font.res(), n(size), n(x), n(y)).into_bytes(),
        );
        self.ops.extend(escape(s));
        self.ops.extend(b") Tj ET\n");
    }

    fn text_right(&mut self, x_right: f32, y: f32, size: f32, font: Font, s: &str) {
        let w = width(s, size, font);
        self.text(x_right - w, y, size, font, s);
    }

    fn caption(&mut self, x: f32, y: f32, s: &str) {
        self.text(x, y, CAP, Font::Regular, s);
    }

    fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, line: f32) {
        self.ops.extend(
            format!("{} w {} {} {} {} re S\n", n(line), n(x), n(y), n(w), n(h)).into_bytes(),
        );
    }

    fn dashed(&mut self, x0: f32, y0: f32, x1: f32, y1: f32) {
        self.ops.extend(
            format!(
                "q 0.5 w [3 2] 0 d {} {} m {} {} l S Q\n",
                n(x0),
                n(y0),
                n(x1),
                n(y1)
            )
            .into_bytes(),
        );
    }
}

#[derive(Debug, Default)]
struct Doc {
    pages: Vec<Page>,
}

impl Doc {
    fn finish(self, title: &str) -> Vec<u8> {
        // Objects: 1 catalog, 2 pages, 3 F1, 4 F2, 5 info, then (page, content) pairs.
        let mut objs: Vec<Vec<u8>> = Vec::new();
        let page_ids: Vec<usize> = (0..self.pages.len()).map(|i| 6 + i * 2).collect();
        let kids: String = page_ids.iter().map(|id| format!("{id} 0 R ")).collect();
        objs.push(b"<< /Type /Catalog /Pages 2 0 R >>".to_vec());
        objs.push(
            format!(
                "<< /Type /Pages /Kids [{}] /Count {} >>",
                kids.trim_end(),
                self.pages.len()
            )
            .into_bytes(),
        );
        for base in ["Helvetica", "Helvetica-Bold"] {
            objs.push(
                format!(
                    "<< /Type /Font /Subtype /Type1 /BaseFont /{base} /Encoding /WinAnsiEncoding >>"
                )
                .into_bytes(),
            );
        }
        let mut info = b"<< /Title (".to_vec();
        info.extend(escape(title));
        info.extend(b") /Producer (Form1099Keep) /Creator (Form1099Keep) >>");
        objs.push(info);
        for (i, page) in self.pages.iter().enumerate() {
            let content_id = 7 + i * 2;
            objs.push(
                format!(
                    "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] \
                     /Resources << /Font << /F1 3 0 R /F2 4 0 R >> >> /Contents {content_id} 0 R >>"
                )
                .into_bytes(),
            );
            let body = page.ops.as_slice();
            let mut s = format!("<< /Length {} >>\nstream\n", body.len()).into_bytes();
            s.extend_from_slice(body);
            s.extend_from_slice(b"\nendstream");
            objs.push(s);
        }
        let mut out = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
        let mut offsets = Vec::with_capacity(objs.len());
        for (i, o) in objs.iter().enumerate() {
            offsets.push(out.len());
            out.extend(format!("{} 0 obj\n", i + 1).into_bytes());
            out.extend_from_slice(o);
            out.extend_from_slice(b"\nendobj\n");
        }
        let xref = out.len();
        out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).into_bytes());
        for off in offsets {
            out.extend(format!("{off:010} 00000 n \n").into_bytes());
        }
        out.extend(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R /Info 5 0 R >>\nstartxref\n{xref}\n%%EOF\n",
                objs.len() + 1
            )
            .into_bytes(),
        );
        out
    }
}

/// A number with at most two decimals.
fn n(v: f32) -> String {
    let s = format!("{v:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_owned()
}

/// Text as WinAnsi bytes inside a PDF string, with `(`, `)` and `\` escaped.
fn escape(s: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    for c in s.chars() {
        let b = winansi(c);
        if matches!(b, b'(' | b')' | b'\\') {
            out.push(b'\\');
        }
        out.push(b);
    }
    out
}

fn winansi(c: char) -> u8 {
    match c {
        ' '..='~' => c as u8,
        '\u{a0}'..='\u{ff}' => u32::from(c) as u8,
        '€' => 0x80,
        '‚' => 0x82,
        '„' => 0x84,
        '…' => 0x85,
        '‘' => 0x91,
        '’' => 0x92,
        '“' => 0x93,
        '”' => 0x94,
        '•' => 0x95,
        '–' => 0x96,
        '—' => 0x97,
        '™' => 0x99,
        _ => b'?',
    }
}

/// Helvetica and Helvetica-Bold advance widths (1/1000 em) for ASCII 32–126 (the
/// standard AFM metrics); other characters use 556.
const HELV: [u16; 95] = [
    278, 278, 355, 556, 556, 889, 667, 191, 333, 333, 389, 584, 278, 333, 278, 278, 556, 556, 556,
    556, 556, 556, 556, 556, 556, 556, 278, 278, 584, 584, 584, 556, 1015, 667, 667, 722, 722, 667,
    611, 778, 722, 278, 500, 667, 556, 833, 722, 778, 667, 778, 722, 667, 611, 722, 667, 944, 667,
    667, 611, 278, 278, 278, 469, 556, 333, 556, 556, 500, 556, 556, 278, 556, 556, 222, 222, 500,
    222, 833, 556, 556, 556, 556, 333, 500, 278, 556, 500, 722, 500, 500, 500, 334, 260, 334, 584,
];
const HELV_BOLD: [u16; 95] = [
    278, 333, 474, 556, 556, 889, 722, 238, 333, 333, 389, 584, 278, 333, 278, 278, 556, 556, 556,
    556, 556, 556, 556, 556, 556, 556, 333, 333, 584, 584, 584, 611, 975, 722, 722, 722, 722, 667,
    611, 778, 722, 278, 556, 722, 611, 833, 722, 778, 667, 778, 722, 667, 611, 722, 667, 944, 667,
    667, 611, 333, 278, 333, 584, 556, 333, 556, 611, 556, 611, 556, 333, 611, 611, 278, 278, 556,
    278, 889, 611, 611, 611, 611, 389, 556, 333, 611, 556, 778, 556, 556, 500, 389, 280, 389, 584,
];

fn width(s: &str, size: f32, font: Font) -> f32 {
    let table = match font {
        Font::Regular => &HELV,
        Font::Bold => &HELV_BOLD,
    };
    let units: u32 = s
        .chars()
        .map(|c| {
            let i = (c as u32).wrapping_sub(32) as usize;
            u32::from(table.get(i).copied().unwrap_or(556))
        })
        .sum();
    units as f32 * size / 1000.0
}

/// Greedy word wrap to `max` points.
fn wrap(s: &str, size: f32, font: Font, max: f32) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in s.split_whitespace() {
        let candidate = if line.is_empty() {
            word.to_owned()
        } else {
            format!("{line} {word}")
        };
        if width(&candidate, size, font) <= max || line.is_empty() {
            line = candidate;
        } else {
            lines.push(std::mem::take(&mut line));
            line = word.to_owned();
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

/// Cut text to fit `max` points, ending with `…` when cut.
fn fit(s: &str, size: f32, font: Font, max: f32) -> String {
    if width(s, size, font) <= max {
        return s.to_owned();
    }
    let mut out = String::new();
    for c in s.chars() {
        let next = format!("{out}{c}…");
        if width(&next, size, font) > max {
            break;
        }
        out.push(c);
    }
    format!("{out}…")
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::model::{Address, NameType, TinType};
    use std::str::FromStr;

    fn sample() -> (Form, Payer, Recipient) {
        let addr = Address {
            line1: "100 Church St".into(),
            city: "Akron".into(),
            state: "OH".into(),
            zip: "443011234".into(),
            ..Address::default()
        };
        let payer = Payer {
            name: "Grace (Community) Chapel".into(),
            tin: "123456789".into(),
            address: addr.clone(),
            phone: "330-555-0100".into(),
            ..Payer::default()
        };
        let rec = Recipient {
            name_type: NameType::Person,
            first: "Zoë".into(),
            last: "O’Brien".into(),
            tin_type: Some(TinType::Ssn),
            tin: "123456789".into(),
            address: addr,
            ..Recipient::default()
        };
        let mut f = Form::default();
        f.set_amount("1a", Decimal::from_str("12345.6").unwrap());
        f.set_amount("4", Decimal::from_str("100").unwrap());
        (f, payer, rec)
    }

    #[test]
    fn writes_a_wellformed_pdf() {
        let (f, p, r) = sample();
        let items = [Item {
            form: &f,
            payer: &p,
            recipient: &r,
        }];
        let opts = Options {
            copies: vec![Copy::B, Copy::Two],
            truncate_recipient_tin: true,
        };
        let pdf = render(&items, &opts);
        let text = String::from_utf8_lossy(&pdf);
        assert!(text.starts_with("%PDF-1.4"));
        assert!(text.contains("/Count 2"));
        assert!(text.contains("12,345.60"));
        assert!(text.contains("***-**-6789"));
        assert!(!text.contains("123-45-6789"), "recipient TIN not truncated");
        assert!(
            text.contains("12-3456789"),
            "payer TIN must be shown in full"
        );
        assert!(text.contains("Grace \\(Community\\) Chapel"));
        // xref offsets point at objects.
        let start: usize = text
            .rsplit("startxref\n")
            .next()
            .unwrap()
            .lines()
            .next()
            .unwrap()
            .parse()
            .unwrap();
        assert!(pdf.get(start..).unwrap().starts_with(b"xref"));
    }

    #[test]
    fn metrics_and_wrap() {
        assert!((width("0", 10.0, Font::Regular) - 5.56).abs() < 0.01);
        let lines = wrap("one two three four five six", 10.0, Font::Regular, 40.0);
        assert!(lines.len() > 1);
        assert!(fit("a very long name indeed", 9.0, Font::Regular, 30.0).ends_with('…'));
    }
}
