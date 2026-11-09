//! Start pages (New Tab, Incognito): no header or navigation; a centred logo and title, a search
//! box that hands the typing to the address bar (like Chrome's), shortcut tiles, cards, and the
//! navigation as chips at the bottom.

use windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F;

use super::input::Target;
use super::layout::{place_sections, Layout, Measure, RowBox};
use super::model::PageDoc;
use super::paint::Pen;
use super::model::Bounce;
use super::{Inner, BOUNCE_MS, HOP_MS, TILE_DELAY_MS, TILE_MS, TILE_STAGGER_MS};
use crate::motion;
use crate::frame::layout::rect;
use crate::frame::style::{glyph, tile_color};

const COLUMN: f32 = 640.0;
const LOGO: f32 = 56.0;
/// The logo of a bare start page (just the logo and the search box, in the middle).
const BIG_LOGO: f32 = 104.0;
const FAKEBOX_W: f32 = 560.0;
const FAKEBOX_H: f32 = 54.0;
const TILE: f32 = 112.0;
const TILES_PER_ROW: usize = 4;
const CHIP_H: f32 = 36.0;

/// Section whose rows are drawn as shortcut tiles.
pub const TILES: &str = "tiles";

/// A start page with nothing but the logo (and the search box): shown big, in the middle.
fn bare(doc: &PageDoc) -> bool {
    doc.title.is_empty() && doc.hero_text.is_empty() && doc.sections.is_empty() && doc.nav.is_empty()
}

pub(super) fn layout_start(doc: &PageDoc, width: f32, height: f32, m: &dyn Measure) -> Layout {
    let col_w = (width - 48.0).clamp(240.0, COLUMN);
    let (col_x, cx) = ((width - col_w) / 2.0, width / 2.0);
    let mut out = Layout { width, column: (col_x, col_w), ..Default::default() };
    if bare(doc) {
        let search = if doc.fakebox.is_empty() { 0.0 } else { 44.0 + FAKEBOX_H };
        let top = ((height - super::layout::HEADER_H - BIG_LOGO - search) / 2.0 - 36.0).max(24.0);
        out.hero = Some((rect(cx - BIG_LOGO / 2.0, top, BIG_LOGO, BIG_LOGO), rect(0.0, 0.0, 0.0, 0.0), rect(0.0, 0.0, 0.0, 0.0)));
        if !doc.fakebox.is_empty() {
            let w = (FAKEBOX_W + 60.0).min(col_w);
            out.fakebox = Some(rect(cx - w / 2.0, top + BIG_LOGO + 44.0, w, FAKEBOX_H));
        }
        out.height = top + BIG_LOGO + search + 48.0;
        return out;
    }
    let mut y = 32.0;
    let logo = rect(cx - LOGO / 2.0, y, LOGO, LOGO);
    y += LOGO + 18.0;
    let title = rect(col_x, y, col_w, 40.0);
    y += 44.0;
    let text_h = if doc.hero_text.is_empty() { 0.0 } else { m.height(&doc.hero_text, col_w) * 1.15 + 4.0 };
    let text = rect(col_x, y, col_w, text_h);
    y += text_h + if text_h > 0.0 { 24.0 } else { 8.0 };
    out.hero = Some((logo, title, text));
    if !doc.fakebox.is_empty() {
        let w = FAKEBOX_W.min(col_w);
        out.fakebox = Some(rect(cx - w / 2.0, y, w, FAKEBOX_H));
        y += FAKEBOX_H + 32.0;
    }
    if let Some(s) = doc.sections.iter().position(|s| s.id == TILES) {
        let rows = &doc.sections[s].cards.first().map(Vec::as_slice).unwrap_or(&[]);
        let per_row = TILES_PER_ROW.min(rows.len()).max(1);
        for (k, _) in rows.iter().enumerate() {
            let (line, col) = (k / per_row, k % per_row);
            let in_line = per_row.min(rows.len() - line * per_row);
            let x = cx - in_line as f32 * TILE / 2.0 + col as f32 * TILE;
            let cell = rect(x, y + line as f32 * TILE, TILE, TILE);
            let label = rect(x + 8.0, cell.top + 70.0, TILE - 16.0, 24.0);
            out.rows.push(RowBox { section: s, card: 0, row: k, rect: cell, text: label, control: vec![], divider: false, tile: true });
        }
        y += rows.len().div_ceil(per_row) as f32 * TILE + 24.0;
    }
    y = place_sections(&mut out, doc, |s| s.id != TILES, |_| true, col_x, col_w, y, m);
    // Navigation as chips, centred, wrapped onto more lines when needed.
    let widths: Vec<f32> = doc.nav.iter().map(|n| m.width(&n.label) + 56.0).collect();
    let mut k = 0;
    while k < widths.len() {
        let mut end = k;
        let mut line = 0.0;
        while end < widths.len() && (end == k || line + widths[end] + 8.0 <= col_w) {
            line += widths[end] + if end > k { 8.0 } else { 0.0 };
            end += 1;
        }
        let mut x = cx - line / 2.0;
        for w in &widths[k..end] {
            out.chips.push(rect(x, y + 8.0, *w, CHIP_H));
            x += w + 8.0;
        }
        y += CHIP_H + 12.0;
        k = end;
    }
    out.height = y + 48.0;
    out
}

