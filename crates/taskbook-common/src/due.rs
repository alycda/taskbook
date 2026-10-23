//! Due-date parsing, formatting and status classification.
//!
//! Due dates are stored as epoch milliseconds. A due date without an explicit
//! time of day is stored at local midnight of the due day (and treated as a
//! whole-day deadline); a due date with a time is stored at that exact local
//! instant.

use chrono::{DateTime, Duration, Local, NaiveDate, NaiveDateTime, NaiveTime, TimeZone};

/// Classification of a due date relative to now (local time).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DueStatus {
    Overdue,
    DueToday,
    Upcoming,
}

/// The user-facing error for a due-date string [`parse_due_date`] rejects.
pub fn invalid_due_date_message(raw: &str) -> String {
    format!(
        "invalid due date '{raw}' (use YYYY-MM-DD, \"YYYY-MM-DD HH:MM\", today, tomorrow, now, \
         or today/tomorrow/now+HHMM)"
    )
}

/// Parse a due-date string into epoch millis (local time).
///
/// Accepted formats (case-insensitive for keywords):
/// - `YYYY-MM-DD` — whole-day deadline at local midnight
/// - `YYYY-MM-DDTHH:MM[:SS]` or `YYYY-MM-DD HH:MM[:SS]` — exact local instant
/// - `today`, `tomorrow`, `now` — relative to the current moment
/// - `today+HHMM`, `tomorrow+HHMM` — that day plus `HH` hours and `MM` minutes
///   (so `today+1430` is today at 14:30)
/// - `now+HHMM` — `HH` hours and `MM` minutes from the current moment
///
/// The offset accepts an optional colon (`14:30` == `1430`).
pub fn parse_due_date(raw: &str) -> Option<i64> {
    let s = raw.trim();
    if s.is_empty() {
        return None;
    }

    // Relative keywords (today/tomorrow/now), optionally with a +HHMM offset.
    if let Some(millis) = parse_relative(&s.to_lowercase()) {
        return Some(millis);
    }

    // Absolute date-time (carries a time of day).
    for fmt in [
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%dT%H:%M",
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%d %H:%M",
    ] {
        if let Ok(ndt) = NaiveDateTime::parse_from_str(s, fmt) {
            return naive_to_millis(ndt);
        }
    }

    // Absolute whole-day deadline (local midnight).
    let date = NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()?;
    date_to_millis(date)
}

/// Parse `today` / `tomorrow` / `now`, with an optional `+HHMM` offset.
fn parse_relative(s: &str) -> Option<i64> {
    let (base, offset) = match s.split_once('+') {
        Some((b, o)) => (b.trim(), Some(o.trim())),
        None => (s, None),
    };

    let base_dt: DateTime<Local> = match base {
        "today" => start_of_day(Local::now().date_naive())?,
        "tomorrow" => start_of_day(Local::now().date_naive().succ_opt()?)?,
        "now" => Local::now(),
        _ => return None,
    };

    let dt = match offset {
        None => base_dt,
        Some(off) => {
            let (hours, minutes) = parse_hhmm(off)?;
            base_dt + Duration::hours(hours) + Duration::minutes(minutes)
        }
    };
    Some(dt.timestamp_millis())
}

/// Parse an `HHMM` (or `HH:MM`) offset into (hours, minutes).
fn parse_hhmm(raw: &str) -> Option<(i64, i64)> {
    let digits: String = raw.chars().filter(|c| *c != ':').collect();
    if !(3..=4).contains(&digits.len()) || !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let split = digits.len() - 2;
    let hours: i64 = digits[..split].parse().ok()?;
    let minutes: i64 = digits[split..].parse().ok()?;
    if minutes >= 60 {
        return None;
    }
    Some((hours, minutes))
}

fn start_of_day(date: NaiveDate) -> Option<DateTime<Local>> {
    Local
        .from_local_datetime(&date.and_hms_opt(0, 0, 0)?)
        .earliest()
}

fn date_to_millis(date: NaiveDate) -> Option<i64> {
    start_of_day(date).map(|dt| dt.timestamp_millis())
}

fn naive_to_millis(ndt: NaiveDateTime) -> Option<i64> {
    Local
        .from_local_datetime(&ndt)
        .earliest()
        .map(|dt| dt.timestamp_millis())
}

/// Whether a due-date millis value carries an explicit time of day (i.e. it is
/// not exactly local midnight).
fn has_time_of_day(local: DateTime<Local>) -> bool {
    local.time() != NaiveTime::MIN
}

/// Format a due-date millis value in local time.
///
/// Whole-day deadlines render as `YYYY-MM-DD`; dues with a time of day render
/// as `YYYY-MM-DD HH:MM`.
pub fn format_due_date(millis: i64) -> String {
    DateTime::from_timestamp_millis(millis)
        .map(|dt| {
            let local = dt.with_timezone(&Local);
            if has_time_of_day(local) {
                local.format("%Y-%m-%d %H:%M").to_string()
            } else {
                local.format("%Y-%m-%d").to_string()
            }
        })
        .unwrap_or_default()
}

