//! Dates and times as the built-in pages write them (local time zone).

use platform::LocalTime;

const MONTHS: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
const DAYS: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];

pub(crate) fn local(ms: i64) -> LocalTime {
    platform::local_time(ms).unwrap_or_default()
}

/// "14:05".
pub(crate) fn clock(t: LocalTime) -> String {
    format!("{:02}:{:02}", t.hour, t.minute)
}

/// "8 Nov 2026".
pub(crate) fn short_date(t: LocalTime) -> String {
    let month = MONTHS.get(t.month.saturating_sub(1) as usize).map_or("", |m| &m[..3]);
    format!("{} {month} {}", t.day, t.year)
}

/// "Today · Saturday, 8 November 2026" (or "Yesterday · …", or just the date).
pub(crate) fn day_title(t: LocalTime, today: LocalTime, yesterday: LocalTime) -> String {
    let same = |a: LocalTime, b: LocalTime| (a.year, a.month, a.day) == (b.year, b.month, b.day);
    let month = MONTHS.get(t.month.saturating_sub(1) as usize).copied().unwrap_or("");
    let date = format!("{}, {} {month} {}", DAYS.get(t.weekday as usize).copied().unwrap_or(""), t.day, t.year);
    if same(t, today) {
        format!("Today · {date}")
    } else if same(t, yesterday) {
        format!("Yesterday · {date}")
    } else {
        date
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_dates_and_days() {
        let t = LocalTime { year: 2026, month: 11, day: 8, weekday: 0, hour: 9, minute: 5 };
        let y = LocalTime { day: 7, weekday: 6, ..t };
        assert_eq!(clock(t), "09:05");
        assert_eq!(short_date(t), "8 Nov 2026");
        assert_eq!(day_title(t, t, y), "Today · Sunday, 8 November 2026");
        assert_eq!(day_title(y, t, y), "Yesterday · Saturday, 7 November 2026");
        assert_eq!(day_title(LocalTime { day: 1, weekday: 0, ..t }, t, y), "Sunday, 1 November 2026");
    }
}
