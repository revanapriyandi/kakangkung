//! Drawing the frame with Direct2D and DirectWrite.

use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Direct2D::{D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_DRAW_TEXT_OPTIONS_CLIP, D2D1_ELLIPSE, D2D1_HWND_RENDER_TARGET_PROPERTIES, D2D1_PRESENT_OPTIONS_NONE, D2D1_RENDER_TARGET_PROPERTIES, D2D1_ROUNDED_RECT, ID2D1HwndRenderTarget};
use windows::Win32::Graphics::Direct2D::Common::{D2D1_ALPHA_MODE_IGNORE, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_PIXEL_FORMAT, D2D_RECT_F, D2D_SIZE_U};
use windows::Win32::Graphics::Direct2D::D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE;
use windows::Win32::Graphics::Dwm::{DwmExtendFrameIntoClientArea, DwmSetWindowAttribute, DWMWA_USE_IMMERSIVE_DARK_MODE};
use windows::Win32::UI::Controls::MARGINS;
use windows::Win32::Graphics::Direct2D::{D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, D2D1_BITMAP_PROPERTIES};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::DirectWrite::{DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT, DWRITE_MEASURING_MODE_NATURAL, DWRITE_PARAGRAPH_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT, DWRITE_WORD_WRAPPING_NO_WRAP, IDWriteFactory, IDWriteTextFormat};
use windows::Win32::UI::WindowsAndMessaging::{GetClientRect, MoveWindow};
use windows::core::{w, PCWSTR};
use windows_numerics::{Matrix3x2, Vector2};
use windows::Win32::Graphics::Direct2D::{D2D1_ANTIALIAS_MODE_ALIASED, D2D1_ARC_SEGMENT, D2D1_ARC_SIZE_LARGE, D2D1_SWEEP_DIRECTION_CLOCKWISE, ID2D1Factory, ID2D1PathGeometry};
use windows::Win32::Graphics::Direct2D::Common::{D2D_SIZE_F, D2D1_FIGURE_BEGIN_HOLLOW, D2D1_FIGURE_END_OPEN};

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
                // With glass the strip is drawn with transparency (premultiplied alpha).
                let pixel = D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: if self.glass { D2D1_ALPHA_MODE_PREMULTIPLIED } else { D2D1_ALPHA_MODE_IGNORE } };
                let props = D2D1_RENDER_TARGET_PROPERTIES { r#type: windows::Win32::Graphics::Direct2D::D2D1_RENDER_TARGET_TYPE_SOFTWARE, pixelFormat: pixel, dpiX: 96.0 * scale, dpiY: 96.0 * scale, ..Default::default() };
                let hp = D2D1_HWND_RENDER_TARGET_PROPERTIES { hwnd, pixelSize: size, presentOptions: D2D1_PRESENT_OPTIONS_NONE };
                let t = self.gfx.factory.CreateHwndRenderTarget(&props, &hp)?;
                t.SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
                if self.glass {
                    // ClearType needs an opaque background; on glass, greyscale smoothing.
                    t.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
                }
                self.gfx.brush = Some(t.CreateSolidColorBrush(&color(0, 1.0), None)?);
                self.gfx.target = Some(t);
                self.gfx.favicons.clear();
            }
            if self.glass {
                // The glass covers the tab strip; it follows the scale, and its tint the theme.
                let margin = (STRIP_H * scale).ceil() as i32;
                if margin != self.glass_margin {
                    let m = MARGINS { cxLeftWidth: 0, cxRightWidth: 0, cyTopHeight: margin, cyBottomHeight: 0 };
                    if DwmExtendFrameIntoClientArea(self.root, &m).is_ok() {
                        self.glass_margin = margin;
                    }
                }
                let dark = self.state.dark || self.state.incognito;
                if self.glass_dark != Some(dark) {
                    let on = windows::core::BOOL::from(dark);
                    let _ = DwmSetWindowAttribute(self.root, DWMWA_USE_IMMERSIVE_DARK_MODE, &on as *const _ as *const std::ffi::c_void, 4);
                    self.glass_dark = Some(dark);
                }
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
    fn make_favicons(&mut self, t: &ID2D1HwndRenderTarget, keys: Vec<&String>) {
        let size = crate::favicon::SIZE;
        let props = D2D1_BITMAP_PROPERTIES {
            pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
            dpiX: 96.0,
            dpiY: 96.0,
        };
        for key in keys {
            if self.gfx.favicons.contains_key(key) {
                continue;
            }
            let Some(px) = crate::favicon::get(key) else { continue };
            if self.gfx.favicons.len() >= 256 {
                self.gfx.favicons.clear();
            }
            // SAFETY: `px` holds `size` rows of `size * 4` bytes; props is a valid local.
            if let Ok(b) = unsafe { t.CreateBitmap(D2D_SIZE_U { width: size, height: size }, Some(px.as_ptr().cast()), size * 4, &props) } {
                self.gfx.favicons.insert(key.clone(), b);
            }
        }
    }

    pub(super) fn draw(&mut self, t: &ID2D1HwndRenderTarget, width: f32) {
        let s = self.state.clone();
        let p = palette(s.dark, s.incognito);
        let brush = self.gfx.brush.clone().expect("brush created with the target");
        self.make_favicons(t, s.tabs.iter().map(|t| &t.favicon).chain(s.pinned.iter().map(|p| &p.icon)).chain(std::iter::once(&s.avatar_image)).collect());
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
            // An icon: Kakangkung's own line drawing of `g` (size, line weight), else the font glyph.
            let ico = |g: &str, r: D2D_RECT_F, (size, weight): (f32, f32), rgb: u32, a: f32| {
                brush.SetColor(&color(rgb, a));
                if !self.gfx.vector.draw(t, &brush, g, r, size, weight) {
                    let w: Vec<u16> = g.encode_utf16().collect();
                    t.DrawText(&w, if size >= 17.0 { &self.gfx.icons } else { &self.gfx.icons_small }, &r, &brush, D2D1_DRAW_TEXT_OPTIONS_CLIP, DWRITE_MEASURING_MODE_NATURAL);
                }
            };
            // Highlight strength of each part as the hover cross-fades (0 = not hovered).
            let fade = self.fade();
            let lit = |h: Hit| fade.at(h);
            // A hover highlight drawn at `lit` strength (`k` scales the palette's hover tint).
            let glow = |r: D2D_RECT_F, radius: f32, h: Hit, k: f32| {
                let a = lit(h);
                if a > 0.001 {
                    round(r, radius, p.hover.0, (p.hover.1 * k * a).min(1.0));
                }
            };

            // Tab strip
            if self.glass {
                // Windows' glass shows through the strip, lightly tinted with the frame colour
                // (more strongly in incognito, so it stays recognisable).
                t.Clear(Some(&color(0, 0.0)));
                fill(rect(0.0, 0.0, width, STRIP_H), p.frame, if s.incognito { 0.6 } else { 0.22 });
            } else {
                t.Clear(Some(&color(p.frame, 1.0)));
            }
            let widths = self.tab_widths(width);
            let mut x = 8.0f32;
            // Tab search at the start of the strip (Chrome's place for it), level with the tabs.
            if s.show_tab_search {
                // Chrome's ⌄ button: a small rounded square, tinted while tab search is open.
                let r = rect(x, STRIP_H - TAB_H + (TAB_H - 28.0) / 2.0, 28.0, 28.0);
                if s.tab_search_open {
                    round(r, 8.0, p.brand, 0.28);
                } else {
                    round(r, 8.0, p.hover.0, p.hover.1 * (0.7 + 0.9 * lit(Hit::TabSearch)));
                }
                ico(glyph::DOWN, r, SMALL, if s.tab_search_open { p.brand } else { p.text }, 1.0);
                hits.push((r, Hit::TabSearch));
                x += TAB_SEARCH_W;
            }
            // Where each tab sits from the start of the strip (a group's label takes room before
            // its first tab), so the strip can scroll when the tabs do not fit.
            // After the tabs changed they slide from where they were drawn; a tab that was not
            // there follows the one before it.
            let slide = self.slide.as_ref().map(|(from, at)| (from, crate::motion::eased(*at, 0.0, super::SLIDE_MS))).filter(|(_, e)| *e < 1.0);
            let mut place = Vec::with_capacity(s.tabs.len());
            let (mut at, mut last_group, mut drawn_end) = (0.0f32, 0u32, 0.0f32);
            for (tab, &w) in s.tabs.iter().zip(&widths) {
                let label = s.groups.iter().find(|(first, _)| *first == tab.id).map(|(_, g)| g).filter(|g| g.id != last_group);
                if let Some(g) = label {
                    last_group = g.id;
                    at += 70.0;
                }
                let (mut o, mut ow) = (at, w);
                if let Some((from, e)) = slide {
                    match from.iter().find(|f| f.0 == tab.id) {
                        Some(&(_, fo, fw)) => {
                            o = fo + (at - fo) * e;
                            ow = fw + (w - fw) * e;
                        }
                        None => o = drawn_end + if label.is_some() { 70.0 } else { 0.0 },
                    }
                }
                place.push((o, ow, label));
                drawn_end = o + ow;
                at += w;
            }
            self.drawn = s.tabs.iter().zip(&place).map(|(t, &(o, w, _))| (t.id, o, w)).collect();
            // Room up to the new-tab button and the window buttons (as in tab_widths).
            let strip_right = width - CAPTION_W * 3.0 - 76.0;
            let overflow = at > strip_right - x + 0.5;
            let (view_l, view_r) = if overflow { (x + SCROLL_W, strip_right - SCROLL_W) } else { (x, strip_right) };
            let view_w = (view_r - view_l).max(0.0);
            if overflow {
                if std::mem::take(&mut self.reveal) {
                    if let Some(&(o, w, _)) = s.tabs.iter().position(|t| t.id == s.active).and_then(|i| place.get(i)) {
                        self.scroll = self.scroll.min(o).max(o + w - view_w);
                    }
                }
                self.scroll = self.scroll.clamp(0.0, (at - view_w).max(0.0));
            } else {
                self.scroll = 0.0;
            }
            let base = view_l - self.scroll;
            // Parts of tabs scrolled out of view take no clicks.
            let tab_hit = |hits: &mut Vec<(D2D_RECT_F, Hit)>, r: D2D_RECT_F, h: Hit| {
                let r = D2D_RECT_F { left: r.left.max(view_l), right: r.right.min(view_r), ..r };
                if r.right - r.left >= 4.0 {
                    hits.push((r, h));
                }
            };
            let spin_angle = (spin_start().elapsed().as_millis() % 1000) as f32 * 0.36;
            t.PushAxisAlignedClip(&rect(view_l, 0.0, view_w, STRIP_H + 8.0), D2D1_ANTIALIAS_MODE_ALIASED);
            // The selected tab's highlight, drawn first so it can glide under the other tabs
            // when another tab is selected.
            self.active_drawn = s.tabs.iter().position(|t| t.id == s.active).and_then(|i| place.get(i)).map(|&(o, w, _)| {
                let (mut hx, mut hw) = (base + o, w);
                if let Some(((fx, fw), at)) = self.active_slide {
                    let e = crate::motion::eased(at, 0.0, super::SLIDE_MS);
                    hx = fx + (hx - fx) * e;
                    hw = fw + (hw - fw) * e;
                }
                // Rounded on top, flaring into the toolbar at the bottom, with a hairline edge.
                let (fill_shape, edge) = tab_shape(&self.gfx.factory, hx, hw);
                if let Some(g) = fill_shape {
                    brush.SetColor(&color(p.toolbar, 1.0));
                    t.FillGeometry(&g, &brush, None);
                }
                if let Some(g) = edge {
                    let night = s.dark || s.incognito;
                    brush.SetColor(&color(if night { 0xffffff } else { 0x000000 }, if night { 0.09 } else { 0.11 }));
                    t.DrawGeometry(&g, &brush, 1.0, None);
                }
                brush.SetColor(&color(p.brand, 1.0));
                t.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: rect(hx + hw * 0.33, STRIP_H - TAB_H, hw * 0.34, 2.5), radiusX: 1.25, radiusY: 1.25 }, &brush);
                (hx, hw)
            });
            for (tab, &(o, w, label)) in s.tabs.iter().zip(&place) {
                let x = base + o;
                if x + w < view_l || x - 70.0 > view_r {
                    continue;
                }
                if let Some(g) = label {
                    let label = rect(x - 66.0, 11.0, 62.0, 22.0);
                    round(label, 8.0, g.color, 1.0);
                    text(&g.name, rect(label.left + 8.0, label.top, 46.0, 22.0), &self.gfx.text, 0x1f1d24);
                    tab_hit(&mut hits, label, Hit::Group(g.id));
                }
                let r = rect(x, STRIP_H - TAB_H, w, TAB_H);
                let active = tab.id == s.active;
                // Hovering any part of a tab (body, close, sound) lights the whole tab.
                let id = tab.id;
                let tab_lit = fade.level(|h| matches!(h, Hit::Tab(x) | Hit::TabClose(x) | Hit::TabAudio(x) if x == id));
                if !active && tab_lit > 0.001 {
                    round(rect(r.left + 2.0, r.top + 4.0, w - 4.0, TAB_H - 8.0), 8.0, p.hover.0, p.hover.1 * tab_lit);
                }
                if tab.group_color != 0 {
                    fill(rect(r.left + 6.0, STRIP_H - 2.0, w - 12.0, 2.0), tab.group_color, 1.0);
                }
                // A thin divider after an unselected tab, unless the next one is selected; it
                // fades out under the hover highlight.
                let next_active = s.tabs.iter().skip_while(|t| t.id != id).nth(1).is_some_and(|t| t.id == s.active);
                if !active && !next_active && !tab.pinned {
                    fill(rect(r.right - 0.5, r.top + 9.0, 1.0, TAB_H - 18.0), p.sep, 0.7 * (1.0 - tab_lit));
                }
                // An unselected tab's text brightens as it is hovered.
                let fg = if active { p.text } else { crate::motion::mix(p.muted, p.text, tab_lit) };
                let icon_x = if tab.pinned { r.left + (w - 16.0) / 2.0 } else { r.left + 12.0 };
                // The site's icon as soon as it is known, loading or not; a turning ring round
                // it (or alone, before the icon arrives) while the page loads.
                let site = self.gfx.favicons.get(&tab.favicon).filter(|_| !tab.private).cloned();
                let (cx, cy) = (icon_x + 8.0, r.top + TAB_H / 2.0);
                // A narrow selected tab shows only its close button there, like Chrome.
                let show_icon = !(active && !tab.pinned && w < 72.0);
                if !show_icon {
                } else if let Some(b) = &site {
                    let e = if tab.loading { 6.0 } else { 8.0 };
                    t.DrawBitmap(b, Some(&rect(cx - e, cy - e, 2.0 * e, 2.0 * e)), 1.0, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, None);
                } else if !tab.loading {
                    let icon = if tab.private { glyph::PRIVATE } else { glyph::GLOBE };
                    ico(icon, rect(icon_x, r.top, 16.0, TAB_H), (16.0, 1.4), fg, 1.0);
                }
                if tab.loading && show_icon {
                    let radius = if site.is_some() { 9.5 } else { 6.5 };
                    if let Some(arc) = spinner(&self.gfx.factory, &mut self.gfx.spinners, radius) {
                        brush.SetColor(&color(p.brand, 1.0));
                        t.SetTransform(&(Matrix3x2::rotation(spin_angle) * Matrix3x2::translation(cx, cy)));
                        t.DrawGeometry(&arc, &brush, if site.is_some() { 1.5 } else { 2.0 }, None);
                        t.SetTransform(&Matrix3x2::identity());
                    }
                }
                tab_hit(&mut hits, r, Hit::Tab(tab.id));
                if !tab.pinned {
                    let mut title_right = r.right - 8.0;
                    let narrow = w < 72.0;
                    if !narrow || active {
                        let close = rect(r.right - 28.0, r.top + 7.0, 20.0, 20.0);
                        glow(close, 10.0, Hit::TabClose(tab.id), 1.6);
                        ico(glyph::CLOSE, close, (13.0, 1.5), fg, 1.0);
                        tab_hit(&mut hits, close, Hit::TabClose(tab.id));
                        title_right = close.left - 2.0;
                    }
                    if tab.audio || tab.muted {
                        let a = rect(title_right - 20.0, r.top + 7.0, 20.0, 20.0);
                        ico(if tab.muted { glyph::MUTED } else { glyph::VOLUME }, a, (14.0, 1.4), if tab.muted { p.danger } else { fg }, 1.0);
                        tab_hit(&mut hits, a, Hit::TabAudio(tab.id));
                        title_right = a.left;
                    }
                    if !narrow {
                        let title = if tab.title.is_empty() { "New Tab" } else { &tab.title };
                        let prefix = if tab.in_split { "\u{25EB} " } else { "" };
                        text(&format!("{prefix}{title}"), rect(icon_x + 24.0, r.top, (title_right - icon_x - 26.0).max(0.0), TAB_H), &self.gfx.text, fg);
                    }
                } else if tab.audio || tab.muted {
                    let a = rect(r.right - 16.0, r.top + 2.0, 14.0, 14.0);
                    ico(if tab.muted { glyph::MUTED } else { glyph::VOLUME }, a, (12.0, 1.4), p.muted, 1.0);
                }
            }
            t.PopAxisAlignedClip();
            if overflow {
                // Arrows at both ends, dimmed at the end the strip cannot scroll further to.
                for (h, g, r, can) in [
                    (Hit::ScrollLeft, glyph::BACK, rect(view_l - SCROLL_W, STRIP_H - TAB_H + 5.0, SCROLL_W - 4.0, TAB_H - 10.0), self.scroll > 0.5),
                    (Hit::ScrollRight, glyph::FORWARD, rect(view_r + 4.0, STRIP_H - TAB_H + 5.0, SCROLL_W - 4.0, TAB_H - 10.0), self.scroll < at - view_w - 0.5),
                ] {
                    if can {
                        glow(r, 6.0, h, 1.0);
                    }
                    ico(g, r, SMALL, if can { p.text } else { p.sep }, 1.0);
                    hits.push((r, h));
                }
            }
            // The new-tab button follows the last tab as the tabs slide.
            let x = if overflow { view_r + SCROLL_W } else { (base + drawn_end).min(view_r) };
            let plus = rect(x + 6.0, 9.0, 28.0, 28.0);
            glow(plus, 14.0, Hit::NewTab, 1.0);
            ico(glyph::ADD, plus, BIG, p.muted, 1.0);
            hits.push((plus, Hit::NewTab));

            // Window buttons and tab search
            let mut right = width;
            for (h, g) in [(Hit::Close, glyph::CLOSE), (Hit::Max, if s.maximized { glyph::RESTORE } else { glyph::MAX }), (Hit::Min, glyph::MIN)] {
                right -= CAPTION_W;
                let r = rect(right, 0.0, CAPTION_W, STRIP_H - 6.0);
                let a = lit(h);
                if a > 0.001 {
                    if h == Hit::Close { fill(r, 0xc42b1c, a.min(1.0)) } else { fill(r, p.hover.0, p.hover.1 * a) }
                }
                ico(g, r, CAPTION, if h == Hit::Close { crate::motion::mix(p.text, 0xffffff, a.min(1.0)) } else { p.text }, 1.0);
                hits.push((r, h));
            }

            // Toolbar
            fill(rect(0.0, STRIP_H, width, TOOLBAR_H), p.toolbar, 1.0);
            fill(rect(0.0, FRAME_H - 1.0, width, 1.0), p.sep, 1.0);
            let y = STRIP_H + (TOOLBAR_H - BUTTON) / 2.0;
            let button = |hits: &mut Vec<(D2D_RECT_F, Hit)>, x: f32, h: Hit, g: &str, enabled: bool, rgb: u32| {
                let r = rect(x, y, BUTTON, BUTTON);
                let a = if enabled { lit(h) } else { 0.0 };
                if a > 0.001 {
                    brush.SetColor(&color(p.hover.0, p.hover.1 * a));
                    t.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: x + BUTTON / 2.0, Y: y + BUTTON / 2.0 }, radiusX: BUTTON / 2.0, radiusY: BUTTON / 2.0 }, &brush);
                }
                ico(g, r, BIG, rgb, if enabled { 1.0 } else { 0.38 });
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
            glow(av, BUTTON / 2.0, Hit::Profile, 1.0);
            let disc = D2D1_ELLIPSE { point: Vector2 { X: r_edge + BUTTON / 2.0, Y: y + BUTTON / 2.0 }, radiusX: 12.0, radiusY: 12.0 };
            if s.avatar.is_empty() {
                ico("\u{E77B}", av, BIG, p.muted, 1.0);
            } else {
                if let Some(bitmap) = self.gfx.favicons.get(&s.avatar_image) {
                    t.DrawBitmap(bitmap, Some(&rect(disc.point.X - 12.0, disc.point.Y - 12.0, 24.0, 24.0)), 1.0, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, None);
                } else {
                    brush.SetColor(&color(0x7c3aed, 1.0));
                    t.FillEllipse(&disc, &brush);
                    text(&s.avatar, av, &self.gfx.text_bold, 0xffffff);
                }
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
            // Pinned buttons, the last one next to the extensions button, like Chrome.
            for (i, pin) in s.pinned.iter().enumerate().rev() {
                r_edge -= BUTTON + 2.0;
                match self.gfx.favicons.get(&pin.icon) {
                    Some(b) => {
                        let r = rect(r_edge, y, BUTTON, BUTTON);
                        glow(r, BUTTON / 2.0, Hit::Pinned(i), 1.0);
                        let (cx, cy) = (r_edge + BUTTON / 2.0, y + BUTTON / 2.0);
                        t.DrawBitmap(b, Some(&rect(cx - 9.0, cy - 9.0, 18.0, 18.0)), 1.0, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, None);
                        hits.push((r, Hit::Pinned(i)));
                    }
                    None => button(&mut hits, r_edge, Hit::Pinned(i), pin.glyph, true, p.muted),
                }
            }

            // Address bar
            let omni = rect(x + 6.0, STRIP_H + (TOOLBAR_H - OMNI_H) / 2.0, (r_edge - 8.0 - x - 6.0).max(80.0), OMNI_H);
            round(omni, OMNI_H / 2.0, p.omni, 1.0);
            // A brand-coloured ring fades in round the address bar while it is being edited.
            let focus = self.omni_focus_level();
            if focus > 0.001 {
                brush.SetColor(&color(p.brand, focus));
                let ring = rect(omni.left - 1.0, omni.top - 1.0, omni.right - omni.left + 2.0, OMNI_H + 2.0);
                t.DrawRoundedRectangle(&D2D1_ROUNDED_RECT { rect: ring, radiusX: OMNI_H / 2.0 + 1.0, radiusY: OMNI_H / 2.0 + 1.0 }, &brush, 2.0, None);
            }
            self.omni_rect = omni;
            let chip = rect(omni.left + 5.0, omni.top + 5.0, 24.0, 24.0);
            glow(chip, 12.0, Hit::Chip, 1.0);
            let (chip_glyph, chip_color) = match s.chip {
                Chip::Secure => (glyph::LOCK, p.muted),
                Chip::Insecure => (glyph::WARN, p.danger),
                Chip::Local => (glyph::INFO, p.muted),
                Chip::Internal => (glyph::INFO, p.muted),
            };
            ico(chip_glyph, chip, (15.0, 1.5), chip_color, 1.0);
            hits.push((chip, Hit::Chip));
            let mut edit_right = omni.right - 12.0;
            // Buttons inside the pill, from its right end: the star, then the key.
            let mut right = omni.right - 4.0;
            if s.show_star {
                let star = rect(right - 28.0, omni.top + 3.0, 28.0, 28.0);
                glow(star, 14.0, Hit::Star, 1.0);
                ico(if s.bookmarked { glyph::STAR_ON } else { glyph::STAR }, star, BIG, if s.bookmarked { p.brand } else { p.muted }, 1.0);
                hits.push((star, Hit::Star));
                right = star.left;
                edit_right = star.left - 4.0;
            }
            if s.installable {
                let install = rect(right - 28.0, omni.top + 3.0, 28.0, 28.0);
                glow(install, 14.0, Hit::InstallApp, 1.0);
                ico("\u{E8F4}", install, BIG, p.brand, 1.0);
                hits.push((install, Hit::InstallApp));
                right = install.left; edit_right = install.left - 4.0;
            }
            if s.key {
                let key = rect(right - 28.0, omni.top + 3.0, 28.0, 28.0);
                glow(key, 14.0, Hit::Key, 1.0);
                ico(glyph::KEY, key, BIG, p.muted, 1.0);
                hits.push((key, Hit::Key));
                edit_right = key.left - 4.0;
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

/// Icon sizes (DIPs) and line weights: small (tab strip), toolbar, window buttons (thin, like Windows').
const SMALL: (f32, f32) = (14.0, 1.5);
const BIG: (f32, f32) = (18.0, 1.6);
const CAPTION: (f32, f32) = (15.0, 1.1);

/// The selected tab's outline at `x`, `w` wide: rounded on top, flaring outwards into the toolbar
/// at the bottom (Chrome's shape). (Filled shape, its edge without the bottom.)
fn tab_shape(factory: &ID2D1Factory, x: f32, w: f32) -> (Option<ID2D1PathGeometry>, Option<ID2D1PathGeometry>) {
    let (top, bottom, f) = (STRIP_H - TAB_H, STRIP_H + 1.0, 8.0f32);
    let r = 10.0f32.min(w / 2.0);
    let outline = format!(
        "M{} {bottom}A{f} {f} 0 0 0 {x} {}L{x} {}A{r} {r} 0 0 1 {} {top}L{} {top}A{r} {r} 0 0 1 {} {}L{} {}A{f} {f} 0 0 0 {} {bottom}",
        x - f, bottom - f, top + r, x + r, x + w - r, x + w, top + r, x + w, bottom - f, x + w + f
    );
    (crate::icons::path(factory, &format!("{outline}Z"), true).ok(), crate::icons::path(factory, &outline, false).ok())
}

/// Width of the scroll arrows at the ends of an overflowing tab strip.
const SCROLL_W: f32 = 28.0;

/// When the loading spinners started turning (they all turn together).
fn spin_start() -> std::time::Instant {
    static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    *START.get_or_init(std::time::Instant::now)
}

/// Three quarters of a circle of `radius` round the origin, made once per radius.
fn spinner(factory: &ID2D1Factory, cache: &mut Vec<(f32, ID2D1PathGeometry)>, radius: f32) -> Option<ID2D1PathGeometry> {
    if let Some((_, g)) = cache.iter().find(|(r, _)| *r == radius) {
        return Some(g.clone());
    }
    // SAFETY: plain Direct2D geometry building on objects created here.
    let g = unsafe {
        let g = factory.CreatePathGeometry().ok()?;
        let sink = g.Open().ok()?;
        sink.BeginFigure(Vector2 { X: radius, Y: 0.0 }, D2D1_FIGURE_BEGIN_HOLLOW);
        sink.AddArc(&D2D1_ARC_SEGMENT {
            point: Vector2 { X: 0.0, Y: -radius },
            size: D2D_SIZE_F { width: radius, height: radius },
            rotationAngle: 0.0,
            sweepDirection: D2D1_SWEEP_DIRECTION_CLOCKWISE,
            arcSize: D2D1_ARC_SIZE_LARGE,
        });
        sink.EndFigure(D2D1_FIGURE_END_OPEN);
        sink.Close().ok()?;
        g
    };
    cache.push((radius, g.clone()));
    Some(g)
}
