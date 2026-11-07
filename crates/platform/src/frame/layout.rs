//! What is where in the frame: the clickable areas found by the last paint.

use windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F;


#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum Hit {
    None,
    Drag,
    Tab(u64),
    TabClose(u64),
    TabAudio(u64),
    Group(u32),
    NewTab,
    TabSearch,
    Min,
    Max,
    Close,
    Back,
    Forward,
    Reload,
    Home,
    Chip,
    Star,
    Extensions,
    Profile,
    Menu,
}

pub(crate) fn rect(x: f32, y: f32, w: f32, h: f32) -> D2D_RECT_F {
    D2D_RECT_F { left: x, top: y, right: x + w, bottom: y + h }
}

pub(crate) fn inside(r: &D2D_RECT_F, x: f32, y: f32) -> bool {
    x >= r.left && x < r.right && y >= r.top && y < r.bottom
}

#[cfg(test)]
mod tests {
    use super::{inside, rect, Hit};

    #[test]
    fn hit_test_prefers_the_topmost_area() {
        let r = rect(0.0, 0.0, 100.0, 34.0);
        let close = rect(72.0, 7.0, 20.0, 20.0);
        assert!(inside(&r, 80.0, 10.0) && inside(&close, 80.0, 10.0));
        let hits = [(r, Hit::Tab(1)), (close, Hit::TabClose(1))];
        let found = hits.iter().rev().find(|(r, _)| inside(r, 80.0, 10.0)).map(|(_, h)| *h);
        assert_eq!(found, Some(Hit::TabClose(1)));
    }
}
