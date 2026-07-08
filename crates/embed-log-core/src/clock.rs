use std::sync::Mutex;

use chrono::{DateTime, Local};

use crate::models::TimestampMode;

/// Manages session timestamps in absolute or relative mode.
///
/// In absolute mode, timestamps are wall-clock times.
/// In relative mode, timestamps are displayed as offsets (`T+HH:MM:SS.mmm`)
/// from the first log entry received.
pub struct SessionClock {
    mode: TimestampMode,
    origin: Mutex<Option<DateTime<Local>>>,
}

pub(crate) fn format_relative_millis(total_ms: u64) -> String {
    let hours = total_ms / 3_600_000;
    let minutes = (total_ms % 3_600_000) / 60_000;
    let seconds = (total_ms % 60_000) / 1_000;
    let millis = total_ms % 1_000;
    format!("T+{hours:02}:{minutes:02}:{seconds:02}.{millis:03}")
}

impl SessionClock {
    pub fn new(mode: TimestampMode) -> Self {
        Self {
            mode,
            origin: Mutex::new(None),
        }
    }

    pub fn mode(&self) -> TimestampMode {
        self.mode
    }

    /// Returns the origin timestamp (time of the first log entry), if set.
    pub fn first_log_at(&self) -> Option<DateTime<Local>> {
        *self.origin.lock().unwrap()
    }

    /// Record the origin time if not yet set. Returns true if this call set it.
    pub fn ensure_origin(&self, ts: DateTime<Local>) -> bool {
        let mut origin = self.origin.lock().unwrap();
        if origin.is_none() {
            *origin = Some(ts);
            true
        } else {
            false
        }
    }

    /// Format a timestamp for writing to session log files.
    ///
    /// Always uses absolute wall-clock format: `YYYY-MM-DD HH:MM:SS.mmm`
    pub fn file_timestamp(&self, ts: DateTime<Local>) -> String {
        ts.format("%Y-%m-%d %H:%M:%S%.3f").to_string()
    }

    /// Format a timestamp for display in the UI.
    ///
    /// Absolute mode: `HH:MM:SS.mmm`
    /// Relative mode: `T+HH:MM:SS.mmm`
    pub fn display_timestamp(&self, ts: DateTime<Local>) -> String {
        match self.mode {
            TimestampMode::Absolute => ts.format("%H:%M:%S%.3f").to_string(),
            TimestampMode::Relative => {
                let origin = *self.origin.lock().unwrap();
                match origin {
                    Some(origin_ts) => {
                        let delta = ts - origin_ts;
                        let total_ms = delta.num_milliseconds().max(0) as u64;
                        format_relative_millis(total_ms)
                    }
                    None => format_relative_millis(0),
                }
            }
        }
    }

    /// Return a numeric timestamp (milliseconds since epoch) for sorting in the UI.
    pub fn numeric_timestamp(&self, ts: DateTime<Local>) -> f64 {
        ts.timestamp_millis() as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, TimeZone};

    #[test]
    fn absolute_display() {
        let clock = SessionClock::new(TimestampMode::Absolute);
        let ts = Local.with_ymd_and_hms(2024, 6, 15, 14, 30, 5).unwrap();
        assert_eq!(clock.display_timestamp(ts), "14:30:05.000");
    }

    #[test]
    fn relative_display() {
        let clock = SessionClock::new(TimestampMode::Relative);
        let origin = Local.with_ymd_and_hms(2024, 6, 15, 14, 30, 0).unwrap();
        clock.ensure_origin(origin);

        let ts = Local.with_ymd_and_hms(2024, 6, 15, 14, 30, 5).unwrap();
        assert_eq!(clock.display_timestamp(ts), "T+00:00:05.000");
    }

    #[test]
    fn relative_display_with_ms() {
        let clock = SessionClock::new(TimestampMode::Relative);
        let origin = Local.with_ymd_and_hms(2024, 6, 15, 14, 30, 0).unwrap();
        clock.ensure_origin(origin);

        let ts = origin + Duration::milliseconds(1500);
        assert_eq!(clock.display_timestamp(ts), "T+00:00:01.500");
    }

    #[test]
    fn relative_display_with_hours() {
        let clock = SessionClock::new(TimestampMode::Relative);
        let origin = Local.with_ymd_and_hms(2024, 6, 15, 14, 30, 0).unwrap();
        clock.ensure_origin(origin);

        let ts = origin + Duration::milliseconds(3_723_045);
        assert_eq!(clock.display_timestamp(ts), "T+01:02:03.045");
    }

    #[test]
    fn relative_no_origin() {
        let clock = SessionClock::new(TimestampMode::Relative);
        let ts = Local::now();
        assert_eq!(clock.display_timestamp(ts), "T+00:00:00.000");
    }

    #[test]
    fn origin_set_once() {
        let clock = SessionClock::new(TimestampMode::Relative);
        let t1 = Local.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        let t2 = Local.with_ymd_and_hms(2024, 1, 1, 0, 0, 1).unwrap();

        assert!(clock.ensure_origin(t1));
        assert!(!clock.ensure_origin(t2));
        assert_eq!(clock.first_log_at(), Some(t1));
    }

    #[test]
    fn file_timestamp_format() {
        let clock = SessionClock::new(TimestampMode::Relative);
        let ts = Local.with_ymd_and_hms(2024, 6, 15, 14, 30, 5).unwrap();
        let formatted = clock.file_timestamp(ts);
        assert!(formatted.starts_with("2024-06-15 14:30:05"));
    }

    #[test]
    fn numeric_timestamp_is_epoch_millis() {
        let clock = SessionClock::new(TimestampMode::Absolute);
        let ts = Local.timestamp_millis_opt(1_718_459_405_123).unwrap();

        assert_eq!(clock.numeric_timestamp(ts), 1_718_459_405_123.0);
    }
}