impl Inner {
    /// The start page's own pieces, in scrolled coordinates (cards and rows are drawn as usual).
    /// Where the bouncing logo is: (lift above its resting place, squash when it lands).
    fn logo_motion(&self) -> (f32, f32) {
        match self.doc.logo_bounce {
            Bounce::None => (0.0, 0.0),
            Bounce::Once => {
                // Dropped in from above, bouncing lower and lower until it rests.
                let t = motion::progress(self.enter, 0.0, BOUNCE_MS);
                let lift = (1.0 - motion::ease_out_bounce(t)) * 90.0;
                (lift, if lift < 3.0 && t < 1.0 { 0.08 * (1.0 - t) } else { 0.0 })
            }
            Bounce::Loop => {
                // Quick hops, up and down: a parabola, squashed for a moment on the ground.
                let Some(ph) = motion::phase(self.enter, HOP_MS) else { return (0.0, 0.0) };
                let height = 4.0 * ph * (1.0 - ph);
                (height * 46.0, 0.16 * (1.0 - height).powi(8))
            }
        }
    }

    /// `h`: height of the page window (the waiting logo sits in its middle).
    pub(super) fn draw_start(&self, p: &Pen) {
        let (c, f) = (&p.c, &self.fonts);
        if let Some((logo, title, text)) = self.layout.hero {
            let (x, y) = ((logo.left + logo.right) / 2.0, (logo.top + logo.bottom) / 2.0);
            let size = logo.right - logo.left;
            // Little shapes and sparkles around the logo, spread sideways (clear of the title).
            let k = size / LOGO;
            super::art::decorations(p, f, x, y, 1.9 * k, 0.75 * k.max(1.4), motion::eased(self.enter, 60.0, 360.0), self.dark);
            if self.doc.hero_icon.is_empty() {
                // The brand mark's purple-to-indigo, bouncing when asked: its shadow on the ground
                // shrinks and fades as it rises.
                let (lift, squash) = self.logo_motion();
                let r = size / 2.0;
                if self.doc.logo_bounce != Bounce::None {
                    let high = (lift / 90.0).min(1.0);
                    p.oval(x, y + r + 9.0, r * (0.8 - 0.35 * high), 5.0 * (1.0 - 0.4 * high), 0x000000, if self.dark { 0.35 } else { 0.12 } * (1.0 - 0.6 * high));
                }
                let (rx, ry) = (r * (1.0 + squash), r * (1.0 - squash));
                let cy = y - lift + (r - ry);
                p.gradient_oval(x, cy, rx, ry, 0x9333ea, 0x4f46e5);
                let font = if size > LOGO { &f.logo_big } else { &f.logo };
                p.text("K", rect(logo.left, cy - r, size, size), font, 0xffffff);
            } else {
                p.circle(x, y, LOGO / 2.0, c.selected, None);
                p.text(&self.doc.hero_icon, logo, &f.hero_icon, c.brand);
            }
            p.text(&self.doc.title, title, &f.hero, c.text);
            if text.bottom > text.top {
                p.text(&self.doc.hero_text, text, &f.hero_text, c.muted);
            }
        }
        if let Some(r) = self.layout.fakebox {
            // A floating search box: a soft shadow under it.
            let shadow = |dy: f32, a: f32| p.fill(D2D_RECT_F { top: r.top + dy, bottom: r.bottom + dy, ..r }, FAKEBOX_H / 2.0, 0x000000, a);
            shadow(4.0, if self.dark { 0.22 } else { 0.05 });
            shadow(1.0, if self.dark { 0.25 } else { 0.06 });
            let edit = self.fakebox_edit();
            // SAFETY: plain query.
            let typing = edit.is_some_and(|e| e == unsafe { windows::Win32::UI::Input::KeyboardAndMouse::GetFocus() });
            let filled = edit.is_some_and(|e| !crate::frame::edit_text(e).is_empty());
            p.fill(r, FAKEBOX_H / 2.0, if typing || filled { c.field } else { c.card }, 1.0);
            let ring = if typing { c.brand } else { motion::mix(c.line, c.brand, self.lit(Target::Fakebox).min(1.0)) };
            p.outline(r, FAKEBOX_H / 2.0, ring, if typing { 2.0 } else { 1.0 });
            p.text(glyph::SEARCH, rect(r.left + 14.0, r.top, 28.0, FAKEBOX_H), &f.icons, if typing { c.brand } else { c.muted });
            if !typing && !filled {
                p.text(&self.doc.fakebox, rect(r.left + 52.0, r.top, r.right - r.left - 64.0, FAKEBOX_H), &f.title, c.muted);
            }
        }
        for (k, chip) in self.layout.chips.iter().enumerate() {
            let item = &self.doc.nav[k];
            let a = self.lit(Target::Nav(k));
            if a > 0.001 {
                p.fill(*chip, CHIP_H / 2.0, c.hover.0, c.hover.1 * 1.5 * a);
            }
            p.outline(*chip, CHIP_H / 2.0, c.line, 1.0);
            p.text(&item.icon, rect(chip.left + 12.0, chip.top, 24.0, CHIP_H), &f.icons, c.brand);
            p.text(&item.label, rect(chip.left + 42.0, chip.top, chip.right - chip.left - 50.0, CHIP_H), &f.nav, c.text);
        }
    }

