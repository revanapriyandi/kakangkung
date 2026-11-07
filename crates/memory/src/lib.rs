//! Memory policy: Memory Saver modes and the rules that protect a tab from being frozen. No platform types.

use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    /// Chrome's "Moderate" (conservative aggressiveness): only tabs hidden for a long time.
    Moderate,
    UltraLite,
    #[default]
    Balanced,
    Performance,
}

/// `None` = this step is off in the mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Thresholds {
    pub suspend_after: Option<Duration>,
    pub discard_after: Option<Duration>,
}

/// Minutes a hidden tab waits before it may be discarded (two hours, the common browser default).
pub const DISCARD_AFTER_MIN: u64 = 120;

impl Mode {
    /// Default thresholds of each mode; starting points, to be tuned with measurements.
    pub fn default_thresholds(self) -> Thresholds {
        let mins = |m: u64| Some(Duration::from_secs(m * 60));
        match self {
            Mode::Moderate => Thresholds { suspend_after: mins(DISCARD_AFTER_MIN), discard_after: None },
            Mode::UltraLite => Thresholds { suspend_after: Some(Duration::from_secs(30)), discard_after: mins(10) },
            Mode::Balanced => Thresholds { suspend_after: mins(5), discard_after: None },
            Mode::Performance => Thresholds { suspend_after: None, discard_after: None },
        }
    }
}

/// Protection signals for one tab. `None` = the signal could not be read.
#[derive(Debug, Clone, Copy, Default)]
pub struct Signals {
    /// `IsDocumentPlayingAudio` (reliable).
    pub playing_audio: Option<bool>,
    /// App's own download tracking (reliable).
    pub downloading: bool,
    /// Camera/mic granted and a live track seen by the injected script (heuristic).
    pub capturing_media: Option<bool>,
    /// Dirty inputs or a `beforeunload` handler seen by the injected script (heuristic).
    pub unsaved_input: Option<bool>,
    /// Pinned tab or user-excluded site (reliable).
    pub pinned_or_excluded: bool,
}

impl Signals {
    /// Uncertain signals count as protected.
    pub fn protected(&self) -> bool {
        let yes_or_unknown = |s: Option<bool>| s.unwrap_or(true);
        self.downloading
            || self.pinned_or_excluded
            || yes_or_unknown(self.playing_audio)
            || yes_or_unknown(self.capturing_media)
            || yes_or_unknown(self.unsaved_input)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_defaults_match_spec() {
        let t = Mode::UltraLite.default_thresholds();
        assert_eq!(t.suspend_after, Some(Duration::from_secs(30)));
        assert_eq!(t.discard_after, Some(Duration::from_secs(600)));
        let t = Mode::Balanced.default_thresholds();
        assert_eq!(t.suspend_after, Some(Duration::from_secs(300)));
        assert_eq!(t.discard_after, None);
        assert_eq!(Mode::Performance.default_thresholds(), Thresholds { suspend_after: None, discard_after: None });
        assert_eq!(Mode::Moderate.default_thresholds().suspend_after, Some(Duration::from_secs(120 * 60)));
        assert_eq!(Mode::default(), Mode::Balanced, "Chrome's default aggressiveness is the middle level");
    }

    #[test]
    fn unknown_signal_means_protected() {
        let known_quiet = Signals {
            playing_audio: Some(false),
            capturing_media: Some(false),
            unsaved_input: Some(false),
            ..Default::default()
        };
        assert!(!known_quiet.protected());
        assert!(Signals::default().protected(), "all unknown -> protected");
        assert!(Signals { unsaved_input: None, ..known_quiet }.protected());
        assert!(Signals { playing_audio: Some(true), ..known_quiet }.protected());
        assert!(Signals { downloading: true, ..known_quiet }.protected());
        assert!(Signals { pinned_or_excluded: true, ..known_quiet }.protected());
    }
}
