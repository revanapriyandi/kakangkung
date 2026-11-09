//! Kakangkung's own line icons: small vector drawings on a 24-unit grid (SVG path syntax),
//! stroked with round ends, drawn in place of the system icon font. Drawn for Kakangkung.
//! Each one is keyed by the icon-font glyph it replaces, so callers keep passing glyphs and any
//! glyph without a drawing still falls back to the font.

use windows::Win32::Graphics::Direct2D::Common::{D2D1_FIGURE_BEGIN_FILLED, D2D1_FIGURE_BEGIN_HOLLOW, D2D1_FIGURE_END_CLOSED, D2D1_FIGURE_END_OPEN, D2D1_BEZIER_SEGMENT, D2D_RECT_F, D2D_SIZE_F};
use windows::Win32::Graphics::Direct2D::{D2D1_ARC_SEGMENT, D2D1_ARC_SIZE_LARGE, D2D1_ARC_SIZE_SMALL, D2D1_CAP_STYLE_ROUND, D2D1_LINE_JOIN_ROUND, D2D1_STROKE_STYLE_PROPERTIES, D2D1_SWEEP_DIRECTION_CLOCKWISE, D2D1_SWEEP_DIRECTION_COUNTER_CLOCKWISE, ID2D1Brush, ID2D1Factory, ID2D1PathGeometry, ID2D1RenderTarget, ID2D1StrokeStyle};
use windows_numerics::{Matrix3x2, Vector2};

