//! Colours, icon glyphs and sizes of the native frame.

use windows::Win32::Foundation::COLORREF;
use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;

/// Layout in DIPs.
pub const STRIP_H: f32 = 41.0;
pub const TOOLBAR_H: f32 = 46.0;
pub const FRAME_H: f32 = STRIP_H + TOOLBAR_H;
pub(super) const TAB_H: f32 = 34.0;
pub(super) const TAB_MAX_W: f32 = 240.0;
pub(super) const TAB_MIN_W: f32 = 44.0;
pub(super) const PINNED_W: f32 = 44.0;
pub(super) const CAPTION_W: f32 = 46.0;
/// Tab search button at the start of the tab strip, with its gap to the first tab.
pub(super) const TAB_SEARCH_W: f32 = 34.0;
pub(super) const BUTTON: f32 = 34.0;
pub(super) const OMNI_H: f32 = 34.0;

pub(crate) struct Palette {
    pub(crate) frame: u32,
    pub(crate) toolbar: u32,
    pub(crate) omni: u32,
    pub(crate) text: u32,
    pub(crate) muted: u32,
    pub(crate) hover: (u32, f32),
    pub(crate) brand: u32,
    pub(crate) sep: u32,
    pub(crate) danger: u32,
}

pub(crate) fn palette(dark: bool, incognito: bool) -> Palette {
    let mut p = if dark || incognito {
        Palette { frame: 0x26232d, toolbar: 0x35323c, omni: 0x24222a, text: 0xebe8f2, muted: 0xc7c7c7, hover: (0xffffff, 0.10), brand: 0xa78bfa, sep: 0x444746, danger: 0xf2b8b5 }
    } else {
        Palette { frame: 0xe7e3f1, toolbar: 0xffffff, omni: 0xf1eef8, text: 0x1f1d24, muted: 0x474747, hover: (0x1f1f1f, 0.08), brand: 0x7c3aed, sep: 0xc4c7c5, danger: 0xb3261e }
    };
    if incognito {
        p.toolbar = 0x3a3348;
        p.omni = 0x28222f;
        p.sep = 0x4a4158;
    }
    p
}

/// Badge colour of a site (start page tiles, tab search), picked from its name.
pub(crate) fn tile_color(name: &str) -> u32 {
    const COLORS: [u32; 8] = [0x7c3aed, 0x0369a1, 0xf97316, 0xdb2777, 0xca8a04, 0x2563eb, 0x059669, 0x9333ea];
    COLORS[name.bytes().map(usize::from).sum::<usize>() % COLORS.len()]
}

pub(crate) fn color(rgb: u32, a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F { r: ((rgb >> 16) & 0xff) as f32 / 255.0, g: ((rgb >> 8) & 0xff) as f32 / 255.0, b: (rgb & 0xff) as f32 / 255.0, a }
}

pub(crate) fn colorref(rgb: u32) -> COLORREF {
    COLORREF(((rgb & 0xff) << 16) | (rgb & 0xff00) | ((rgb >> 16) & 0xff))
}

/// Glyphs of Segoe Fluent Icons / Segoe MDL2 Assets.
pub(crate) mod glyph {
    pub const BACK: &str = "\u{E72B}";
    pub const FORWARD: &str = "\u{E72A}";
    pub const RELOAD: &str = "\u{E72C}";
    pub const STOP: &str = "\u{E711}";
    pub const HOME: &str = "\u{E80F}";
    pub const STAR: &str = "\u{E734}";
    pub const STAR_ON: &str = "\u{E735}";
    pub const PUZZLE: &str = "\u{EA86}";
    pub const MORE: &str = "\u{E712}";
    pub const ADD: &str = "\u{E710}";
    pub const DOWN: &str = "\u{E70D}";
    pub const MIN: &str = "\u{E921}";
    pub const MAX: &str = "\u{E922}";
    pub const RESTORE: &str = "\u{E923}";
    pub const CLOSE: &str = "\u{E8BB}";
    pub const LOCK: &str = "\u{E72E}";
    pub const WARN: &str = "\u{E7BA}";
    pub const INFO: &str = "\u{E946}";
    pub const SEARCH: &str = "\u{E721}";
    pub const GLOBE: &str = "\u{E774}";
    pub const VOLUME: &str = "\u{E767}";
    pub const MUTED: &str = "\u{E74F}";
    pub const PRIVATE: &str = "\u{E727}";
    pub const SYNC: &str = "\u{E895}";
}

#[cfg(test)]
mod tests {
    use super::{color, colorref};

    #[test]
    fn colours_convert() {
        let c = color(0x7c3aed, 1.0);
        assert!((c.r - 0x7c as f32 / 255.0).abs() < 1e-6 && (c.b - 0xed as f32 / 255.0).abs() < 1e-6);
        assert_eq!(colorref(0x112233).0, 0x332211);
    }
}
