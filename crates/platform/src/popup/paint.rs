//! Drawing the pop-up list with Direct2D in the frame's colours.

use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Direct2D::Common::{D2D_RECT_F, D2D_SIZE_U};
use windows::Win32::Graphics::Direct2D::{
    ID2D1HwndRenderTarget, ID2D1SolidColorBrush, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_DRAW_TEXT_OPTIONS_CLIP, D2D1_HWND_RENDER_TARGET_PROPERTIES, D2D1_PRESENT_OPTIONS_NONE,
    D2D1_RENDER_TARGET_PROPERTIES, D2D1_ROUNDED_RECT,
};
use windows::Win32::Graphics::DirectWrite::{IDWriteTextFormat, DWRITE_MEASURING_MODE_NATURAL};
use windows::Win32::UI::WindowsAndMessaging::GetClientRect;

use super::{Inner, CLOSE_W, FILTER_H, PADDING};
use crate::frame::layout::rect;
use crate::frame::scale_of;
use crate::frame::style::{color, glyph, palette, tile_color};

/// Icon of a row kind ("" = none).
pub(super) fn icon(kind: &str) -> &'static str {
    match kind {
        "search" | "find" => glyph::SEARCH,
        "history" => "\u{E81C}",
        "bookmark" => glyph::STAR_ON,
        "bookmarks" => "\u{E728}",
        "tab" => "\u{E737}",
        "url" => glyph::GLOBE,
        "newtab" => glyph::ADD,
        "window" => "\u{E8A7}",
        "incognito" => glyph::PRIVATE,
        "passwords" => "\u{E8D7}",
        "download" => "\u{E896}",
        "extension" => glyph::PUZZLE,
        "delete" => "\u{E74D}",
        "zoom" => "\u{E8A3}",
        "print" => "\u{E749}",
        "tools" => "\u{E90F}",
        "settings" => "\u{E713}",
        "help" => "\u{E897}",
        "exit" => "\u{E7E8}",
        "sync" => "\u{E895}",
        "check" => "\u{E73E}",
        "back" => glyph::BACK,
        "reload" => glyph::RELOAD,
        "copy" => "\u{E8C8}",
        "pin" => "\u{E718}",
        "unpin" => "\u{E77A}",
        "mute" => glyph::MUTED,
        "volume" => glyph::VOLUME,
        "group" => "\u{E8FD}",
        "split" => "\u{E8A9}",
        "close" => glyph::CLOSE,
        "account" => "\u{E77B}",
        "devices" => "\u{E772}",
        "lock" => glyph::LOCK,
        "info" => glyph::INFO,
        "cookie" => "\u{E8B7}",
        "shield" => "\u{EA18}",
        "install" => "\u{E8F4}",
        "warn" => glyph::WARN,
        "store" => "\u{E719}",
        "update" => "\u{E777}",
        "color" => "\u{E790}",
        "rename" => "\u{E8AC}",
        _ => "",
    }
}

impl Inner {
    /// Where the list starts (below the filter box when there is one).
    pub(super) fn list_top(&self) -> f32 {
        PADDING + if self.kind == "suggest" || self.filter_hidden() { 0.0 } else { FILTER_H }
    }

