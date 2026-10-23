//! Centralized board name handling.
//!
//! Board names are stored **without** the `@` prefix. The `@` is added back
//! only at the display layer via [`display_name`].

use crate::due;
use crate::priority::{ParsePriorityError, Priority};

/// The default board name used when no board is specified.
pub const DEFAULT_BOARD: &str = "My Board";

/// Normalize a raw board name to its canonical stored form.
///
/// - Strips leading `@`
/// - Trims whitespace
/// - Maps the alias `myboard` (case-insensitive) to [`DEFAULT_BOARD`]
pub fn normalize_board_name(raw: &str) -> String {
    // First trim removes outer whitespace; trim_start_matches strips the '@' prefix;
    // second trim handles whitespace between '@' and the board name (e.g. "@  board").
    let trimmed = raw.trim().trim_start_matches('@').trim();
    if trimmed.is_empty()
        || trimmed.eq_ignore_ascii_case("myboard")
        || trimmed.eq_ignore_ascii_case("my board")
    {
        DEFAULT_BOARD.to_string()
    } else {
        trimmed.to_string()
    }
}

/// Case-insensitive board comparison.
pub fn board_eq(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

/// Format a board name for display.
///
/// The default board is shown as-is (`My Board`). All other boards get an `@` prefix.
pub fn display_name(board: &str) -> String {
    if board_eq(board, DEFAULT_BOARD) {
        DEFAULT_BOARD.to_string()
    } else {
        format!("@{}", board)
    }
}

/// Structured result of parsing item-creation input.
#[derive(Debug)]
pub struct ParsedInput {
    /// Boards named with `@`. Empty from [`parse_item_input`] when none were
    /// given; [`parse_cli_input`] fills in [`DEFAULT_BOARD`].
    pub boards: Vec<String>,
    pub description: String,
    /// Priority from a `p:` token, if one was given.
    pub priority: Option<Priority>,
    pub tags: Vec<String>,
    /// Due date as epoch millis (from a `due:` token).
    pub due_date: Option<i64>,
}

/// Errors from parsing item-creation input.
#[derive(Debug, PartialEq, Eq)]
pub enum CliParseError {
    /// The value after `due:` was not a valid date.
    InvalidDueDate(String),
    /// A `p:` token did not name a valid priority.
    InvalidPriority(ParsePriorityError),
}

impl std::fmt::Display for CliParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CliParseError::InvalidDueDate(raw) => {
                write!(f, "{}", due::invalid_due_date_message(raw))
            }
            CliParseError::InvalidPriority(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for CliParseError {}

/// Parse item-creation words into a [`ParsedInput`], leaving `boards` empty
/// when none are named.
///
/// - `@name` (longer than 1 char) names a board.
/// - `+name` (longer than 1 char) adds a tag.
/// - `p:N` sets priority (`p:1` = high, `p:2` = medium, `p:3` = normal; the
///   labels `p:high` etc. also work). Any other `p:` value is an error.
/// - `due:VALUE` sets the due date (`YYYY-MM-DD`, `YYYY-MM-DDTHH:MM`,
///   `today`, `tomorrow`, `now`, or `today+HHMM` / `tomorrow+HHMM` /
///   `now+HHMM`). A `due:` token cannot contain a space, so use the `T`
///   separator for a time.
/// - Everything else is the description.
pub fn parse_item_input<S: AsRef<str>>(input: &[S]) -> Result<ParsedInput, CliParseError> {
    let mut boards: Vec<String> = Vec::new();
    let mut tags: Vec<String> = Vec::new();
    let mut desc = Vec::new();
    let mut priority = None;
    let mut due_date: Option<i64> = None;

    for word in input {
        let word = word.as_ref();
        if word.starts_with("p:") {
            priority = Some(word.parse().map_err(CliParseError::InvalidPriority)?);
        } else if let Some(value) = word.strip_prefix("due:") {
            match due::parse_due_date(value) {
                Some(millis) => due_date = Some(millis),
                None => return Err(CliParseError::InvalidDueDate(value.to_string())),
            }
        } else if word.starts_with('@') && word.len() > 1 {
            let board = normalize_board_name(word);
            if !boards.iter().any(|b| board_eq(b, &board)) {
                boards.push(board);
            }
        } else if word.starts_with('+') && word.len() > 1 {
            let tag = normalize_tag(word);
            if !tags.iter().any(|t| t.eq_ignore_ascii_case(&tag)) {
                tags.push(tag);
            }
        } else {
            desc.push(word);
        }
    }

    Ok(ParsedInput {
        boards,
        description: desc.join(" "),
        priority,
        tags,
        due_date,
    })
}

/// Like [`parse_item_input`], defaulting to [`DEFAULT_BOARD`] when no board
/// is named.
pub fn parse_cli_input<S: AsRef<str>>(input: &[S]) -> Result<ParsedInput, CliParseError> {
    let mut parsed = parse_item_input(input)?;
    if parsed.boards.is_empty() {
        parsed.boards.push(DEFAULT_BOARD.to_string());
    }
    Ok(parsed)
}

/// Normalize a raw tag name: strip leading `+`, trim whitespace, lowercase.
pub fn normalize_tag(raw: &str) -> String {
    raw.trim().trim_start_matches('+').trim().to_lowercase()
}

/// Format a tag for display with `+` prefix.
pub fn display_tag(tag: &str) -> String {
    format!("+{}", tag)
}

/// Deserialize a list of board names, normalizing each one.
///
/// Used as `#[serde(deserialize_with = "...")]` on the `boards` field
/// in Task and Note structs to transparently migrate old `@`-prefixed names.
pub fn deserialize_boards<'de, D>(deserializer: D) -> std::result::Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize;
    let raw: Vec<String> = Vec::deserialize(deserializer)?;
    Ok(raw.into_iter().map(|b| normalize_board_name(&b)).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_strips_at_prefix() {
        assert_eq!(normalize_board_name("@coding"), "coding");
        assert_eq!(normalize_board_name("@@double"), "double");
    }

    #[test]
    fn test_normalize_trims_whitespace() {
        assert_eq!(normalize_board_name("  @coding  "), "coding");
        assert_eq!(normalize_board_name("  coding  "), "coding");
    }

    #[test]
    fn test_normalize_myboard_alias() {
        assert_eq!(normalize_board_name("myboard"), DEFAULT_BOARD);
        assert_eq!(normalize_board_name("MyBoard"), DEFAULT_BOARD);
        assert_eq!(normalize_board_name("MYBOARD"), DEFAULT_BOARD);
        assert_eq!(normalize_board_name("My Board"), DEFAULT_BOARD);
        assert_eq!(normalize_board_name("@myboard"), DEFAULT_BOARD);
    }

    #[test]
    fn test_normalize_empty_returns_default() {
        assert_eq!(normalize_board_name(""), DEFAULT_BOARD);
        assert_eq!(normalize_board_name("   "), DEFAULT_BOARD);
        assert_eq!(normalize_board_name("@"), DEFAULT_BOARD);
    }

    #[test]
    fn test_board_eq_case_insensitive() {
        assert!(board_eq("coding", "Coding"));
        assert!(board_eq("coding", "CODING"));
        assert!(board_eq("My Board", "my board"));
        assert!(!board_eq("coding", "reviews"));
    }

    #[test]
    fn test_display_name_default_board() {
        assert_eq!(display_name(DEFAULT_BOARD), "My Board");
    }

    #[test]
    fn test_display_name_other_boards() {
        assert_eq!(display_name("coding"), "@coding");
        assert_eq!(display_name("reviews"), "@reviews");
    }

    #[test]
    fn test_parse_cli_input_basic() {
        let input: Vec<String> = vec!["@coding".into(), "Fix".into(), "bug".into()];
        let parsed = parse_cli_input(&input).unwrap();
        assert_eq!(parsed.boards, vec!["coding"]);
        assert_eq!(parsed.description, "Fix bug");
        assert_eq!(parsed.priority, None);
        assert!(parsed.tags.is_empty());
        assert_eq!(parsed.due_date, None);
    }

    #[test]
    fn test_parse_cli_input_with_priority() {
        let input: Vec<String> = vec!["@coding".into(), "Fix".into(), "bug".into(), "p:3".into()];
        let parsed = parse_cli_input(&input).unwrap();
        assert_eq!(parsed.boards, vec!["coding"]);
        assert_eq!(parsed.description, "Fix bug");
        assert_eq!(parsed.priority, Some(Priority::Normal));
    }

    #[test]
    fn test_parse_cli_input_no_board_defaults() {
        let input: Vec<String> = vec!["Simple".into(), "task".into()];
        let parsed = parse_cli_input(&input).unwrap();
        assert_eq!(parsed.boards, vec![DEFAULT_BOARD]);
        assert_eq!(parsed.description, "Simple task");
        assert_eq!(parsed.priority, None);
    }

    #[test]
    fn test_parse_cli_input_dedup_boards() {
        let input: Vec<String> = vec!["@coding".into(), "@Coding".into(), "task".into()];
        let parsed = parse_cli_input(&input).unwrap();
        assert_eq!(parsed.boards, vec!["coding"]);
        assert_eq!(parsed.description, "task");
    }

    #[test]
    fn test_parse_cli_input_priority_parsing() {
        for p in 1..=3u8 {
            let input: Vec<String> = vec!["task".into(), format!("p:{p}")];
            let parsed = parse_cli_input(&input).unwrap();
            assert_eq!(
                parsed.priority,
                Priority::from_level(p),
                "expected priority {p}"
            );
        }
        let parsed = parse_cli_input(&["task", "p:high"]).unwrap();
        assert_eq!(parsed.priority, Some(Priority::High));
    }

    #[test]
    fn test_parse_cli_input_invalid_priority_errors() {
        for bad in ["p:4", "p:0", "p:", "p:urgent"] {
            let err = parse_cli_input(&["task", bad]).unwrap_err();
            assert!(
                matches!(err, CliParseError::InvalidPriority(_)),
                "{bad} should be rejected"
            );
        }
    }

    #[test]
    fn test_parse_item_input_leaves_boards_empty() {
        let parsed = parse_item_input(&["task"]).unwrap();
        assert!(parsed.boards.is_empty());
    }

    #[test]
    fn test_parse_cli_input_multiple_boards() {
        let input: Vec<String> = vec!["@coding".into(), "@reviews".into(), "task".into()];
        let parsed = parse_cli_input(&input).unwrap();
        assert_eq!(parsed.boards, vec!["coding", "reviews"]);
        assert_eq!(parsed.description, "task");
    }

    #[test]
    fn test_parse_cli_input_with_tags() {
        let input: Vec<String> = vec![
            "@coding".into(),
            "+urgent".into(),
            "+frontend".into(),
            "Fix".into(),
            "login".into(),
            "bug".into(),
        ];
        let parsed = parse_cli_input(&input).unwrap();
        assert_eq!(parsed.boards, vec!["coding"]);
        assert_eq!(parsed.description, "Fix login bug");
        assert_eq!(parsed.tags, vec!["urgent", "frontend"]);
    }

    #[test]
    fn test_parse_cli_input_dedup_tags() {
        let input: Vec<String> = vec!["+urgent".into(), "+Urgent".into(), "task".into()];
        let parsed = parse_cli_input(&input).unwrap();
        assert_eq!(parsed.tags, vec!["urgent"]);
    }

    #[test]
    fn test_parse_cli_input_with_due_date() {
        let input: Vec<String> = vec!["Pay".into(), "rent".into(), "due:2026-07-01".into()];
        let parsed = parse_cli_input(&input).unwrap();
        assert_eq!(parsed.description, "Pay rent");
        assert_eq!(
            parsed.due_date,
            Some(crate::due::parse_due_date("2026-07-01").unwrap())
        );
    }

    #[test]
    fn test_parse_cli_input_with_due_date_and_time() {
        // A `due:` token carries a time via the `T` separator (no spaces).
        let input: Vec<String> = vec!["Call".into(), "vet".into(), "due:2026-07-01T14:30".into()];
        let parsed = parse_cli_input(&input).unwrap();
        assert_eq!(parsed.description, "Call vet");
        assert_eq!(
            parsed.due_date,
            Some(crate::due::parse_due_date("2026-07-01T14:30").unwrap())
        );
    }

    #[test]
    fn test_parse_cli_input_invalid_due_date_errors() {
        let input: Vec<String> = vec!["task".into(), "due:soon".into()];
        assert_eq!(
            parse_cli_input(&input).unwrap_err(),
            CliParseError::InvalidDueDate("soon".to_string())
        );
    }

    #[test]
    fn test_normalize_tag() {
        assert_eq!(normalize_tag("+urgent"), "urgent");
        assert_eq!(normalize_tag("+FrontEnd"), "frontend");
        assert_eq!(normalize_tag("  +spaced  "), "spaced");
    }

    #[test]
    fn test_display_tag() {
        assert_eq!(display_tag("urgent"), "+urgent");
    }
}
