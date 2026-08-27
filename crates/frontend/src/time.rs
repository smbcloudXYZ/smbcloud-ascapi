//! Timestamp helpers for comparing against App Store Connect's expiry
//! strings without a date-parsing dependency.

/// Whether an ISO-8601 UTC timestamp is in the past.
///
/// Lexicographic comparison is sound for the fixed-width UTC timestamps
/// App Store Connect returns, and avoids a date-parsing dependency for a
/// field that is only ever compared and displayed.
pub fn is_expired(expiration_date: &str, now: &str) -> bool {
    expiration_date < now
}

/// Current UTC time as `YYYY-MM-DDTHH:MM:SS`, for comparison against
/// Apple's expiry strings.
pub fn now_iso8601() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    // days_from_civil, inverted. Shifting the era to March makes the leap
    // day the last day of the year, which removes every special case.
    let days = secs.div_euclid(86_400);
    let secs_of_day = secs.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
        y,
        m,
        d,
        secs_of_day / 3600,
        (secs_of_day % 3600) / 60,
        secs_of_day % 60
    )
}
