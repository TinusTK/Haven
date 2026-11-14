//! Haven's video player.
//!
//! The rules live in plain modules that build everywhere: which addresses
//! may be played ([`source`]), when a title resumes or counts as watched
//! ([`resume`]) and how mpv's tracks are named ([`tracks`]). The part that
//! talks to libmpv and draws the picture is [`video`], behind the `mpv`
//! feature.

pub mod resume;
pub mod source;
pub mod tracks;
#[cfg(feature = "mpv")]
pub mod video;

/// Seconds as a clock: `4:05`, or `1:02:03` from one hour up.
pub fn clock(seconds: f64) -> String {
    let total = if seconds.is_finite() { seconds.max(0.0) as u64 } else { 0 };
    let (h, m, s) = (total / 3600, total / 60 % 60, total % 60);
    if h > 0 { format!("{h}:{m:02}:{s:02}") } else { format!("{m}:{s:02}") }
}

#[cfg(test)]
mod tests {
    use super::clock;

    #[test]
    fn clock_shows_hours_only_when_needed() {
        assert_eq!(clock(0.0), "0:00");
        assert_eq!(clock(245.9), "4:05");
        assert_eq!(clock(3723.0), "1:02:03");
        assert_eq!(clock(-5.0), "0:00");
        assert_eq!(clock(f64::NAN), "0:00");
    }
}
