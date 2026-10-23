//! Default take naming: `take-YYYYMMDD-HHMM` in the device's local
//! timezone, anchored to the recording's persisted start time (design D5).

use crate::AudioError;

/// Default take title `take-YYYYMMDD-HHMM` for a recording that started at
/// `started_at_unix_secs`, rendered in `tz` (pass
/// `jiff::tz::TimeZone::system()` for the device's local timezone).
///
/// Anchored to the start time — not the finish — so interrupted takes
/// recovered later still get a stable name consistent with
/// `RecordingMeta::started_at`.
pub fn default_title(
    started_at_unix_secs: u64,
    tz: &jiff::tz::TimeZone,
) -> Result<String, AudioError> {
    let secs = i64::try_from(started_at_unix_secs)
        .map_err(|_| AudioError::InvalidConfig("started_at out of range".into()))?;
    let timestamp = jiff::Timestamp::new(secs, 0)
        .map_err(|e| AudioError::InvalidConfig(format!("invalid started_at: {e}")))?;
    Ok(format!(
        "take-{}",
        timestamp.to_zoned(tz.clone()).strftime("%Y%m%d-%H%M")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::tz::{Offset, TimeZone};

    // 2026-10-22T08:15:00Z.
    const T_2026_10_22_0815Z: u64 = 1_792_656_900;

    #[test]
    fn title_uses_local_timezone() {
        let plus2 = TimeZone::fixed(Offset::constant(2));
        assert_eq!(
            default_title(T_2026_10_22_0815Z, &plus2).unwrap(),
            "take-20261022-1015"
        );
        assert_eq!(
            default_title(T_2026_10_22_0815Z, &TimeZone::UTC).unwrap(),
            "take-20261022-0815"
        );
    }

    #[test]
    fn title_flips_at_midnight_boundary() {
        let plus2 = TimeZone::fixed(Offset::constant(2));
        // 2026-10-22 23:59 local (+02:00) is 21:59Z.
        let t_2359_local = 1_792_706_340u64;
        assert_eq!(
            default_title(t_2359_local, &plus2).unwrap(),
            "take-20261022-2359"
        );
        // One minute later the date rolls over.
        assert_eq!(
            default_title(t_2359_local + 60, &plus2).unwrap(),
            "take-20261023-0000"
        );
    }
}
