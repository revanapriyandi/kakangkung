//! Where everything of a page goes (DIPs): header with the search box, left navigation, and the
//! content column of section titles and cards. Pure arithmetic over the description, so the
//! painter and the hit-testing read the same boxes.

use windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F;

use super::model::{Control, PageDoc, Row, Section};
use crate::frame::layout::rect;

pub(super) const HEADER_H: f32 = 64.0;
pub(super) const NAV_W: f32 = 256.0;
pub(super) const NAV_ITEM_H: f32 = 40.0;
/// Narrower windows hide the navigation (like Chrome).
const NAV_MIN_WINDOW: f32 = 900.0;
const CONTENT_MAX: f32 = 680.0;
const SIDE: f32 = 24.0;
const TITLE_H: f32 = 52.0;
const ROW_PAD_X: f32 = 20.0;
const ROW_PAD_Y: f32 = 12.0;
const ROW_MIN_H: f32 = 52.0;
const HEADING_H: f32 = 40.0;
const TITLE_LINE: f32 = 20.0;
const INDENT: f32 = 28.0;
pub(super) const TOGGLE_W: f32 = 36.0;
pub(super) const TOGGLE_H: f32 = 20.0;
pub(super) const CONTROL_H: f32 = 32.0;
const SELECT_W: f32 = 210.0;
const FIELD_W: f32 = 230.0;
const GAP: f32 = 8.0;
const LEAD_RADIO: f32 = 34.0;
const LEAD_AVATAR: f32 = 56.0;
const BOTTOM: f32 = 64.0;

/// Text sizes the layout needs (DirectWrite in the window, a stand-in in tests).
pub(super) trait Measure {
    /// Width of one line of button text.
    fn width(&self, text: &str) -> f32;
    /// Height of secondary text wrapped to `width`.
    fn height(&self, text: &str, width: f32) -> f32;
}

/// One row of a card, placed.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct RowBox {
    pub section: usize,
    pub card: usize,
    pub row: usize,
    pub rect: D2D_RECT_F,
    /// Title and secondary text.
    pub text: D2D_RECT_F,
    /// The control's pieces: one box per button; for a text field the field then its button.
    pub control: Vec<D2D_RECT_F>,
    /// A line above the row (not on a card's first row).
    pub divider: bool,
    /// Drawn as a shortcut tile (start pages).
    pub tile: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct Layout {
    pub width: f32,
    pub nav: bool,
    /// Content column: x and width.
    pub column: (f32, f32),
    pub titles: Vec<(D2D_RECT_F, usize)>,
    pub cards: Vec<D2D_RECT_F>,
    pub rows: Vec<RowBox>,
    /// Where each shown section starts (scroll position that puts its title at the top).
    pub anchors: Vec<(String, f32)>,
    /// Height of the scrolled content.
    pub height: f32,
    /// Start pages: logo, title and text; the search box; the navigation chips.
    pub hero: Option<(D2D_RECT_F, D2D_RECT_F, D2D_RECT_F)>,
    pub fakebox: Option<D2D_RECT_F>,
    pub chips: Vec<D2D_RECT_F>,
}

impl Layout {
    pub fn search_box(&self) -> D2D_RECT_F {
        rect(self.column.0, 12.0, self.column.1, 40.0)
    }

    pub fn nav_item(&self, index: usize) -> D2D_RECT_F {
        rect(12.0, HEADER_H + 8.0 + index as f32 * NAV_ITEM_H, NAV_W - 24.0, NAV_ITEM_H - 4.0)
    }
}

