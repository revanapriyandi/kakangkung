//! Drawing the frame with Direct2D and DirectWrite.

use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Direct2D::{D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_DRAW_TEXT_OPTIONS_CLIP, D2D1_ELLIPSE, D2D1_HWND_RENDER_TARGET_PROPERTIES, D2D1_PRESENT_OPTIONS_NONE, D2D1_RENDER_TARGET_PROPERTIES, D2D1_ROUNDED_RECT, ID2D1HwndRenderTarget};
use windows::Win32::Graphics::Direct2D::Common::{D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_PIXEL_FORMAT, D2D_RECT_F, D2D_SIZE_U};
use windows::Win32::Graphics::Direct2D::{D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, D2D1_BITMAP_PROPERTIES};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::DirectWrite::{DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT, DWRITE_MEASURING_MODE_NATURAL, DWRITE_PARAGRAPH_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT, DWRITE_WORD_WRAPPING_NO_WRAP, IDWriteFactory, IDWriteTextFormat};
use windows::Win32::UI::WindowsAndMessaging::{GetClientRect, MoveWindow};
use windows::core::{w, PCWSTR};
use windows_numerics::Vector2;

use super::style::{BUTTON, CAPTION_W, TAB_SEARCH_W, FRAME_H, OMNI_H, PINNED_W, STRIP_H, TAB_H, TAB_MAX_W, TAB_MIN_W, TOOLBAR_H, color, glyph, palette};
use super::layout::{rect, Hit};
use super::input::scale_of;
use super::{Chip, Inner};

pub(crate) fn text_format(dw: &IDWriteFactory, family: PCWSTR, size: f32, weight: DWRITE_FONT_WEIGHT, align: DWRITE_TEXT_ALIGNMENT) -> windows::core::Result<IDWriteTextFormat> {
    // SAFETY: plain DirectWrite factory calls with valid arguments.
    unsafe {
        let f = dw.CreateTextFormat(family, None, weight, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_STRETCH_NORMAL, size, w!("en-us"))?;
        f.SetTextAlignment(align)?;
        f.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
        f.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
        Ok(f)
    }
}

/// Windows 11's icon font, else Windows 10's.
pub(crate) fn icon_family(dw: &IDWriteFactory) -> PCWSTR {
    // SAFETY: plain DirectWrite calls; the out-params are valid locals.
    unsafe {
        let mut fonts = None;
        if dw.GetSystemFontCollection(&mut fonts, false).is_ok() {
            if let Some(fonts) = fonts {
                let (mut index, mut exists) = (0u32, windows::core::BOOL(0));
                if fonts.FindFamilyName(w!("Segoe Fluent Icons"), &mut index, &mut exists).is_ok() && exists.as_bool() {
                    return w!("Segoe Fluent Icons");
                }
            }
        }
    }
    w!("Segoe MDL2 Assets")
}

impl Inner {
    /// Width of each tab in the strip.
    pub(super) fn tab_widths(&self, width: f32) -> Vec<f32> {
        let s = &self.state;
        let right = CAPTION_W * 3.0 + 40.0 + 36.0 + if s.show_tab_search { TAB_SEARCH_W } else { 0.0 };
        let pinned = s.tabs.iter().filter(|t| t.pinned).count() as f32;
        let labels = s.groups.len() as f32 * 70.0;
        let normal = s.tabs.iter().filter(|t| !t.pinned).count().max(1) as f32;
        let room = (width - 8.0 - right - pinned * PINNED_W - labels).max(0.0);
        let w = (room / normal).clamp(TAB_MIN_W, TAB_MAX_W);
        // A tab opened a moment ago is still widening.
        let grow = |id: u64| self.appear.iter().find(|(a, _)| *a == id).map_or(1.0, |(_, at)| crate::motion::eased(*at, 0.0, super::TAB_GROW_MS));
        s.tabs.iter().map(|t| if t.pinned { PINNED_W } else { (w * grow(t.id)).max(1.0) }).collect()
    }

