//! Motion of the native UI (Settings > Appearance): whether things animate, how fast, and
//! whether scrolling glides. One setting for every window, so it is kept here rather than passed
//! to each of them.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::{Duration, Instant};

/// Duration scale of every animation (bits of an f32); 0 = no animations.
static SCALE: AtomicU32 = AtomicU32::new(0x3f80_0000); // 1.0
static SMOOTH_SCROLL: AtomicBool = AtomicBool::new(true);

/// Frame interval of running animations (about 60 per second).
pub(crate) const FRAME_MS: u32 = 15;

/// Animations at `speed` (1.0 normal, below 1 faster, above 1 slower), or none with `None`;
/// `smooth_scroll` makes scrolling glide instead of jump.
pub fn set_motion(speed: Option<f32>, smooth_scroll: bool) {
    SCALE.store(speed.map_or(0.0, |s| s.clamp(0.25, 4.0)).to_bits(), Ordering::Relaxed);
    SMOOTH_SCROLL.store(smooth_scroll, Ordering::Relaxed);
}

fn scale() -> f32 {
    f32::from_bits(SCALE.load(Ordering::Relaxed))
}

pub(crate) fn enabled() -> bool {
    scale() > 0.0
}

pub(crate) fn smooth_scroll() -> bool {
    SMOOTH_SCROLL.load(Ordering::Relaxed)
}

/// How far (0..=1) an animation of `ms` milliseconds (at normal speed), started at `start` after
/// a `delay_ms`, has come. Always 1 when animations are off.
pub(crate) fn progress(start: Instant, delay_ms: f32, ms: f32) -> f32 {
    let s = scale();
    if s == 0.0 {
        return 1.0;
    }
    let t = start.elapsed().as_secs_f32() * 1000.0 - delay_ms * s;
    (t / (ms * s)).clamp(0.0, 1.0)
}

/// Fast start, gentle stop (cubic).
pub(crate) fn ease_out(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(3)
}

/// A dropped ball: falls, then bounces lower and lower until it rests (0 → 1).
pub(crate) fn ease_out_bounce(t: f32) -> f32 {
    const N: f32 = 7.5625;
    const D: f32 = 2.75;
    if t < 1.0 / D {
        N * t * t
    } else if t < 2.0 / D {
        let t = t - 1.5 / D;
        N * t * t + 0.75
    } else if t < 2.5 / D {
        let t = t - 2.25 / D;
        N * t * t + 0.9375
    } else {
        let t = t - 2.625 / D;
        N * t * t + 0.984375
    }
}

/// Where (0..1) a repeating animation of `period_ms` (at normal speed) started at `start` is in
/// its cycle; `None` when animations are off.
pub(crate) fn phase(start: Instant, period_ms: f32) -> Option<f32> {
    let s = scale();
    (s > 0.0).then(|| (start.elapsed().as_secs_f32() * 1000.0 / (period_ms * s)).fract())
}

/// `ease_out(progress(..))`.
pub(crate) fn eased(start: Instant, delay_ms: f32, ms: f32) -> f32 {
    ease_out(progress(start, delay_ms, ms))
}

/// An instant long enough ago that every animation started then has ended.
pub(crate) fn long_ago() -> Instant {
    Instant::now().checked_sub(Duration::from_secs(60)).unwrap_or_else(Instant::now)
}

/// `a` → `b` (0xRRGGBB) at `t`.
pub(crate) fn mix(a: u32, b: u32, t: f32) -> u32 {
    let ch = |c: u32, shift: u32| ((c >> shift) & 0xff) as f32;
    let one = |shift: u32| (ch(a, shift) + (ch(b, shift) - ch(a, shift)) * t).round() as u32 & 0xff;
    (one(16) << 16) | (one(8) << 8) | one(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eases_mixes_and_switches_off() {
        assert_eq!(ease_out(0.0), 0.0);
        assert_eq!(ease_out(1.0), 1.0);
        assert!(ease_out(0.5) > 0.5, "fast start");
        assert_eq!(mix(0x000000, 0xffffff, 0.5), 0x808080);
        assert_eq!(ease_out_bounce(0.0), 0.0);
        assert!((ease_out_bounce(1.0) - 1.0).abs() < 1e-5);
        assert!(ease_out_bounce(0.5) > 0.7, "it has landed once and bounces lower");
        assert_eq!(mix(0x102030, 0x102030, 0.3), 0x102030);
        assert!(progress(long_ago(), 0.0, 200.0) == 1.0);
        set_motion(None, true);
        assert_eq!(progress(Instant::now(), 0.0, 200.0), 1.0, "off: always finished");
        set_motion(Some(1.0), true);
        assert!(progress(Instant::now(), 0.0, 200.0) < 0.5);
    }
}
