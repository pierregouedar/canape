//! Time of day for the status strip, as "HH:MM".

use std::time::Duration;

/// How often the strip is refreshed: often enough for the minute to turn
/// within a few seconds of the real one.
pub const REFRESH: Duration = Duration::from_secs(5);

#[cfg(windows)]
pub fn now() -> String {
    use windows_sys::Win32::System::SystemInformation::GetLocalTime;
    // SAFETY: GetLocalTime only fills the structure it is given.
    let t = unsafe {
        let mut t = std::mem::zeroed();
        GetLocalTime(&mut t);
        t
    };
    format(t.wHour.into(), t.wMinute.into())
}

/// Without a local time zone at hand, UTC: only used while developing.
#[cfg(not(windows))]
pub fn now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    format(secs / 3600 % 24, secs / 60 % 60)
}

fn format(hour: u64, minute: u64) -> String {
    format!("{hour}:{minute:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minutes_are_padded_hours_are_not() {
        assert_eq!(format(9, 5), "9:05");
        assert_eq!(format(21, 42), "21:42");
    }
}