    /// One shortcut tile: a lettered badge and the site's name.
    pub(super) fn draw_tile(&self, p: &Pen, k: usize, b: &RowBox) {
        let (c, f) = (&p.c, &self.fonts);
        let row = &self.doc.sections[b.section].cards[b.card][b.row];
        // Tiles rise into place one after another.
        let rise = motion::eased(self.enter, TILE_DELAY_MS + k as f32 * TILE_STAGGER_MS, TILE_MS);
        let dy = (1.0 - rise) * 22.0;
        let b = &RowBox { rect: shift(b.rect, dy), text: shift(b.text, dy), ..b.clone() };
        let card = inset(b.rect, 6.0);
        // Hovered, a tile firms up, lifts a little and gains an edge.
        let lit = self.lit(Target::Row(k)).min(1.0);
        let card = shift(card, -2.0 * lit);
        if lit > 0.001 {
            p.fill(shift(card, 3.0), 16.0, 0x000000, (if self.dark { 0.18 } else { 0.05 }) * lit);
        }
        p.fill(card, 16.0, c.card, 0.55 + 0.45 * lit);
        if lit > 0.001 {
            p.outline(card, 16.0, c.line, 1.0);
        }
        let name = row.sub.trim_start_matches("www.");
        let (x, y) = ((b.rect.left + b.rect.right) / 2.0, b.rect.top + 38.0);
        let badge = rect(x - 21.0, y - 21.0, 42.0, 42.0);
        // The site's own icon (`site:<host>`, loaded by the browser) on a light plate, like
        // Chrome's shortcuts; a lettered badge until it is there.
        p.fill(badge, 21.0, if self.dark { 0x34323c } else { 0xf1eff6 }, 1.0);
        if !p.icon(rect(x - 12.0, y - 12.0, 24.0, 24.0), &format!("site:{name}")) {
            p.fill(badge, 12.0, tile_color(name), 1.0);
            let letter = name.chars().next().map(|ch| ch.to_uppercase().to_string()).unwrap_or_else(|| "?".into());
            p.text(&letter, badge, &f.button, 0xffffff);
        }
        p.text(&row.title, b.text, &f.tile, c.text);
        if rise < 1.0 {
            p.fill(inset(b.rect, 2.0), 16.0, c.bg, 1.0 - rise);
        }
    }
}