/// Lay out `doc` for a window `width` DIPs wide. One section is a page of its own (`current`,
/// "" = all); a search (`filter`) shows the matching sections of every page.
pub(super) fn layout(doc: &PageDoc, width: f32, height: f32, filter: &str, current: &str, m: &dyn Measure) -> Layout {
    if doc.start {
        return super::start::layout_start(doc, width, height, m);
    }
    let nav = width >= NAV_MIN_WINDOW && !doc.nav.is_empty();
    let left = if nav { NAV_W } else { 0.0 };
    let col_w = (width - left - 2.0 * SIDE).clamp(200.0, CONTENT_MAX);
    let col_x = left + ((width - left - col_w) / 2.0).max(SIDE);
    let mut out = Layout { width, nav, column: (col_x, col_w), ..Default::default() };
    let shown = |s: &Section| match (filter.is_empty(), doc.paged) {
        (true, _) => current.is_empty() || s.id == current,
        (false, true) => s.matches(filter),
        // Lists keep the matching rows (below); a section without any is left out.
        (false, false) => s.cards.iter().flatten().any(|r| r.matches(filter)),
    };
    let row_shown = |r: &Row| doc.paged || r.matches(filter);
    let y = place_sections(&mut out, doc, shown, row_shown, col_x, col_w, 0.0, m);
    out.height = y + BOTTOM;
    out
}

/// Section titles and cards of the sections `shown` lets through (and their rows `row_shown`
/// lets through), from `y` down; returns where they end.
#[allow(clippy::too_many_arguments)]
pub(super) fn place_sections(
    out: &mut Layout,
    doc: &PageDoc,
    shown: impl Fn(&Section) -> bool,
    row_shown: impl Fn(&Row) -> bool,
    col_x: f32,
    col_w: f32,
    mut y: f32,
    m: &dyn Measure,
) -> f32 {
    for (s, section) in doc.sections.iter().enumerate().filter(|(_, s)| shown(s)) {
        out.anchors.push((section.id.clone(), y));
        if !section.title.is_empty() {
            out.titles.push((rect(col_x + 4.0, y + 16.0, col_w - 8.0, TITLE_H - 16.0), s));
            y += TITLE_H;
        } else {
            y += 16.0;
        }
        for (c, card) in section.cards.iter().enumerate() {
            if !card.iter().any(&row_shown) {
                continue;
            }
            let top = y;
            let mut first = true;
            for (r, row) in card.iter().enumerate().filter(|(_, r)| row_shown(r)) {
                let b = place_row(row, col_x, col_w, y, m);
                let h = b.0;
                let divider = !first && !matches!(card[r - 1].control, Control::Heading);
                out.rows.push(RowBox { section: s, card: c, row: r, rect: rect(col_x, y, col_w, h), text: b.1, control: b.2, divider, tile: false });
                first = false;
                y += h;
            }
            out.cards.push(rect(col_x, top, col_w, y - top));
            y += 16.0;
        }
    }
    y
}