    pub(super) fn paint(&mut self) -> windows::core::Result<()> {
        let hwnd = self.hwnd;
        let scale = scale_of(hwnd);
        let mut rc = RECT::default();
        // SAFETY: plain queries and Direct2D drawing on our own window between Begin/EndDraw.
        unsafe {
            GetClientRect(hwnd, &mut rc)?;
            let size = D2D_SIZE_U { width: (rc.right - rc.left).max(1) as u32, height: (rc.bottom - rc.top).max(1) as u32 };
            if self.gfx.target.is_none() {
                // The tab strip and toolbar: small, so drawn by the CPU; a graphics device of its own would cost
                // about 14 MB of memory for nothing.
                let props = D2D1_RENDER_TARGET_PROPERTIES { r#type: windows::Win32::Graphics::Direct2D::D2D1_RENDER_TARGET_TYPE_SOFTWARE, dpiX: 96.0 * scale, dpiY: 96.0 * scale, ..Default::default() };
                let hp = D2D1_HWND_RENDER_TARGET_PROPERTIES { hwnd, pixelSize: size, presentOptions: D2D1_PRESENT_OPTIONS_NONE };
                let t = self.gfx.factory.CreateHwndRenderTarget(&props, &hp)?;
                t.SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
                self.gfx.brush = Some(t.CreateSolidColorBrush(&color(0, 1.0), None)?);
                self.gfx.target = Some(t);
                self.gfx.favicons.clear();
            }
            let t = self.gfx.target.clone().expect("render target created above");
            t.Resize(&size)?;
            t.SetDpi(96.0 * scale, 96.0 * scale);
            let width = size.width as f32 / scale;
            t.BeginDraw();
            self.draw(&t, width);
            if t.EndDraw(None, None).is_err() {
                // Device lost: recreate the target on the next paint.
                self.gfx.target = None;
            }
        }
        Ok(())
    }

    /// Direct2D bitmaps of the tabs' site icons that are decoded but not made yet.
    fn make_favicons(&mut self, t: &ID2D1HwndRenderTarget, tabs: &[super::FrameTab]) {
        let size = crate::favicon::SIZE;
        let props = D2D1_BITMAP_PROPERTIES {
            pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
            dpiX: 96.0,
            dpiY: 96.0,
        };
        for tab in tabs {
            if self.gfx.favicons.contains_key(&tab.favicon) {
                continue;
            }
            let Some(px) = crate::favicon::get(&tab.favicon) else { continue };
            if self.gfx.favicons.len() >= 64 {
                self.gfx.favicons.clear();
            }
            // SAFETY: `px` holds `size` rows of `size * 4` bytes; props is a valid local.
            if let Ok(b) = unsafe { t.CreateBitmap(D2D_SIZE_U { width: size, height: size }, Some(px.as_ptr().cast()), size * 4, &props) } {
                self.gfx.favicons.insert(tab.favicon.clone(), b);
            }
        }
    }

    pub(super) fn draw(&mut self, t: &ID2D1HwndRenderTarget, width: f32) {
        let s = self.state.clone();
        let p = palette(s.dark, s.incognito);
        let brush = self.gfx.brush.clone().expect("brush created with the target");
        self.make_favicons(t, &s.tabs);
        let mut hits: Vec<(D2D_RECT_F, Hit)> = vec![];
        // SAFETY: drawing calls on a live render target between BeginDraw and EndDraw.
        unsafe {
            let fill = |r: D2D_RECT_F, rgb: u32, a: f32| {
                brush.SetColor(&color(rgb, a));
                t.FillRectangle(&r, &brush);
            };
            let round = |r: D2D_RECT_F, radius: f32, rgb: u32, a: f32| {
                brush.SetColor(&color(rgb, a));
                t.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: r, radiusX: radius, radiusY: radius }, &brush);
            };
            let text = |s: &str, r: D2D_RECT_F, format: &IDWriteTextFormat, rgb: u32| {
                brush.SetColor(&color(rgb, 1.0));
                let w: Vec<u16> = s.encode_utf16().collect();
                t.DrawText(&w, format, &r, &brush, D2D1_DRAW_TEXT_OPTIONS_CLIP, DWRITE_MEASURING_MODE_NATURAL);
            };
            let hover = self.hover;
            let hovered = |h: Hit| hover == h;

            // Tab strip
            t.Clear(Some(&color(p.frame, 1.0)));
            let widths = self.tab_widths(width);
            let mut x = 8.0f32;
            // Tab search at the start of the strip (Chrome's place for it), level with the tabs.
            if s.show_tab_search {
                // Chrome's ⌄ button: a small rounded square, tinted while tab search is open.
                let r = rect(x, STRIP_H - TAB_H + (TAB_H - 28.0) / 2.0, 28.0, 28.0);
                if s.tab_search_open {
                    round(r, 8.0, p.brand, 0.28);
                } else {
                    round(r, 8.0, p.hover.0, if hovered(Hit::TabSearch) { p.hover.1 * 1.6 } else { p.hover.1 * 0.7 });
                }
                text(glyph::DOWN, r, &self.gfx.icons_small, if s.tab_search_open { p.brand } else { p.text });
                hits.push((r, Hit::TabSearch));
                x += TAB_SEARCH_W;
            }
            let mut last_group = 0u32;
            for (tab, w) in s.tabs.iter().zip(widths) {
                if let Some((_, g)) = s.groups.iter().find(|(first, _)| *first == tab.id) {
                    if g.id != last_group {
                        let label = rect(x + 4.0, 11.0, 62.0, 22.0);
                        round(label, 8.0, g.color, 1.0);
                        text(&g.name, rect(label.left + 8.0, label.top, 46.0, 22.0), &self.gfx.text, 0x1f1d24);
                        hits.push((label, Hit::Group(g.id)));
                        x += 70.0;
                        last_group = g.id;
                    }
                }
                let r = rect(x, STRIP_H - TAB_H, w, TAB_H);
                let active = tab.id == s.active;
                if active {
                    round(rect(r.left, r.top, r.right - r.left, TAB_H + 8.0), 8.0, p.toolbar, 1.0);
                    brush.SetColor(&color(p.brand, 1.0));
                    let mid = (r.left + r.right) / 2.0;
                    t.FillRectangle(&rect(mid - w * 0.17, r.top, w * 0.34, 2.0), &brush);
                } else if hovered(Hit::Tab(tab.id)) || hovered(Hit::TabClose(tab.id)) {
                    round(rect(r.left + 2.0, r.top + 4.0, w - 4.0, TAB_H - 8.0), 8.0, p.hover.0, p.hover.1);
                }
                if tab.group_color != 0 {
                    fill(rect(r.left + 6.0, STRIP_H - 2.0, w - 12.0, 2.0), tab.group_color, 1.0);
                }
                let fg = if active { p.text } else { p.muted };
                let icon_x = if tab.pinned { r.left + (w - 16.0) / 2.0 } else { r.left + 12.0 };
                let site = self.gfx.favicons.get(&tab.favicon).filter(|_| !tab.loading && !tab.private);
                if let Some(b) = site {
                    t.DrawBitmap(b, Some(&rect(icon_x, r.top + (TAB_H - 16.0) / 2.0, 16.0, 16.0)), 1.0, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, None);
                } else {
                    let icon = if tab.loading { glyph::SYNC } else if tab.private { glyph::PRIVATE } else { glyph::GLOBE };
                    text(icon, rect(icon_x, r.top, 16.0, TAB_H), &self.gfx.icons_small, if tab.loading { p.brand } else { fg });
                }
                hits.push((r, Hit::Tab(tab.id)));
                if !tab.pinned {
                    let mut title_right = r.right - 8.0;
                    let narrow = w < 72.0;
                    if !narrow || active {
                        let close = rect(r.right - 28.0, r.top + 7.0, 20.0, 20.0);
                        if hovered(Hit::TabClose(tab.id)) {
                            round(close, 10.0, p.hover.0, p.hover.1 * 1.6);
                        }
                        text(glyph::CLOSE, close, &self.gfx.icons_small, fg);
                        hits.push((close, Hit::TabClose(tab.id)));
                        title_right = close.left - 2.0;
                    }
                    if tab.audio || tab.muted {
                        let a = rect(title_right - 20.0, r.top + 7.0, 20.0, 20.0);
                        text(if tab.muted { glyph::MUTED } else { glyph::VOLUME }, a, &self.gfx.icons_small, if tab.muted { p.danger } else { fg });
                        hits.push((a, Hit::TabAudio(tab.id)));
                        title_right = a.left;
                    }
                    if !narrow {
                        let title = if tab.title.is_empty() { "New Tab" } else { &tab.title };
                        let prefix = if tab.in_split { "\u{25EB} " } else { "" };
                        text(&format!("{prefix}{title}"), rect(icon_x + 24.0, r.top, (title_right - icon_x - 26.0).max(0.0), TAB_H), &self.gfx.text, fg);
                    }
                } else if tab.audio || tab.muted {
                    let a = rect(r.right - 16.0, r.top + 2.0, 14.0, 14.0);
                    text(if tab.muted { glyph::MUTED } else { glyph::VOLUME }, a, &self.gfx.icons_small, p.muted);
                }
                x += w;
            }
            let plus = rect(x + 6.0, 9.0, 28.0, 28.0);
            if hovered(Hit::NewTab) {
                round(plus, 14.0, p.hover.0, p.hover.1);
            }
            text(glyph::ADD, plus, &self.gfx.icons, p.muted);
            hits.push((plus, Hit::NewTab));

            // Window buttons and tab search
            let mut right = width;
            for (h, g) in [(Hit::Close, glyph::CLOSE), (Hit::Max, if s.maximized { glyph::RESTORE } else { glyph::MAX }), (Hit::Min, glyph::MIN)] {
                right -= CAPTION_W;
                let r = rect(right, 0.0, CAPTION_W, STRIP_H - 6.0);
                if hovered(h) {
                    if h == Hit::Close { fill(r, 0xc42b1c, 1.0) } else { fill(r, p.hover.0, p.hover.1) }
                }
                text(g, r, &self.gfx.icons_small, if hovered(h) && h == Hit::Close { 0xffffff } else { p.text });
                hits.push((r, h));
            }

            // Toolbar
            fill(rect(0.0, STRIP_H, width, TOOLBAR_H), p.toolbar, 1.0);
            fill(rect(0.0, FRAME_H - 1.0, width, 1.0), p.sep, 1.0);
            let y = STRIP_H + (TOOLBAR_H - BUTTON) / 2.0;
            let button = |hits: &mut Vec<(D2D_RECT_F, Hit)>, x: f32, h: Hit, g: &str, enabled: bool, rgb: u32| {
                let r = rect(x, y, BUTTON, BUTTON);
                if enabled && hovered(h) {
                    brush.SetColor(&color(p.hover.0, p.hover.1));
                    t.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: x + BUTTON / 2.0, Y: y + BUTTON / 2.0 }, radiusX: BUTTON / 2.0, radiusY: BUTTON / 2.0 }, &brush);
                }
                brush.SetColor(&color(rgb, if enabled { 1.0 } else { 0.38 }));
                let w: Vec<u16> = g.encode_utf16().collect();
                t.DrawText(&w, &self.gfx.icons, &r, &brush, D2D1_DRAW_TEXT_OPTIONS_CLIP, DWRITE_MEASURING_MODE_NATURAL);
                if enabled {
                    hits.push((r, h));
                }
            };
            let mut x = 6.0;
            button(&mut hits, x, Hit::Back, glyph::BACK, s.can_back, p.muted);
            x += BUTTON + 2.0;
            button(&mut hits, x, Hit::Forward, glyph::FORWARD, s.can_forward, p.muted);
            x += BUTTON + 2.0;
            button(&mut hits, x, Hit::Reload, if s.loading { glyph::STOP } else { glyph::RELOAD }, true, p.muted);
            x += BUTTON + 2.0;
            if s.show_home {
                button(&mut hits, x, Hit::Home, glyph::HOME, true, p.muted);
                x += BUTTON + 2.0;
            }
            let mut r_edge = width - 6.0;
            r_edge -= BUTTON;
            button(&mut hits, r_edge, Hit::Menu, glyph::MORE, true, p.muted);
            r_edge -= BUTTON + 2.0;
            // Avatar: account initial in a brand disc, or a person outline.
            let av = rect(r_edge, y, BUTTON, BUTTON);
            if hovered(Hit::Profile) {
                round(av, BUTTON / 2.0, p.hover.0, p.hover.1);
            }
            let disc = D2D1_ELLIPSE { point: Vector2 { X: r_edge + BUTTON / 2.0, Y: y + BUTTON / 2.0 }, radiusX: 12.0, radiusY: 12.0 };
            if s.avatar.is_empty() {
                text("\u{E77B}", av, &self.gfx.icons, p.muted);
            } else {
                brush.SetColor(&color(0x7c3aed, 1.0));
                t.FillEllipse(&disc, &brush);
                text(&s.avatar, av, &self.gfx.text_bold, 0xffffff);
                if s.synced {
                    brush.SetColor(&color(0x22c55e, 1.0));
                    t.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: r_edge + BUTTON - 7.0, Y: y + BUTTON - 7.0 }, radiusX: 4.0, radiusY: 4.0 }, &brush);
                }
            }
            hits.push((av, Hit::Profile));
            if s.show_extensions {
                r_edge -= BUTTON + 2.0;
                button(&mut hits, r_edge, Hit::Extensions, glyph::PUZZLE, true, p.muted);
            }

            // Address bar
            let omni = rect(x + 6.0, STRIP_H + (TOOLBAR_H - OMNI_H) / 2.0, (r_edge - 8.0 - x - 6.0).max(80.0), OMNI_H);
            round(omni, OMNI_H / 2.0, p.omni, 1.0);
            self.omni_rect = omni;
            let chip = rect(omni.left + 5.0, omni.top + 5.0, 24.0, 24.0);
            if hovered(Hit::Chip) {
                round(chip, 12.0, p.hover.0, p.hover.1);
            }
            let (chip_glyph, chip_color) = match s.chip {
                Chip::Secure => (glyph::LOCK, p.muted),
                Chip::Insecure => (glyph::WARN, p.danger),
                Chip::Local => (glyph::INFO, p.muted),
                Chip::Internal => (glyph::SEARCH, p.muted),
            };
            text(chip_glyph, chip, &self.gfx.icons_small, chip_color);
            hits.push((chip, Hit::Chip));
            let mut edit_right = omni.right - 12.0;
            if s.show_star {
                let star = rect(omni.right - 32.0, omni.top + 3.0, 28.0, 28.0);
                if hovered(Hit::Star) {
                    round(star, 14.0, p.hover.0, p.hover.1);
                }
                text(if s.bookmarked { glyph::STAR_ON } else { glyph::STAR }, star, &self.gfx.icons, if s.bookmarked { p.brand } else { p.muted });
                hits.push((star, Hit::Star));
                edit_right = star.left - 4.0;
            }
            // The EDIT control sits inside the pill, vertically centred.
            let scale = scale_of(self.hwnd);
            let line = 20.0;
            let ex = chip.right + 8.0;
            let ey = omni.top + (OMNI_H - line) / 2.0;
            let _ = MoveWindow(self.edit, (ex * scale) as i32, (ey * scale) as i32, ((edit_right - ex) * scale) as i32, (line * scale) as i32, true);
        }
        self.hits = hits;
    }
}
