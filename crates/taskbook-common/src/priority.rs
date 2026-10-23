//! Task priority.
//!
//! Levels count down from the most urgent: `1` = high, `2` = medium,
//! `3` = normal (the default). This is the scale used everywhere a user or
//! agent sees a number (CLI, TUI, MCP) and in the stored `_priority` field.
//!
//! Taskbook 1.x and the original Node.js taskbook stored the inverse scale
//! in the `priority` field (1 = normal … 3 = high). Tasks still write that
//! legacy field alongside `_priority` so older clients sharing the same data
//! read the right value; see `models/task.rs` for which field is read.

use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum Priority {
    High = 1,
    Medium = 2,
    #[default]
    Normal = 3,
}

impl Priority {
    /// The user-facing level: 1 = high, 2 = medium, 3 = normal.
    pub fn level(self) -> u8 {
        self as u8
    }

    pub fn from_level(level: u8) -> Option<Self> {
        match level {
            1 => Some(Priority::High),
            2 => Some(Priority::Medium),
            3 => Some(Priority::Normal),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Priority::High => "high",
            Priority::Medium => "medium",
            Priority::Normal => "normal",
        }
    }

    /// Decode a stored `_priority` value; anything out of range is Normal.
    pub(crate) fn from_stored(level: u8) -> Self {
        Self::from_level(level).unwrap_or_default()
    }

    /// Decode the legacy (1.x / Node.js) `priority` field, where larger is
    /// more urgent. Mirrors 1.x rendering: 2 is medium, anything above is high.
    pub(crate) fn from_legacy(value: u8) -> Self {
        match value {
            0 | 1 => Priority::Normal,
            2 => Priority::Medium,
            _ => Priority::High,
        }
    }

    /// Encode for the legacy `priority` field.
    pub(crate) fn to_legacy(self) -> u8 {
        4 - self.level()
    }
}

impl fmt::Display for Priority {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "p{} ({})", self.level(), self.label())
    }
}

/// Error returned when a priority string cannot be parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsePriorityError(pub String);

impl fmt::Display for ParsePriorityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "invalid priority '{}' (use 1/high, 2/medium or 3/normal)",
            self.0
        )
    }
}

impl std::error::Error for ParsePriorityError {}

impl FromStr for Priority {
    type Err = ParsePriorityError;

    /// Accepts `1`-`3`, optionally prefixed with `p` or `p:`, or the labels
    /// `high`/`medium`/`normal` (case-insensitive).
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let lower = s.trim().to_ascii_lowercase();
        let bare = lower
            .strip_prefix("p:")
            .or_else(|| lower.strip_prefix('p'))
            .unwrap_or(&lower);
        let parsed = match bare {
            "high" => Some(Priority::High),
            "medium" => Some(Priority::Medium),
            "normal" => Some(Priority::Normal),
            digits => digits.parse::<u8>().ok().and_then(Priority::from_level),
        };
        parsed.ok_or_else(|| ParsePriorityError(s.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_levels_count_down_from_high() {
        assert_eq!(Priority::High.level(), 1);
        assert_eq!(Priority::Medium.level(), 2);
        assert_eq!(Priority::Normal.level(), 3);
        assert_eq!(Priority::default(), Priority::Normal);
    }

    #[test]
    fn test_ordering_sorts_high_first() {
        let mut v = vec![Priority::Normal, Priority::High, Priority::Medium];
        v.sort();
        assert_eq!(v, vec![Priority::High, Priority::Medium, Priority::Normal]);
    }

    #[test]
    fn test_parse_accepts_numbers_prefixes_and_labels() {
        for s in ["1", "p1", "p:1", "P:1", "high", "HIGH"] {
            assert_eq!(s.parse::<Priority>(), Ok(Priority::High), "{s}");
        }
        assert_eq!("2".parse::<Priority>(), Ok(Priority::Medium));
        assert_eq!("p:3".parse::<Priority>(), Ok(Priority::Normal));
        assert_eq!("normal".parse::<Priority>(), Ok(Priority::Normal));
    }

    #[test]
    fn test_parse_rejects_out_of_range_and_garbage() {
        for s in ["0", "4", "p:4", "p:", "", "urgent", "-1"] {
            assert!(s.parse::<Priority>().is_err(), "{s}");
        }
    }

    #[test]
    fn test_legacy_round_trip_inverts_scale() {
        assert_eq!(Priority::from_legacy(1), Priority::Normal);
        assert_eq!(Priority::from_legacy(2), Priority::Medium);
        assert_eq!(Priority::from_legacy(3), Priority::High);
        for p in [Priority::High, Priority::Medium, Priority::Normal] {
            assert_eq!(Priority::from_legacy(p.to_legacy()), p);
        }
    }

    #[test]
    fn test_from_stored_out_of_range_is_normal() {
        assert_eq!(Priority::from_stored(0), Priority::Normal);
        assert_eq!(Priority::from_stored(9), Priority::Normal);
    }

    #[test]
    fn test_display() {
        assert_eq!(Priority::High.to_string(), "p1 (high)");
    }
}