fn shift(r: D2D_RECT_F, dy: f32) -> D2D_RECT_F {
    D2D_RECT_F { top: r.top + dy, bottom: r.bottom + dy, ..r }
}

fn inset(r: D2D_RECT_F, by: f32) -> D2D_RECT_F {
    D2D_RECT_F { left: r.left + by, top: r.top + by, right: r.right - by, bottom: r.bottom - by }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::page::model::{Control, NavItem, Row, Section};

    struct Fixed;
    impl Measure for Fixed {
        fn width(&self, t: &str) -> f32 {
            t.chars().count() as f32 * 7.0
        }
        fn height(&self, _: &str, _: f32) -> f32 {
            17.0
        }
    }

    #[test]
    fn hero_search_tiles_and_chips_stack_down_the_middle() {
        let tile = |n: &str| Row::new(n, &format!("{n}.example"), Control::Link { id: format!("https://{n}.example/") });
        let doc = PageDoc {
            start: true,
            title: "Kakangkung".into(),
            fakebox: "Search Google or type a URL".into(),
            sections: vec![Section { id: TILES.into(), cards: vec![(0..6).map(|k| tile(&format!("s{k}"))).collect()], ..Default::default() }],
            nav: (0..3).map(|k| NavItem { id: format!("n{k}"), label: format!("Link {k}"), icon: String::new() }).collect(),
            ..Default::default()
        };
        let l = layout_start(&doc, 1200.0, 800.0, &Fixed);
        let fake = l.fakebox.expect("search box");
        assert!((fake.left + fake.right - 1200.0).abs() < 0.5, "centred");
        assert_eq!(l.rows.len(), 6);
        assert!(l.rows.iter().all(|r| r.tile && r.rect.top >= fake.bottom));
        // Four tiles on the first line, two centred on the second.
        assert_eq!(l.rows[4].rect.top, l.rows[0].rect.top + TILE);
        assert!((l.rows[4].rect.left + l.rows[5].rect.right - 1200.0).abs() < 0.5);
        assert_eq!(l.chips.len(), 3);
        assert!(l.chips[0].top > l.rows[5].rect.bottom);
    }
}

#[cfg(test)]
mod bare_tests {
    use super::*;

    struct Fixed;
    impl Measure for Fixed {
        fn width(&self, t: &str) -> f32 {
            t.chars().count() as f32 * 7.0
        }
        fn height(&self, _: &str, _: f32) -> f32 {
            17.0
        }
    }

    #[test]
    fn a_bare_start_page_is_a_big_logo_and_the_search_box_in_the_middle() {
        let doc = PageDoc { start: true, fakebox: "Search".into(), ..Default::default() };
        let l = layout_start(&doc, 1200.0, 900.0, &Fixed);
        let (logo, _, _) = l.hero.expect("logo");
        let fake = l.fakebox.expect("search box");
        assert_eq!(logo.right - logo.left, BIG_LOGO);
        let middle = (logo.top + fake.bottom) / 2.0;
        assert!((middle - (900.0 - crate::page::layout::HEADER_H) / 2.0).abs() < 60.0, "{middle}");
        assert!(l.rows.is_empty() && l.chips.is_empty());
    }
}