/// Classify a due date against the current local time.
///
/// Whole-day deadlines (local midnight) are classified by date, so a task due
/// `today` stays `DueToday` for the whole day. Dues with an explicit time are
/// classified by the exact instant, so they flip to `Overdue` once the time
/// passes.
pub fn due_status(due_millis: i64) -> DueStatus {
    let Some(due) = DateTime::from_timestamp_millis(due_millis).map(|dt| dt.with_timezone(&Local))
    else {
        return DueStatus::Upcoming;
    };
    let now = Local::now();
    let today = now.date_naive();

    if has_time_of_day(due) {
        if due < now {
            DueStatus::Overdue
        } else if due.date_naive() == today {
            DueStatus::DueToday
        } else {
            DueStatus::Upcoming
        }
    } else {
        match due.date_naive() {
            d if d < today => DueStatus::Overdue,
            d if d == today => DueStatus::DueToday,
            _ => DueStatus::Upcoming,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Timelike;

    fn local_dt(millis: i64) -> DateTime<Local> {
        DateTime::from_timestamp_millis(millis)
            .unwrap()
            .with_timezone(&Local)
    }

    #[test]
    fn parse_iso_date_round_trips() {
        let millis = parse_due_date("2026-07-01").unwrap();
        assert_eq!(format_due_date(millis), "2026-07-01");
    }

    #[test]
    fn parse_iso_datetime_round_trips() {
        let millis = parse_due_date("2026-07-01T14:30").unwrap();
        assert_eq!(format_due_date(millis), "2026-07-01 14:30");
    }

    #[test]
    fn parse_iso_datetime_space_separator() {
        let millis = parse_due_date("2026-07-01 09:05").unwrap();
        assert_eq!(format_due_date(millis), "2026-07-01 09:05");
    }

    #[test]
    fn parse_iso_datetime_with_seconds() {
        // Seconds are accepted; display truncates to minutes.
        let millis = parse_due_date("2026-07-01T14:30:45").unwrap();
        assert_eq!(format_due_date(millis), "2026-07-01 14:30");
    }

    #[test]
    fn midnight_time_renders_as_date_only() {
        let millis = parse_due_date("2026-07-01T00:00").unwrap();
        assert_eq!(format_due_date(millis), "2026-07-01");
    }

    #[test]
    fn parse_today_and_tomorrow() {
        let today = Local::now().date_naive();
        let t = parse_due_date("today").unwrap();
        assert_eq!(format_due_date(t), today.format("%Y-%m-%d").to_string());
        let tm = parse_due_date("Tomorrow").unwrap();
        assert_eq!(
            format_due_date(tm),
            today.succ_opt().unwrap().format("%Y-%m-%d").to_string()
        );
    }

    #[test]
    fn parse_today_plus_offset_sets_time_of_day() {
        let millis = parse_due_date("today+1430").unwrap();
        let dt = local_dt(millis);
        assert_eq!(dt.date_naive(), Local::now().date_naive());
        assert_eq!(dt.hour(), 14);
        assert_eq!(dt.minute(), 30);
    }

    #[test]
    fn parse_tomorrow_plus_offset_with_colon() {
        let millis = parse_due_date("tomorrow+09:05").unwrap();
        let dt = local_dt(millis);
        assert_eq!(
            dt.date_naive(),
            Local::now().date_naive().succ_opt().unwrap()
        );
        assert_eq!(dt.hour(), 9);
        assert_eq!(dt.minute(), 5);
    }

    #[test]
    fn parse_now_plus_offset_adds_duration() {
        let before = Local::now();
        let millis = parse_due_date("now+0130").unwrap();
        let dt = local_dt(millis);
        let delta = dt - before;
        // ~1h30m from now (allow a generous window for test execution time).
        assert!(delta >= Duration::minutes(89) && delta <= Duration::minutes(91));
    }

    #[test]
    fn parse_invalid_returns_none() {
        assert_eq!(parse_due_date("next week"), None);
        assert_eq!(parse_due_date("2026-13-99"), None);
        assert_eq!(parse_due_date(""), None);
        assert_eq!(parse_due_date("today+99"), None); // offset too short
        assert_eq!(parse_due_date("today+1499"), None); // minutes >= 60
        assert_eq!(parse_due_date("today+abcd"), None);
    }

    #[test]
    fn status_classification_whole_day() {
        let today = parse_due_date("today").unwrap();
        assert_eq!(due_status(today), DueStatus::DueToday);
        let tomorrow = parse_due_date("tomorrow").unwrap();
        assert_eq!(due_status(tomorrow), DueStatus::Upcoming);
        let yesterday = today - 24 * 60 * 60 * 1000;
        assert_eq!(due_status(yesterday), DueStatus::Overdue);
    }

    #[test]
    fn status_timed_due_is_time_granular() {
        let now = Local::now();
        // A timed due one hour ago today is overdue (not merely "due today").
        let past = (now - Duration::hours(1)).timestamp_millis();
        assert_eq!(due_status(past), DueStatus::Overdue);
        // A timed due one hour from now today is due today.
        let soon = (now + Duration::hours(1)).timestamp_millis();
        // Guard against the rare case where +1h crosses midnight.
        if local_dt(soon).date_naive() == now.date_naive() {
            assert_eq!(due_status(soon), DueStatus::DueToday);
        }
    }
}