/// (glyph replaced, path on a 24-unit grid, filled instead of stroked)
const ICONS: &[(&str, &str, bool)] = &[
    ("\u{E72B}", "M19 12H5 M11 6l-6 6 6 6", false),                                   // back
    ("\u{E72A}", "M5 12h14 M13 6l6 6-6 6", false),                                    // forward
    ("\u{E72C}", "M20 12a8 8 0 1 1-2.6-5.9 L20 8.5 M20 3.5v5h-5", false),             // reload
    ("\u{E711}", "M6.5 6.5l11 11 M17.5 6.5l-11 11", false),                           // stop
    ("\u{E8BB}", "M6.5 6.5l11 11 M17.5 6.5l-11 11", false),                           // close
    ("\u{E80F}", "M4 11l8-7 8 7 M6 9.5V20h4.5v-5.5h3V20H18V9.5", false),               // home
    ("\u{E734}", "M12 3.8l2.5 5.2 5.7.8-4.1 4 1 5.7L12 16.8l-5.1 2.7 1-5.7-4.1-4 5.7-.8z", false), // star
    ("\u{E735}", "M12 3.8l2.5 5.2 5.7.8-4.1 4 1 5.7L12 16.8l-5.1 2.7 1-5.7-4.1-4 5.7-.8z", true),  // star, set
    ("\u{E8D7}", "M10.5 9.5a4 4 0 1 0 8 0a4 4 0 1 0-8 0 M11.7 12.3L4 20 M7 17l2.2 2.2", false), // key
    ("\u{EA86}", "M5 5h4.5a2 2 0 0 1 4 0H18v4.5a2 2 0 0 1 0 4V19h-4.5a2 2 0 0 0-4 0H5v-5.5a2 2 0 0 0 0-4z", false), // extensions
    ("\u{E712}", "M4.6 12a1.4 1.4 0 1 0 2.8 0a1.4 1.4 0 1 0-2.8 0 M10.6 12a1.4 1.4 0 1 0 2.8 0a1.4 1.4 0 1 0-2.8 0 M16.6 12a1.4 1.4 0 1 0 2.8 0a1.4 1.4 0 1 0-2.8 0", true), // more
    ("\u{E710}", "M12 5v14 M5 12h14", false),                                         // add
    ("\u{E70D}", "M7 10l5 5 5-5", false),                                             // down
    ("\u{E921}", "M6.5 12h11", false),                                                // minimise
    ("\u{E922}", "M8 6.5h8A1.5 1.5 0 0 1 17.5 8v8a1.5 1.5 0 0 1-1.5 1.5H8A1.5 1.5 0 0 1 6.5 16V8A1.5 1.5 0 0 1 8 6.5z", false), // maximise
    ("\u{E923}", "M6.5 9.5h8v8h-8z M9.5 6.5h8v8", false),                             // restore
    ("\u{E72E}", "M7 11h10a1.5 1.5 0 0 1 1.5 1.5v6A1.5 1.5 0 0 1 17 20H7a1.5 1.5 0 0 1-1.5-1.5v-6A1.5 1.5 0 0 1 7 11z M8.5 11V8a3.5 3.5 0 0 1 7 0v3", false), // lock
    ("\u{E7BA}", "M12 4l9 16H3z M12 10v4.5 M12 17.5v.01", false),                     // warning
    ("\u{E946}", "M3.5 12a8.5 8.5 0 1 0 17 0a8.5 8.5 0 1 0-17 0 M12 11v5.5 M12 7.8v.01", false), // info
    ("\u{E721}", "M4 10.5a6.5 6.5 0 1 0 13 0a6.5 6.5 0 1 0-13 0 M15.2 15.2L20 20", false), // search
    ("\u{E774}", "M3.5 12a8.5 8.5 0 1 0 17 0a8.5 8.5 0 1 0-17 0 M3.5 12h17 M12 3.5a13 13 0 0 1 0 17 M12 3.5a13 13 0 0 0 0 17", false), // site
    ("\u{E767}", "M4 9.5h3.5L12 5.5v13l-4.5-4H4z M15.5 9a4 4 0 0 1 0 6 M18 6.5a7.5 7.5 0 0 1 0 11", false), // sound
    ("\u{E74F}", "M4 9.5h3.5L12 5.5v13l-4.5-4H4z M16 9.5l5 5 M21 9.5l-5 5", false),   // muted
    ("\u{E727}", "M3 11h18 M6.5 11c.5-3 1.3-6 2.5-6 1 0 1.6 1 3 1 1.4 0 2-1 3-1 1.2 0 2 3 2.5 6 M4.5 16.5a2.5 2.5 0 1 0 5 0a2.5 2.5 0 1 0-5 0 M14.5 16.5a2.5 2.5 0 1 0 5 0a2.5 2.5 0 1 0-5 0 M9.5 16.5c1.5-.8 3.5-.8 5 0", false), // private
    ("\u{E77B}", "M8 8a4 4 0 1 0 8 0a4 4 0 1 0-8 0 M4.5 20a7.5 7.5 0 0 1 15 0", false), // person
    ("\u{E8F4}", "M4 5h16v11H4z M9 20h6 M12 16v4 M12 7.5v5 M9.8 10.5l2.2 2.2 2.2-2.2", false), // install app
    ("\u{EA18}", "M12 3.5l7 2.5v5.5c0 4.5-3 7.6-7 9-4-1.4-7-4.5-7-9V6z", false),      // ad blocker
    ("\u{E896}", "M12 4v11 M7.5 10.5L12 15l4.5-4.5 M5 19.5h14", false),               // downloads
    ("\u{E81C}", "M4.5 12a7.5 7.5 0 1 0 2.2-5.3 M4 4.5V8h3.5 M12 8v4.5l3 1.8", false), // history
    ("\u{E728}", "M7 4h10v16l-5-3.5L7 20z", false),                                   // bookmarks
    ("\u{E9D9}", "M3 12h4l2.5-6 5 12 2.5-6h4", false),                                // task manager
    // Menus and pages
    ("\u{E737}", "M4 6.5A1.5 1.5 0 0 1 5.5 5h13A1.5 1.5 0 0 1 20 6.5v11a1.5 1.5 0 0 1-1.5 1.5h-13A1.5 1.5 0 0 1 4 17.5z M4 9.5h16 M10 5v4.5", false), // tab
    ("\u{E8A7}", "M4 6.5A1.5 1.5 0 0 1 5.5 5h13A1.5 1.5 0 0 1 20 6.5v11a1.5 1.5 0 0 1-1.5 1.5h-13A1.5 1.5 0 0 1 4 17.5z M4 9h16", false), // window
    ("\u{E74D}", "M4.5 7h15 M9.5 7V5h5v2 M6.5 7l1 12.5h9l1-12.5 M10 10.5v6 M14 10.5v6", false), // delete
    ("\u{E8A3}", "M4 10.5a6.5 6.5 0 1 0 13 0a6.5 6.5 0 1 0-13 0 M15.2 15.2L20 20 M10.5 8v5 M8 10.5h5", false), // zoom
    ("\u{E749}", "M7 9V4h10v5 M7 17H5a1.5 1.5 0 0 1-1.5-1.5v-5A1.5 1.5 0 0 1 5 9h14a1.5 1.5 0 0 1 1.5 1.5v5A1.5 1.5 0 0 1 19 17h-2 M7 14h10v6H7z", false), // print
    ("\u{E90F}", "M4 9h16v10H4z M9 9V6.5h6V9 M4 13h16", false),                        // more tools
    ("\u{E713}", "M4 7h10 M18 7h2 M4 17h2 M10 17h10 M16 5v4 M8 15v4", false),         // settings
    ("\u{E897}", "M3.5 12a8.5 8.5 0 1 0 17 0a8.5 8.5 0 1 0-17 0 M9.5 9.5a2.5 2.5 0 1 1 3.5 2.3c-.6.3-1 .8-1 1.5v.7 M12 17v.01", false), // help
    ("\u{E7E8}", "M12 3.5v8 M7 6.5a7.5 7.5 0 1 0 10 0", false),                        // exit
    ("\u{E895}", "M4.5 12a7.5 7.5 0 0 1 13-5.1 M19.5 12a7.5 7.5 0 0 1-13 5.1 M18 3.5V7h-3.5 M6 20.5V17h3.5", false), // sync
    ("\u{E73E}", "M5 12.5l4.5 4.5L19 7.5", false),                                    // check
    ("\u{E8C8}", "M9 9h10v11H9z M15 9V5H5v11h4", false),                              // copy
    ("\u{E718}", "M9 4h6 M10 4v6l-3 4h10l-3-4V4 M12 14v6", false),                     // pin
    ("\u{E77A}", "M9 4h6 M10 4v6l-3 4h10l-3-4V4 M12 14v6 M4 4l16 16", false),          // unpin
    ("\u{E8FD}", "M4 6h7v5H4z M13 6h7v5h-7z M4 13h7v5H4z M13 13h7v5h-7z", false),      // group
    ("\u{E8A9}", "M4 5h16v14H4z M12 5v14", false),                                    // split view
    ("\u{E772}", "M3 6h12v9H3z M2 18h14 M18 8h4v11h-4z", false),                      // devices
    ("\u{E8B7}", "M3.5 12a8.5 8.5 0 1 0 17 0a8.5 8.5 0 1 0-17 0 M9 9v.01 M14.5 10.5v.01 M10 15v.01 M15 15.5v.01", false), // cookies
    ("\u{E719}", "M5 8h14l-1 12H6z M9 8V6.5a3 3 0 0 1 6 0V8", false),                  // store
    ("\u{E777}", "M12 20V9 M7.5 13.5L12 9l4.5 4.5 M5 4.5h14", false),                  // update
    ("\u{E790}", "M12 3.5a8.5 8.5 0 1 0 0 17c1.2 0 1.8-.8 1.8-1.8 0-1.3-1-1.7-1-2.7 0-1 .8-1.5 1.8-1.5H17a3.5 3.5 0 0 0 3.5-3.5c0-4.2-3.8-7.5-8.5-7.5z M7.5 11.5v.01 M10 8v.01 M14.5 8v.01", false), // colour
    ("\u{E8AC}", "M4 20h4L19 9l-4-4L4 16z M13.5 6.5l4 4", false),                      // rename
    ("\u{E70F}", "M4 20h4L19 9l-4-4L4 16z M13.5 6.5l4 4", false),                      // edit
    ("\u{E70E}", "M7 14l5-5 5 5", false),                                             // up
    ("\u{E738}", "M5 12h14", false),                                                  // minus
    ("\u{E740}", "M4 9V4h5 M15 4h5v5 M20 15v5h-5 M9 20H4v-5", false),                 // full screen
    ("\u{EC4A}", "M4 16a8 8 0 0 1 16 0 M12 16l3.5-4.5 M12 16v.01 M3.5 19.5h17", false), // performance
    ("\u{F2B7}", "M4 6h9 M8.5 4v2 M6 6c.5 3 2.5 5.5 5.5 7 M11 6c-.7 3-2.8 5.6-6 7.5 M13 20l3.5-8 3.5 8 M14.2 17.5h4.6", false), // languages
    ("\u{E776}", "M10.5 4.5a1.5 1.5 0 1 0 3 0a1.5 1.5 0 1 0-3 0 M5 8l7 1.5L19 8 M12 9.5V14 M12 14l-3 6 M12 14l3 6", false), // accessibility
    ("\u{E770}", "M3.5 5h17v11h-17z M9 20h6 M12 16v4", false),                        // system
    ("\u{E838}", "M3.5 7A1.5 1.5 0 0 1 5 5.5h4l2 2h8a1.5 1.5 0 0 1 1.5 1.5v8.5A1.5 1.5 0 0 1 19 19H5a1.5 1.5 0 0 1-1.5-1.5z", false), // folder
    ("\u{E8B5}", "M12 3.5v10 M8 9.5l4 4 4-4 M4 14v4.5A1.5 1.5 0 0 0 5.5 20h13a1.5 1.5 0 0 0 1.5-1.5V14", false), // import
    ("\u{E71D}", "M8.5 6H20 M8.5 12H20 M8.5 18H20 M4.5 6v.01 M4.5 12v.01 M4.5 18v.01", false), // other
    ("\u{E76C}", "M10 7l5 5-5 5", false),                                             // forward chevron
];

