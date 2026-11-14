//! When a title is remembered, resumed, or counted as watched.

/// Nothing is remembered before this many seconds have been watched.
pub const KEEP_AFTER: f64 = 30.0;
/// From this share of the running time on, the title counts as watched.
pub const WATCHED_FROM: f64 = 0.95;
/// Resuming starts this many seconds earlier, to pick the thread back up.
pub const REWIND: f64 = 3.0;

/// How far someone got into a video.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Watched {
    /// Barely started: not worth remembering.
    Started,
    /// Somewhere in the middle: remember the place.
    Partway,
    /// At or near the end.
    Finished,
}

/// Judges a position. `None` when the running time is unknown (a live
/// stream), where there is no place to remember.
pub fn judge(position: f64, duration: f64) -> Option<Watched> {
    if !position.is_finite() || !duration.is_finite() || duration <= 0.0 {
        return None;
    }
    Some(if position >= duration * WATCHED_FROM {
        Watched::Finished
    } else if position >= KEEP_AFTER {
        Watched::Partway
    } else {
        Watched::Started
    })
}

/// Where to start a title that was left at `position`; `None` to start
/// from the beginning.
pub fn resume_from(position: f64, duration: f64, finished: bool) -> Option<f64> {
    if finished || judge(position, duration) != Some(Watched::Partway) {
        return None;
    }
    Some((position - REWIND).max(0.0))
}

/// The share watched, from 0 to 1, for a progress bar.
pub fn share(position: f64, duration: f64) -> f32 {
    if !position.is_finite() || !duration.is_finite() || duration <= 0.0 {
        return 0.0;
    }
    (position / duration).clamp(0.0, 1.0) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn judges_the_start_middle_and_end() {
        assert_eq!(judge(5.0, 3600.0), Some(Watched::Started));
        assert_eq!(judge(30.0, 3600.0), Some(Watched::Partway));
        assert_eq!(judge(1800.0, 3600.0), Some(Watched::Partway));
        assert_eq!(judge(3420.0, 3600.0), Some(Watched::Finished));
        assert_eq!(judge(3600.0, 3600.0), Some(Watched::Finished));
    }

    #[test]
    fn a_live_stream_has_no_place_to_remember() {
        assert_eq!(judge(500.0, 0.0), None);
        assert_eq!(judge(500.0, f64::NAN), None);
        assert_eq!(judge(f64::INFINITY, 100.0), None);
        assert_eq!(resume_from(500.0, 0.0, false), None);
    }

    #[test]
    fn resumes_a_little_before_where_it_stopped() {
        assert_eq!(resume_from(600.0, 3600.0, false), Some(597.0));
        assert_eq!(resume_from(10.0, 3600.0, false), None);
        assert_eq!(resume_from(3500.0, 3600.0, false), None);
        assert_eq!(resume_from(600.0, 3600.0, true), None);
    }

    #[test]
    fn share_stays_between_nothing_and_all() {
        assert_eq!(share(900.0, 3600.0), 0.25);
        assert_eq!(share(4000.0, 3600.0), 1.0);
        assert_eq!(share(-1.0, 3600.0), 0.0);
        assert_eq!(share(10.0, 0.0), 0.0);
    }
}
