//! Small vector illustrations drawn with Direct2D (no image files, nothing kept in memory):
//! an icon on a floating card over soft colour shapes, with dots, a ring and sparkles around it.
//! Used for empty states (no history yet, no downloads…) and around the start pages' logo.

use windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F;

use super::paint::Pen;
use super::Fonts;
use crate::frame::layout::rect;
use crate::motion;

/// Accent colours of the decorations (purple, blue, pink, amber).
const PURPLE: u32 = 0x8b5cf6;
const BLUE: u32 = 0x3b82f6;
const PINK: u32 = 0xec4899;
const AMBER: u32 = 0xf59e0b;

/// Height an illustration takes (DIPs).
pub(super) const ART_H: f32 = 132.0;

/// Little shapes floating around a centre point: (dx, dy, radius, colour, ring?).
const BITS: [(f32, f32, f32, u32, bool); 7] = [
    (-78.0, 26.0, 5.0, PURPLE, false),
    (74.0, 34.0, 4.0, BLUE, false),
    (-52.0, -44.0, 3.0, PINK, false),
    (88.0, -18.0, 8.0, BLUE, true),
    (-92.0, -12.0, 6.0, AMBER, true),
    (40.0, -54.0, 2.5, AMBER, false),
    (-24.0, 56.0, 2.5, BLUE, false),
];

/// Sparkles: (dx, dy, size).
const SPARKLES: [(f32, f32, f32); 3] = [(-66.0, -30.0, 16.0), (62.0, -42.0, 13.0), (96.0, 22.0, 11.0)];

/// The decorations around (`x`, `y`), spread by (`sx`, `sy`), faded in by `t` (0..=1) and
/// drifting outwards as they come.
#[allow(clippy::too_many_arguments)]
pub(super) fn decorations(p: &Pen, f: &Fonts, x: f32, y: f32, sx: f32, sy: f32, t: f32, dark: bool) {
    let a = if dark { 0.55 } else { 0.45 } * t;
    let spread = 0.85 + 0.15 * t;
    for (dx, dy, r, rgb, ring) in BITS {
        let (cx, cy) = (x + dx * sx * spread, y + dy * sy * spread);
        if ring {
            p.ring(cx, cy, r, rgb, a, 2.0);
        } else {
            p.dot(cx, cy, r, rgb, a);
        }
    }
    for (k, (dx, dy, s)) in SPARKLES.into_iter().enumerate() {
        let rgb = [PURPLE, BLUE, PINK][k % 3];
        let (cx, cy) = (x + dx * sx * spread, y + dy * sy * spread);
        p.text_alpha("\u{2726}", rect(cx - s, cy - s, 2.0 * s, 2.0 * s), &f.sparkle, rgb, (a * 1.3).min(1.0));
    }
}

/// An illustration centred in `r` (its top `ART_H` DIPs): soft shapes, a floating card with
/// `glyph` (an icon-font character) on it, and the decorations. `t` (0..=1) is how far it has
/// come in.
pub(super) fn illustration(p: &Pen, f: &Fonts, r: D2D_RECT_F, glyph: &str, t: f32, dark: bool) {
    let c = &p.c;
    let (x, y) = ((r.left + r.right) / 2.0, r.top + ART_H / 2.0 + 4.0);
    let t = motion::ease_out(t);
    // Soft colour shapes behind the card.
    p.dot(x - 14.0, y + 4.0, 50.0, PURPLE, if dark { 0.20 } else { 0.12 } * t);
    p.dot(x + 30.0, y - 16.0, 30.0, BLUE, if dark { 0.20 } else { 0.12 } * t);
    decorations(p, f, x, y, 1.0, 1.0, t, dark);
    // The card floats up into place.
    let lift = (1.0 - t) * 10.0;
    let card = rect(x - 38.0, y - 38.0 + lift, 76.0, 76.0);
    p.fill(D2D_RECT_F { top: card.top + 6.0, bottom: card.bottom + 6.0, ..card }, 22.0, 0x000000, if dark { 0.30 } else { 0.08 } * t);
    p.fill(card, 22.0, c.card, t);
    p.outline(card, 22.0, c.line, 1.0);
    p.text_alpha(glyph, card, &f.art, c.brand, t);
}