#[derive(Clone)]
pub(crate) struct Icons {
    drawings: Vec<(&'static str, ID2D1PathGeometry, bool)>,
    round: Option<ID2D1StrokeStyle>,
}

impl Icons {
    /// Every drawing, made once (factory resources: they survive a lost graphics device).
    pub(crate) fn new(factory: &ID2D1Factory) -> Self {
        let drawings = ICONS.iter().filter_map(|&(glyph, d, filled)| path(factory, d, filled).ok().map(|g| (glyph, g, filled))).collect();
        let props = D2D1_STROKE_STYLE_PROPERTIES { startCap: D2D1_CAP_STYLE_ROUND, endCap: D2D1_CAP_STYLE_ROUND, dashCap: D2D1_CAP_STYLE_ROUND, lineJoin: D2D1_LINE_JOIN_ROUND, miterLimit: 10.0, ..Default::default() };
        // SAFETY: plain factory call with a valid local.
        let round = unsafe { factory.CreateStrokeStyle(&props, None) }.ok();
        Icons { drawings, round }
    }

    /// Draw text `s` as its line drawing when it is one icon-font glyph that has one, sized like
    /// the font's `font_size` (lines thicken with size); false otherwise (draw it as text).
    pub(crate) fn draw_glyph(&self, t: &ID2D1RenderTarget, brush: &ID2D1Brush, s: &str, r: D2D_RECT_F, font_size: f32) -> bool {
        let mut chars = s.chars();
        let (Some(c), None) = (chars.next(), chars.next()) else { return false };
        if !('\u{E000}'..='\u{F8FF}').contains(&c) {
            return false;
        }
        let size = font_size * 1.15;
        self.draw(t, brush, s, r, size, (size / 11.0).clamp(1.4, 3.2))
    }

