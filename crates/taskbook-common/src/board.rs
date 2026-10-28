//! Centralized board name handling.
//!
//! Board names are stored **without** the `@` prefix. The `@` is added back
//! only at the display layer via [`display_name`].

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

/// Structured result of parsing CLI create input.
#[derive(Debug)]
pub struct ParsedInput {
    pub boards: Vec<String>,
    pub description: String,
    pub priority: u8,
    pub tags: Vec<String>,
    /// Due date as local-midnight epoch millis (from a `due:` token).
    pub due_date: Option<i64>,
}

/// Errors from parsing CLI create input.
#[derive(Debug, PartialEq, Eq)]
pub enum CliParseError {
    /// The value after `due:` was not a valid date.
    InvalidDueDate(String),
}

/// Parse CLI input words into a [`ParsedInput`].
///
/// Words starting with `@` (and longer than 1 char) are treated as board names.
/// Words starting with `+` (and longer than 1 char) are treated as tags.
/// Words matching `p:1`, `p:2`, `p:3` set priority.
/// Words starting with `due:` set the due date (`YYYY-MM-DD`, `YYYY-MM-DDTHH:MM`,
/// `today`, `tomorrow`, `now`, or `today+HHMM` / `tomorrow+HHMM` / `now+HHMM`).
/// (A `due:` token cannot contain a space, so use the `T` separator for a time.)
/// Everything else is the description.
///
/// If no boards are found, defaults to [`DEFAULT_BOARD`].
pub fn parse_cli_input(input: &[String]) -> Result<ParsedInput, CliParseError> {
    let mut boards = Vec::new();
    let mut tags = Vec::new();
    let mut desc = Vec::new();
    let mut priority: u8 = 1;
    let mut due_date: Option<i64> = None;

    for word in input {
        if is_priority_opt(word) {
            if let Ok(p) = word.trim_start_matches("p:").parse::<u8>() {
                priority = p;
            }
        } else if let Some(value) = word.strip_prefix("due:") {
            match crate::due::parse_due_date(value) {
                Some(millis) => due_date = Some(millis),
                None => return Err(CliParseError::InvalidDueDate(value.to_string())),
            }
        } else if word.starts_with('@') && word.len() > 1 {
            boards.push(normalize_board_name(word));
        } else if word.starts_with('+') && word.len() > 1 {
            let tag = normalize_tag(word);
            if !tags.iter().any(|t: &String| t.eq_ignore_ascii_case(&tag)) {
                tags.push(tag);
            }
        } else {
            desc.push(word.clone());
        }
    }

    if boards.is_empty() {
        boards.push(DEFAULT_BOARD.to_string());
    }

    // Deduplicate boards (case-insensitive)
    let mut deduped: Vec<String> = Vec::new();
    for board in boards {
        if !deduped.iter().any(|b| board_eq(b, &board)) {
            deduped.push(board);
        }
    }

    Ok(ParsedInput {
        boards: deduped,
        description: desc.join(" "),
        priority,
        tags,
        due_date,
    })
}

/// Normalize a raw tag name: strip leading `+`, trim whitespace, lowercase.
pub fn normalize_tag(raw: &str) -> String {
    raw.trim().trim_start_matches('+').trim().to_lowercase()
}

/// Format a tag for display with `+` prefix.
pub fn display_tag(tag: &str) -> String {
    format!("+{}", tag)
}

/// Split words into description words and normalized `+tag` tags.
///
/// Uses the same rule as [`parse_cli_input`]: a word is a tag when it starts
/// with `+` and is longer than 1 char. Tags are deduplicated case-insensitively.
pub fn extract_tags<'a>(words: impl IntoIterator<Item = &'a str>) -> (Vec<&'a str>, Vec<String>) {
    let mut desc = Vec::new();
    let mut tags: Vec<String> = Vec::new();
    for word in words {
        if word.starts_with('+') && word.len() > 1 {
            let tag = normalize_tag(word);
            if !contains_tag(&tags, &tag) {
                tags.push(tag);
            }
        } else {
            desc.push(word);
        }
    }
    (desc, tags)
}

