//! Motion of the native UI (Settings > Appearance): whether things animate, how fast, and
//! whether scrolling glides. One setting for every window, so it is kept here rather than passed
//! to each of them.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::{Duration, Instant};

/// Duration scale of every animation (bits of an f32); 0 = no animations.
static SCALE: AtomicU32 = AtomicU32::new(0x3f80_0000); // 1.0
static SMOOTH_SCROLL: AtomicBool = AtomicBool::new(true);

/// Frame interval of running animations. Windows timers fire on its 15.6 ms tick: 15 gives every
/// tick (about 64 frames a second); 16 would round up to every other tick (32).
pub(crate) const FRAME_MS: u32 = 15;

/// Animations at `speed` (1.0 normal, below 1 faster, above 1 slower), or none with `None`;
/// `smooth_scroll` makes scrolling glide instead of jump.
pub fn set_motion(speed: Option<f32>, smooth_scroll: bool) {
    // Windows' "Animation effects" switched off (Accessibility > Visual effects) also means none here.
    // (Unit tests ignore it: CI machines often run with it off.)
    let speed = speed.filter(|_| cfg!(test) || windows_animates());
    SCALE.store(speed.map_or(0.0, |s| s.clamp(0.25, 4.0)).to_bits(), Ordering::Relaxed);
    SMOOTH_SCROLL.store(smooth_scroll, Ordering::Relaxed);
}

/// Whether Windows animates its own windows (true when the setting cannot be read).
fn windows_animates() -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{SystemParametersInfoW, SPI_GETCLIENTAREAANIMATION, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS};
    let mut on = windows::core::BOOL(1);
    // SAFETY: SPI_GETCLIENTAREAANIMATION writes one BOOL into the valid local.
    let read = unsafe { SystemParametersInfoW(SPI_GETCLIENTAREAANIMATION, 0, Some((&mut on as *mut windows::core::BOOL).cast()), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0)) };
    read.is_err() || on.as_bool()
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

/// Fast start, gentle stop (quartic): the interface answers at once and settles softly.
pub(crate) fn ease_out(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(4)
}

/// Start for an animation of `ms` that turns back one started at `start` while it still runs:
/// the new one begins exactly where the old one is on screen (eased), so changing one's mind
/// half-way never jumps. `now` when the old one has finished.
pub(crate) fn reverse(start: Instant, ms: f32) -> Instant {
    let done = progress(start, 0.0, ms);
    if done >= 1.0 {
        return Instant::now();
    }
    // Where the old one is (eased), seen from the other end.
    start_at(1.0 - ease_out(done), ms)
}

/// Start for an animation of `ms` that is already `level` (0..=1, eased) of the way: it picks up
/// exactly there (a pop-up reopened while it fades out fades back in from its current opacity).
pub(crate) fn start_at(level: f32, ms: f32) -> Instant {
    start_on(level, ms, ease_out)
}

/// Like `start_at`, for an opacity fade (`fade`).
pub(crate) fn fade_start_at(level: f32, ms: f32) -> Instant {
    start_on(level, ms, fade_curve)
}

fn start_on(level: f32, ms: f32, curve: fn(f32) -> f32) -> Instant {
    let now = Instant::now();
    if level <= 0.0 || !enabled() {
        return now;
    }
    // Back from the curve to linear time.
    let (mut lo, mut hi) = (0.0f32, 1.0f32);
    for _ in 0..24 {
        let mid = (lo + hi) / 2.0;
        if curve(mid) < level.min(1.0) { lo = mid } else { hi = mid }
    }
    now.checked_sub(Duration::from_secs_f32(lo * ms * scale() / 1000.0)).unwrap_or(now)
}

/// Opacity curve: a gentler ease-out than `ease_out`, so a short fade shows its in-between frames
/// instead of jumping to nearly opaque on the first one (movement keeps the snappier curve).
fn fade_curve(t: f32) -> f32 {
    1.0 - (1.0 - t) * (1.0 - t)
}

/// How far (0..=1) an opacity fade of `ms` started at `start` has come.
pub(crate) fn fade(start: Instant, ms: f32) -> f32 {
    fade_curve(progress(start, 0.0, ms))
}

/// Where an opening fade starts: already a little visible, so the first frame on screen shows
/// something at once instead of an empty step.
pub(crate) const FADE_FROM: f32 = 0.2;

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

    #[test]
    fn reversing_mid_way_continues_from_the_same_place() {
        set_motion(Some(1.0), true);
        let start = Instant::now() - Duration::from_millis(50);
        let shown = eased(start, 0.0, 200.0);
        let back = reverse(start, 200.0);
        let from_other_end = 1.0 - eased(back, 0.0, 200.0);
        assert!((shown - from_other_end).abs() < 0.03, "{shown} vs {from_other_end}");
        assert!(reverse(long_ago(), 200.0).elapsed().as_millis() < 5, "finished: starts now");
        assert!((eased(start_at(0.6, 200.0), 0.0, 200.0) - 0.6).abs() < 0.03, "starts part-way");
        assert!((fade(fade_start_at(0.6, 200.0), 200.0) - 0.6).abs() < 0.03, "fades from part-way");
        assert!(fade_curve(0.1) < ease_out(0.1), "fades show their in-between frames");
    }
}