    /// Draw `glyph`'s drawing `size` DIPs tall in the middle of `r`, with lines `weight` DIPs
    /// thick. False when it has no drawing (draw the font glyph instead).
    pub(crate) fn draw(&self, t: &ID2D1RenderTarget, brush: &ID2D1Brush, glyph: &str, r: D2D_RECT_F, size: f32, weight: f32) -> bool {
        let Some((_, g, filled)) = self.drawings.iter().find(|(k, _, _)| *k == glyph) else { return false };
        let k = size / 24.0;
        let (cx, cy) = ((r.left + r.right) / 2.0, (r.top + r.bottom) / 2.0);
        // SAFETY: drawing calls on a live render target between BeginDraw and EndDraw.
        unsafe {
            let mut before = Matrix3x2::default();
            t.GetTransform(&mut before);
            t.SetTransform(&(Matrix3x2::scale(k, k) * Matrix3x2::translation(cx - 12.0 * k, cy - 12.0 * k) * before));
            if *filled {
                t.FillGeometry(g, brush, None);
            } else {
                t.DrawGeometry(g, brush, weight / k, self.round.as_ref());
            }
            t.SetTransform(&before);
        }
        true
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Tok {
    Cmd(char),
    Num(f32),
}

/// Commands and numbers of SVG path data ("M5 12h14", "l2.6 5.3 5.9.9", "a2 2 0 0 1-4 0").
fn tokens(d: &str) -> Vec<Tok> {
    let mut out = vec![];
    let mut num = String::new();
    let flush = |num: &mut String, out: &mut Vec<Tok>| {
        if let Ok(n) = num.parse::<f32>() {
            out.push(Tok::Num(n));
        }
        num.clear();
    };
    for c in d.chars() {
        match c {
            '0'..='9' => num.push(c),
            '.' => {
                if num.contains('.') {
                    flush(&mut num, &mut out);
                }
                num.push('.');
            }
            '-' => {
                flush(&mut num, &mut out);
                num.push('-');
            }
            c if c.is_ascii_alphabetic() => {
                flush(&mut num, &mut out);
                out.push(Tok::Cmd(c));
            }
            _ => flush(&mut num, &mut out),
        }
    }
    flush(&mut num, &mut out);
    out
}

/// The number at `i` (0 when there is none), moving past it.
fn num(toks: &[Tok], i: &mut usize) -> f32 {
    match toks.get(*i) {
        Some(Tok::Num(v)) => {
            *i += 1;
            *v
        }
        _ => 0.0,
    }
}

/// A path geometry from SVG path data (M, L, H, V, C, A, Z; absolute or relative).
pub(crate) fn path(factory: &ID2D1Factory, d: &str, filled: bool) -> windows::core::Result<ID2D1PathGeometry> {
    let toks = tokens(d);
    // SAFETY: plain Direct2D geometry building on objects created here.
    unsafe {
        let g = factory.CreatePathGeometry()?;
        let sink = g.Open()?;
        let begin = if filled { D2D1_FIGURE_BEGIN_FILLED } else { D2D1_FIGURE_BEGIN_HOLLOW };
        let (mut i, mut cmd, mut open) = (0usize, ' ', false);
        let (mut x, mut y, mut sx, mut sy) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
        while i < toks.len() {
            if let Tok::Cmd(c) = toks[i] {
                i += 1;
                cmd = c;
                if c == 'z' || c == 'Z' {
                    if open {
                        sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                        open = false;
                    }
                    (x, y) = (sx, sy);
                    continue;
                }
            }
            if cmd == ' ' || !matches!(toks.get(i), Some(Tok::Num(_))) {
                // Numbers before any command, or a command without its numbers: skip.
                i += usize::from(cmd == ' ');
                continue;
            }
            macro_rules! n {
                () => {
                    num(&toks, &mut i)
                };
            }
            let rel = cmd.is_ascii_lowercase();
            let (ox, oy) = if rel { (x, y) } else { (0.0, 0.0) };
            let pt = |a: f32, b: f32| Vector2 { X: a, Y: b };
            match cmd.to_ascii_uppercase() {
                'M' => {
                    let (a, b) = (n!() + ox, n!() + oy);
                    if open {
                        sink.EndFigure(D2D1_FIGURE_END_OPEN);
                    }
                    sink.BeginFigure(pt(a, b), begin);
                    open = true;
                    (x, y, sx, sy) = (a, b, a, b);
                    // Further pairs after a move are lines.
                    cmd = if rel { 'l' } else { 'L' };
                }
                'L' => {
                    (x, y) = (n!() + ox, n!() + oy);
                    sink.AddLine(pt(x, y));
                }
                'H' => {
                    x = n!() + ox;
                    sink.AddLine(pt(x, y));
                }
                'V' => {
                    y = n!() + oy;
                    sink.AddLine(pt(x, y));
                }
                'C' => {
                    let (a1, b1, a2, b2, a3, b3) = (n!() + ox, n!() + oy, n!() + ox, n!() + oy, n!() + ox, n!() + oy);
                    sink.AddBezier(&D2D1_BEZIER_SEGMENT { point1: pt(a1, b1), point2: pt(a2, b2), point3: pt(a3, b3) });
                    (x, y) = (a3, b3);
                }
                'A' => {
                    let (rx, ry, rot, large, sweep) = (n!(), n!(), n!(), n!(), n!());
                    (x, y) = (n!() + ox, n!() + oy);
                    sink.AddArc(&D2D1_ARC_SEGMENT {
                        point: pt(x, y),
                        size: D2D_SIZE_F { width: rx, height: ry },
                        rotationAngle: rot,
                        sweepDirection: if sweep != 0.0 { D2D1_SWEEP_DIRECTION_CLOCKWISE } else { D2D1_SWEEP_DIRECTION_COUNTER_CLOCKWISE },
                        arcSize: if large != 0.0 { D2D1_ARC_SIZE_LARGE } else { D2D1_ARC_SIZE_SMALL },
                    });
                }
                _ => {
                    // Not drawn by these icons: drop its numbers.
                    while matches!(toks.get(i), Some(Tok::Num(_))) {
                        i += 1;
                    }
                }
            }
        }
        if open {
            sink.EndFigure(if filled { D2D1_FIGURE_END_CLOSED } else { D2D1_FIGURE_END_OPEN });
        }
        sink.Close()?;
        Ok(g)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Graphics::Direct2D::{D2D1CreateFactory, D2D1_FACTORY_TYPE_SINGLE_THREADED};

    #[test]
    fn path_numbers_split_like_svg() {
        use Tok::*;
        assert_eq!(tokens("l2.6 5.3 5.9.9-4.1 4"), vec![Cmd('l'), Num(2.6), Num(5.3), Num(5.9), Num(0.9), Num(-4.1), Num(4.0)]);
        assert_eq!(tokens("M12 17.5v.01"), vec![Cmd('M'), Num(12.0), Num(17.5), Cmd('v'), Num(0.01)]);
    }

    #[test]
    fn every_icon_builds_and_has_a_unique_glyph() {
        // SAFETY: plain factory creation.
        let factory: ID2D1Factory = unsafe { D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None) }.expect("factory");
        let icons = Icons::new(&factory);
        assert_eq!(icons.drawings.len(), ICONS.len(), "every drawing builds");
        for (k, (glyph, _, _)) in ICONS.iter().enumerate() {
            assert!(!ICONS[..k].iter().any(|(g, _, _)| g == glyph), "{glyph:?} drawn twice");
        }
        // SAFETY: bounds of a geometry made above (line figures have bounds only as stroked).
        let star = unsafe { icons.drawings.iter().find(|d| d.0 == "\u{E734}").unwrap().1.GetWidenedBounds(1.0, None, None, 0.25) }.expect("bounds");
        assert!(star.left > 2.0 && star.right < 22.0 && star.top > 2.0 && star.bottom < 22.0, "star fits the grid: {star:?}");
    }
}