/// Height, text box and control boxes of one row whose top is at `y`.
fn place_row(row: &Row, x: f32, w: f32, y: f32, m: &dyn Measure) -> (f32, D2D_RECT_F, Vec<D2D_RECT_F>) {
    let right = x + w - ROW_PAD_X;
    let lead = if row.indent { INDENT } else { 0.0 }
        + match row.control {
            Control::Radio { .. } => LEAD_RADIO,
            Control::Avatar { .. } | Control::AvatarImage { .. } => LEAD_AVATAR,
            Control::Entry { .. } => LEAD_RADIO,
            _ => 0.0,
        };
    let text_x = x + ROW_PAD_X + lead;
    // Control widths from the right edge, right to left.
    let widths: Vec<f32> = match &row.control {
        Control::Toggle { .. } => vec![TOGGLE_W],
        Control::Select { .. } => vec![SELECT_W],
        Control::Buttons(b) => b.iter().map(|b| m.width(&b.label) + 32.0).collect(),
        Control::Text { button, .. } if button.is_empty() => vec![FIELD_W],
        Control::Text { button, .. } => vec![FIELD_W, m.width(button) + 32.0],
        Control::Link { .. } => vec![24.0],
        Control::Entry { .. } => vec![CONTROL_H],
        Control::Value(text) => vec![m.width(text) + 4.0],
        _ => vec![],
    };
    let used: f32 = widths.iter().sum::<f32>() + GAP * widths.len().saturating_sub(1) as f32;
    let text_w = (right - text_x - if used > 0.0 { used + 16.0 } else { 0.0 }).max(80.0);
    let h = match row.control {
        Control::Heading => HEADING_H,
        Control::Illustration { .. } => {
            let sub = if row.sub.is_empty() { 0.0 } else { m.height(&row.sub, w - 120.0) * 1.15 + 4.0 };
            return (super::art::ART_H + 44.0 + sub + 24.0, rect(x + 60.0, y, w - 120.0, 0.0), vec![]);
        }
        _ => {
            let sub = if row.sub.is_empty() { 0.0 } else { m.height(&row.sub, text_w) + 2.0 };
            (2.0 * ROW_PAD_Y + TITLE_LINE + sub).max(ROW_MIN_H)
        }
    };
    let mut boxes = vec![];
    let mut cx = right - used;
    for wi in widths {
        let ch = if matches!(row.control, Control::Toggle { .. }) { TOGGLE_H } else if matches!(row.control, Control::Link { .. }) { 24.0 } else { CONTROL_H };
        boxes.push(rect(cx, y + (h - ch) / 2.0, wi, ch));
        cx += wi + GAP;
    }
    (h, rect(text_x, y, text_w, h), boxes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::page::model::{Button, ButtonStyle, NavItem, Section};

    struct Fixed;
    impl Measure for Fixed {
        fn width(&self, t: &str) -> f32 {
            t.chars().count() as f32 * 7.0
        }
        fn height(&self, t: &str, w: f32) -> f32 {
            ((t.chars().count() as f32 * 6.5 / w).ceil()).max(1.0) * 17.0
        }
    }

    fn doc() -> PageDoc {
        let toggle = |t: &str| Row::new(t, "", Control::Toggle { id: t.into(), on: true });
        PageDoc {
            nav: vec![NavItem { id: "a".into(), label: "A".into(), icon: String::new() }],
            sections: vec![
                Section { id: "a".into(), title: "Alpha".into(), keywords: String::new(), cards: vec![vec![Row::heading("Head"), toggle("one"), toggle("two")]] },
                Section {
                    id: "b".into(),
                    title: "Beta".into(),
                    keywords: "zzz".into(),
                    cards: vec![vec![Row::new("Go", "a long explanation ".repeat(10).trim(), Control::Buttons(vec![Button::new("x", "Clear", ButtonStyle::Danger)]))]],
                },
            ],
            ..Default::default()
        }
    }

    #[test]
    fn rows_stack_inside_cards_and_controls_sit_on_the_right() {
        let l = layout(&doc(), 1200.0, 800.0, "", "", &Fixed);
        assert!(l.nav);
        assert_eq!(l.rows.len(), 4);
        assert_eq!(l.cards.len(), 2);
        // Rows follow each other without gaps; the row after a heading has no divider.
        assert_eq!(l.rows[1].rect.top, l.rows[0].rect.bottom);
        assert!(!l.rows[1].divider && l.rows[2].divider);
        let (cx, cw) = l.column;
        let toggle = l.rows[1].control[0];
        assert!((toggle.right - (cx + cw - ROW_PAD_X)).abs() < 0.01);
        // Long secondary text wraps: that row is taller than the minimum.
        assert!(l.rows[3].rect.bottom - l.rows[3].rect.top > ROW_MIN_H);
        assert!(l.rows[3].text.right <= l.rows[3].control[0].left);
    }

    #[test]
    fn one_section_per_page_and_search_looks_in_all() {
        let l = layout(&doc(), 1200.0, 800.0, "", "b", &Fixed);
        assert_eq!(l.anchors.len(), 1);
        assert_eq!(l.anchors[0].0, "b");
        let l = layout(&PageDoc { paged: true, ..doc() }, 700.0, 800.0, "ZZZ", "a", &Fixed);
        assert!(!l.nav);
        assert_eq!(l.anchors.len(), 1);
        assert_eq!(l.anchors[0].0, "b");
        assert_eq!(l.rows.len(), 1);
    }

    #[test]
    fn lists_keep_only_matching_rows() {
        let l = layout(&doc(), 1200.0, 800.0, "two", "", &Fixed);
        assert_eq!(l.anchors.len(), 1);
        assert_eq!(l.rows.len(), 1);
        assert!(!l.rows[0].divider);
    }
}
