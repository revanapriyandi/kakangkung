//! Local calendar time (Windows' time zone rules) for dates shown on built-in pages.

use windows::Win32::Foundation::{FILETIME, SYSTEMTIME};
use windows::Win32::System::Time::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime};

/// A moment in the local time zone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LocalTime {
    pub year: u16,
    pub month: u16,
    pub day: u16,
    /// 0 = Sunday.
    pub weekday: u16,
    pub hour: u16,
    pub minute: u16,
}

/// Unix milliseconds as local time (`None` for times Windows cannot convert).
pub fn local_time(unix_ms: i64) -> Option<LocalTime> {
    // FILETIME counts 100 ns steps since 1601-01-01.
    let ticks = (unix_ms.checked_add(11_644_473_600_000)?).checked_mul(10_000)?;
    let ft = FILETIME { dwLowDateTime: ticks as u32, dwHighDateTime: (ticks >> 32) as u32 };
    let (mut utc, mut local) = (SYSTEMTIME::default(), SYSTEMTIME::default());
    // SAFETY: plain conversions between valid local structs.
    unsafe {
        FileTimeToSystemTime(&ft, &mut utc).ok()?;
        SystemTimeToTzSpecificLocalTime(None, &utc, &mut local).ok()?;
    }
    Some(LocalTime { year: local.wYear, month: local.wMonth, day: local.wDay, weekday: local.wDayOfWeek, hour: local.wHour, minute: local.wMinute })
}

#[cfg(test)]
mod tests {
    use super::local_time;

    #[test]
    fn converts_unix_milliseconds() {
        // 2024-01-15 12:00 UTC: the local date is the 15th or a neighbour, whatever the zone.
        let t = local_time(1_705_320_000_000).expect("converts");
        assert_eq!((t.year, t.month), (2024, 1));
        assert!((14..=16).contains(&t.day));
    }
}