    pub(super) fn paint(&mut self) -> windows::core::Result<()> {
        let scale = scale_of(self.hwnd);
        let mut rc = RECT::default();
        // SAFETY: Direct2D drawing on our own window between BeginDraw and EndDraw.
        unsafe {
            GetClientRect(self.hwnd, &mut rc)?;
            let size = D2D_SIZE_U { width: (rc.right - rc.left).max(1) as u32, height: (rc.bottom - rc.top).max(1) as u32 };
            if self.target.is_none() {
                // The pop-up: small, so drawn by the CPU; a graphics device of its own would cost
                // about 14 MB of memory for nothing.
                let props = D2D1_RENDER_TARGET_PROPERTIES { r#type: windows::Win32::Graphics::Direct2D::D2D1_RENDER_TARGET_TYPE_SOFTWARE, dpiX: 96.0 * scale, dpiY: 96.0 * scale, ..Default::default() };
                let hp = D2D1_HWND_RENDER_TARGET_PROPERTIES { hwnd: self.hwnd, pixelSize: size, presentOptions: D2D1_PRESENT_OPTIONS_NONE };
                let t = self.factory.CreateHwndRenderTarget(&props, &hp)?;
                t.SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
                self.brush = Some(t.CreateSolidColorBrush(&color(0, 1.0), None)?);
                self.target = Some(t);
            }
            let Some(t) = self.target.clone() else { return Ok(()) };
            t.Resize(&size)?;
            t.SetDpi(96.0 * scale, 96.0 * scale);
            t.BeginDraw();
            self.draw(&t, size.width as f32 / scale, size.height as f32 / scale);
            if t.EndDraw(None, None).is_err() {
                self.target = None;
            }
        }
        Ok(())
    }

    fn draw(&self, t: &ID2D1HwndRenderTarget, width: f32, height: f32) {
        let p = palette(self.dark, false);
        let (bg, line) = if self.dark { (0x232228, 0x393841) } else { (0xf8f7fb, 0xe3e1ea) };
        let sel = if self.dark { 0x3a3842 } else { 0xebe3fd };
        let Some(brush) = self.brush.clone() else { return };
        let fill = |r: D2D_RECT_F, radius: f32, rgb: u32, a: f32| {
            // SAFETY: drawing calls on a live render target between BeginDraw and EndDraw.
            unsafe {
                brush.SetColor(&color(rgb, a));
                t.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: r, radiusX: radius, radiusY: radius }, &brush);
            }
        };
        // SAFETY: as above.
        unsafe {
            t.Clear(Some(&color(bg, 1.0)));
            // A hairline edge following Windows 11's rounded pop-up corners.
            brush.SetColor(&color(line, 1.0));
            t.DrawRoundedRectangle(&D2D1_ROUNDED_RECT { rect: rect(0.5, 0.5, width - 1.0, height - 1.0), radiusX: 8.0, radiusY: 8.0 }, &brush, 1.0, None);
        }
        if !self.filter_hidden() && self.kind != "suggest" {
            // Tab search's box: a search icon, the native field, its shortcut on the right.
            let fb = rect(PADDING, PADDING, width - 2.0 * PADDING, FILTER_H - 8.0);
            fill(fb, 10.0, if self.dark { 0x2a2930 } else { 0xffffff }, 1.0);
            let icon = if self.input_mode { "\u{E70F}" } else { glyph::SEARCH };
            self.text_at(t, &brush, icon, rect(fb.left + 10.0, fb.top, 26.0, fb.bottom - fb.top), &self.icons, p.muted);
            self.text_at(t, &brush, &self.hint, rect(fb.right - 104.0, fb.top, 96.0, fb.bottom - fb.top), &self.small, p.muted);
            fill(rect(0.0, FILTER_H + PADDING - 1.0, width, 1.0), 0.0, line, 1.0);
        }
        // Text lines up after an icon column when any row of this list has an icon.
        let icon_column = self.rows.iter().any(|r| !icon(&r.kind).is_empty());
        let mut y = self.list_top();
        for (slot, &k) in self.shown.iter().enumerate().skip(self.scroll) {
            let h = self.height_of(k);
            if y + h > height - PADDING + 0.5 {
                break;
            }
            let row = &self.rows[k];
            let r = rect(PADDING, y, width - 2.0 * PADDING, h);
            y += h;
            if self.is_header(k) {
                // Section title; one with an id folds (chevron on the right).
                self.text_at(t, &brush, &row.title, rect(r.left + 16.0, r.top + 6.0, r.right - r.left - 60.0, h - 6.0), &self.bold, p.muted);
                if !row.id.is_empty() {
                    if self.hover == Some(slot) {
                        fill(rect(r.right - 36.0, r.top + 8.0, 28.0, 28.0), 14.0, p.hover.0, p.hover.1);
                    }
                    let chevron = if self.collapsed.contains(&row.id) { "\u{E70D}" } else { "\u{E70E}" };
                    self.text_at(t, &brush, chevron, rect(r.right - 36.0, r.top + 8.0, 28.0, 28.0), &self.icons, p.muted);
                }
                continue;
            }
            if row.kind == "sep" {
                fill(rect(r.left + 10.0, r.top + h / 2.0, r.right - r.left - 20.0, 1.0), 0.0, line, 1.0);
                continue;
            }
            if row.kind == "note" {
                // A panel's plain lines. Those opening it (before any action) form its heading: the
                // first in bold as the title, the next ones smaller and quieter (the site's address),
                // all from the left edge. Later lines are a quiet paragraph under the icon column.
                let heading = self.rows[..k].iter().all(|r| r.kind == "note");
                let left = if heading { r.left + 12.0 } else if icon_column { r.left + 38.0 } else { r.left + 16.0 };
                let (font, tint) = if k == 0 { (&self.bold, p.text) } else { (&self.small, p.muted) };
                self.text_at(t, &brush, &row.title, rect(left, r.top, r.right - left - 8.0, h), font, tint);
                continue;
            }
            if row.kind == "buttons" {
                // Chrome's dialog buttons: the other one outlined, the main one filled.
                let (main, other) = Self::button_rects(r);
                let hovered = self.hover == Some(slot);
                let other_hover = hovered && self.hover_close;
                fill(other, 17.0, if other_hover { p.hover.0 } else { bg }, if other_hover { p.hover.1 } else { 1.0 });
                // SAFETY: drawing call on a live render target between BeginDraw and EndDraw.
                unsafe {
                    brush.SetColor(&color(line, 1.0));
                    t.DrawRoundedRectangle(&D2D1_ROUNDED_RECT { rect: other, radiusX: 17.0, radiusY: 17.0 }, &brush, 1.0, None);
                }
                self.text_at(t, &brush, &row.detail, other, &self.center, p.brand);
                fill(main, 17.0, p.brand, if hovered && !self.hover_close { 0.88 } else { 1.0 });
                self.text_at(t, &brush, &row.title, main, &self.center, 0xffffff);
                continue;
            }
            if row.kind == "zoom" {
                // Chrome's zoom row: label, then − 100% + and full screen, each a round button.
                let (selected, hovered) = (self.selected == Some(slot), self.hover == Some(slot));
                if selected {
                    fill(r, 8.0, sel, 1.0);
                }
                let text_left = if icon_column { r.left + 38.0 } else { r.left + 14.0 };
                self.text_at(t, &brush, icon("zoom"), rect(r.left + 8.0, r.top, 20.0, h), &self.icons, p.muted);
                self.text_at(t, &brush, &row.title, rect(text_left, r.top, 120.0, h), &self.text, p.text);
                let rects = Self::zoom_rects(r);
                let under = if hovered { self.zoom_at(self.mouse_x) } else { None };
                for (n, (b, cmd)) in rects.iter().zip(["zoomout", "zoomreset", "zoomin", "fullscreen"]).enumerate() {
                    if under == Some(cmd) {
                        fill(*b, 14.0, p.hover.0, p.hover.1 * 1.6);
                    }
                    let (s, f, tint) = match n {
                        0 => ("\u{E738}", &self.icons, p.text),
                        1 => (row.detail.as_str(), &self.center, p.text),
                        2 => ("\u{E710}", &self.icons, p.text),
                        _ => ("\u{E740}", &self.icons, p.muted),
                    };
                    self.text_at(t, &brush, s, *b, f, tint);
                }
                // A thin line before full screen, like Chrome's.
                fill(rect(rects[3].left - 7.0, r.top + 8.0, 1.0, h - 16.0), 0.0, line, 1.0);
                continue;
            }
            let (selected, hovered) = (self.selected == Some(slot), self.hover == Some(slot));
            // The keyboard's selection shows at once; the pointer's highlight fades in and out.
            let lit = self.hover_level(slot);
            if selected {
                fill(r, 8.0, sel, 1.0);
            } else if lit > 0.001 {
                fill(r, 8.0, p.hover.0, p.hover.1 * lit);
            }
            let text_left = if !row.sub.is_empty() { r.left + 62.0 } else if icon_column { r.left + 38.0 } else { r.left + 14.0 };
            let right = r.right - if row.close.is_empty() { 8.0 } else { CLOSE_W + 4.0 };
            if row.sub.is_empty() {
                let glyph = icon(&row.kind);
                let tint = if row.kind == "check" { p.brand } else { p.muted };
                self.text_at(t, &brush, glyph, rect(r.left + 8.0, r.top, 20.0, h), &self.icons, tint);
                // Menus: a fixed column for the shortcut (or a submenu's ›), the rest for the label.
                let detail_w = match (row.detail.is_empty(), super::is_menu(&self.kind)) {
                    (true, _) => 0.0,
                    (false, true) => if row.detail.chars().count() <= 1 { 16.0 } else { 104.0 },
                    (false, false) => ((width - text_left) * 0.45).min(260.0),
                };
                self.text_at(t, &brush, &row.title, rect(text_left, r.top, right - text_left - detail_w - 8.0, h), &self.text, p.text);
                if !row.detail.is_empty() {
                    let accent = if self.kind == "suggest" { p.brand } else { p.muted };
                    self.text_at(t, &brush, &row.detail, rect(right - detail_w - 4.0, r.top, detail_w, h), &self.text, accent);
                }
            } else {
                // A site entry: lettered badge, title, then "host • when".
                let host = row.sub.split(" • ").next().unwrap_or("").trim_start_matches("www.");
                let badge = rect(r.left + 12.0, r.top + (h - 36.0) / 2.0, 36.0, 36.0);
                fill(badge, 8.0, if self.dark { 0x2f2e36 } else { 0xefedf4 }, 1.0);
                let tile = rect(badge.left + 8.0, badge.top + 8.0, 20.0, 20.0);
                let glyph = icon(&row.kind);
                if glyph.is_empty() {
                    fill(tile, 5.0, tile_color(host), 1.0);
                    let letter = host.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_else(|| "K".into());
                    self.text_at(t, &brush, &letter, tile, &self.center, 0xffffff);
                } else {
                    self.text_at(t, &brush, glyph, badge, &self.icons, if row.kind == "warn" { p.danger } else { p.muted });
                }
                self.text_at(t, &brush, &row.title, rect(text_left, r.top + 9.0, right - text_left, 20.0), &self.bold, p.text);
                self.text_at(t, &brush, &row.sub, rect(text_left, r.top + 29.0, right - text_left, 18.0), &self.small, p.muted);
            }
            if !row.close.is_empty() && (selected || hovered) {
                let x = rect(r.right - CLOSE_W, r.top + (h - 28.0) / 2.0, 28.0, 28.0);
                if hovered && self.hover_close {
                    fill(x, 14.0, p.hover.0, p.hover.1 * 1.5);
                }
                self.text_at(t, &brush, glyph::CLOSE, x, &self.icons_small, p.text);
            }
        }
    }

    fn text_at(&self, t: &ID2D1HwndRenderTarget, brush: &ID2D1SolidColorBrush, s: &str, r: D2D_RECT_F, f: &IDWriteTextFormat, rgb: u32) {
        let w: Vec<u16> = s.encode_utf16().collect();
        // SAFETY: drawing call on a live render target between BeginDraw and EndDraw.
        unsafe {
            brush.SetColor(&color(rgb, 1.0));
            // An icon-font glyph is drawn as Kakangkung's own line icon where there is one.
            if self.vector.draw_glyph(t, brush, s, r, f.GetFontSize()) {
                return;
            }
            t.DrawText(&w, f, &r, brush, D2D1_DRAW_TEXT_OPTIONS_CLIP, DWRITE_MEASURING_MODE_NATURAL);
        }
    }
}
