use std::collections::HashSet;

#[derive(Default)]
pub(crate) struct MediaFrames {
    live: HashSet<String>,
    overflow: bool,
}

impl MediaFrames {
    pub(crate) fn set(&mut self, frame: String, live: bool) -> bool {
        if live {
            if self.live.len() < 64 || self.live.contains(&frame) { self.live.insert(frame); }
            else { self.overflow = true; }
        } else { self.live.remove(&frame); }
        self.overflow || !self.live.is_empty()
    }
    pub(crate) fn clear(&mut self) { self.live.clear(); self.overflow = false; }
}

pub(crate) fn media_message(body: &str) -> Option<(String, bool)> {
    let fields = body.strip_prefix("kk:media:")?;
    let (frame, live) = fields.rsplit_once(':').unwrap_or(("legacy", fields));
    if frame.is_empty() || frame.len() > 64 || !frame.bytes().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_')) { return None; }
    Some((frame.into(), match live { "1" => true, "0" => false, _ => return None }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_frame_cannot_clear_sibling_playback() {
        let mut frames = MediaFrames::default();
        assert!(frames.set("video".into(), true));
        assert!(frames.set("call".into(), true));
        assert!(frames.set("video".into(), false));
        assert!(!frames.set("call".into(), false));
    }

    #[test]
    fn overload_stays_protected_without_unbounded_storage_until_navigation() {
        let mut frames = MediaFrames::default();
        for id in 0..100 { assert!(frames.set(format!("frame{id}"), true)); }
        assert!(frames.live.len() <= 64);
        for id in 0..100 { frames.set(format!("frame{id}"), false); }
        assert!(frames.set("unknown".into(), false));
        frames.clear();
        assert!(!frames.set("unknown".into(), false));
    }

    #[test]
    fn parse_rejects_unbounded_or_ambiguous_sources() {
        assert_eq!(media_message("kk:media:document-1:1"), Some(("document-1".into(), true)));
        assert_eq!(media_message("kk:media:0"), Some(("legacy".into(), false)));
        for message in ["kk:media::1", "kk:media:x:2", "kk:media:x:y:0"] { assert!(media_message(message).is_none()); }
        assert!(media_message(&format!("kk:media:{}:1", "x".repeat(65))).is_none());
    }
}