/// Outcome of applying tag additions and removals to an item's tag list.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct TagChanges {
    /// The item's tag list after the change.
    pub tags: Vec<String>,
    /// Tags that were not on the item before and are now.
    pub added: Vec<String>,
    /// Tags that were on the item before and are not now.
    pub removed: Vec<String>,
    /// Requested removals that were not on the item to begin with.
    pub not_found: Vec<String>,
}

/// Apply `remove`, then `add`, to `current`, comparing tags case-insensitively.
///
/// `added` and `removed` are computed against `current`, so a tag that is
/// both removed and re-added is reported as neither.
pub fn apply_tag_changes(current: &[String], add: &[String], remove: &[String]) -> TagChanges {
    let mut tags: Vec<String> = current
        .iter()
        .filter(|t| !contains_tag(remove, t))
        .cloned()
        .collect();
    for tag in add {
        if !contains_tag(&tags, tag) {
            tags.push(tag.clone());
        }
    }

    let added = tags
        .iter()
        .filter(|t| !contains_tag(current, t))
        .cloned()
        .collect();
    let removed = current
        .iter()
        .filter(|t| !contains_tag(&tags, t))
        .cloned()
        .collect();
    let mut not_found: Vec<String> = Vec::new();
    for tag in remove {
        if !contains_tag(current, tag) && !contains_tag(&not_found, tag) {
            not_found.push(tag.clone());
        }
    }

    TagChanges {
        tags,
        added,
        removed,
        not_found,
    }
}

fn contains_tag(tags: &[String], tag: &str) -> bool {
    tags.iter().any(|t| t.eq_ignore_ascii_case(tag))
}

fn is_priority_opt(s: &str) -> bool {
    matches!(s, "p:1" | "p:2" | "p:3")
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
        assert_eq!(parsed.priority, 1);
        assert!(parsed.tags.is_empty());
        assert_eq!(parsed.due_date, None);
    }

    #[test]
    fn test_parse_cli_input_with_priority() {
        let input: Vec<String> = vec!["@coding".into(), "Fix".into(), "bug".into(), "p:3".into()];
        let parsed = parse_cli_input(&input).unwrap();
        assert_eq!(parsed.boards, vec!["coding"]);
        assert_eq!(parsed.description, "Fix bug");
        assert_eq!(parsed.priority, 3);
    }

    #[test]
    fn test_parse_cli_input_no_board_defaults() {
        let input: Vec<String> = vec!["Simple".into(), "task".into()];
        let parsed = parse_cli_input(&input).unwrap();
        assert_eq!(parsed.boards, vec![DEFAULT_BOARD]);
        assert_eq!(parsed.description, "Simple task");
        assert_eq!(parsed.priority, 1);
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
            assert_eq!(parsed.priority, p, "expected priority {p}");
        }
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

    #[test]
    fn test_extract_tags() {
        let (desc, tags) = extract_tags("fix the +Bug in C++ +bug + +ui".split_whitespace());
        assert_eq!(desc, vec!["fix", "the", "in", "C++", "+"]);
        assert_eq!(tags, vec!["bug", "ui"]);
    }

    fn strings(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn test_apply_tag_changes_add_and_remove() {
        let changes = apply_tag_changes(
            &strings(&["urgent", "frontend"]),
            &strings(&["backend", "frontend"]),
            &strings(&["urgent"]),
        );
        assert_eq!(changes.tags, strings(&["frontend", "backend"]));
        assert_eq!(changes.added, strings(&["backend"]));
        assert_eq!(changes.removed, strings(&["urgent"]));
        assert!(changes.not_found.is_empty());
    }

    #[test]
    fn test_apply_tag_changes_reports_missing_removals() {
        let changes = apply_tag_changes(
            &strings(&["urgent"]),
            &[],
            &strings(&["nope", "URGENT", "nope"]),
        );
        assert!(changes.tags.is_empty());
        assert_eq!(changes.removed, strings(&["urgent"]));
        assert_eq!(changes.not_found, strings(&["nope"]));
    }

    #[test]
    fn test_apply_tag_changes_noop() {
        let changes = apply_tag_changes(
            &strings(&["urgent"]),
            &strings(&["urgent"]),
            &strings(&["urgent"]),
        );
        assert_eq!(changes.tags, strings(&["urgent"]));
        assert!(changes.added.is_empty());
        assert!(changes.removed.is_empty());
        assert!(changes.not_found.is_empty());
    }
}
