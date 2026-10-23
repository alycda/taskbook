use taskbook_common::board::{self, ParsedInput};
use taskbook_common::id::{self, IdError};
use taskbook_common::Priority;

/// Parsed command from the command line input
#[derive(Debug, Clone)]
pub enum ParsedCommand {
    Task {
        /// Empty means "the filtered board, else the default board".
        boards: Vec<String>,
        description: String,
        priority: Priority,
        tags: Vec<String>,
        due_date: Option<i64>,
    },
    Note {
        /// Empty means "the filtered board, else the default board".
        boards: Vec<String>,
        description: String,
        tags: Vec<String>,
    },
    Edit {
        id: u64,
        description: String,
    },
    Move {
        id: u64,
        board: String,
    },
    Delete {
        ids: Vec<u64>,
    },
    Search {
        term: String,
    },
    Priority {
        id: u64,
        level: Priority,
    },
    Check {
        ids: Vec<u64>,
    },
    Star {
        ids: Vec<u64>,
    },
    Begin {
        ids: Vec<u64>,
    },
    Tag {
        id: u64,
        add: Vec<String>,
        remove: Vec<String>,
    },
    Clear,
    RenameBoard {
        old_name: String,
        new_name: String,
    },
    Board,
    Timeline,
    Archive,
    Journal,
    Sort,
    HideDone,
    Help,
    Quit,
}

#[derive(Debug, Clone)]
pub struct ParseError {
    pub message: String,
}

impl ParseError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl From<IdError> for ParseError {
    fn from(e: IdError) -> Self {
        Self::new(e.to_string())
    }
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

/// Parse a command line input into a ParsedCommand
pub fn parse_command(input: &str) -> Result<ParsedCommand, ParseError> {
    let input = input.trim();
    if !input.starts_with('/') {
        return Err(ParseError {
            message: "Commands must start with /".to_string(),
        });
    }

    let parts: Vec<&str> = input[1..].splitn(2, ' ').collect();
    let cmd = parts[0].to_lowercase();
    let args = parts.get(1).copied().unwrap_or("");

    match cmd.as_str() {
        "task" => parse_task(args),
        "note" => parse_note(args),
        "edit" => parse_edit(args),
        "move" => parse_move(args),
        "delete" => parse_id_list(args).map(|ids| ParsedCommand::Delete { ids }),
        "search" => {
            let term = args.trim().to_string();
            if term.is_empty() {
                Err(ParseError {
                    message: "Usage: /search <term>".to_string(),
                })
            } else {
                Ok(ParsedCommand::Search { term })
            }
        }
        "priority" => parse_priority(args),
        "check" => parse_id_list(args).map(|ids| ParsedCommand::Check { ids }),
        "star" => parse_id_list(args).map(|ids| ParsedCommand::Star { ids }),
        "begin" => parse_id_list(args).map(|ids| ParsedCommand::Begin { ids }),
        "tag" => parse_tag(args),
        "clear" => Ok(ParsedCommand::Clear),
        "rename-board" => parse_rename_board(args),
        "board" => Ok(ParsedCommand::Board),
        "timeline" => Ok(ParsedCommand::Timeline),
        "archive" => Ok(ParsedCommand::Archive),
        "journal" => Ok(ParsedCommand::Journal),
        "sort" => Ok(ParsedCommand::Sort),
        "hide-done" => Ok(ParsedCommand::HideDone),
        "help" => Ok(ParsedCommand::Help),
        "quit" | "q" => Ok(ParsedCommand::Quit),
        _ => Err(ParseError {
            message: format!("Unknown command: /{}", cmd),
        }),
    }
}

/// Parse `[@"quoted board"] <item input>` with the shared item grammar
/// (`@board`, `+tag`, `p:N`, `due:DATE`, description words).
fn parse_item_args(args: &str) -> Result<ParsedInput, ParseError> {
    let (quoted, rest) = match extract_at_board(args) {
        Some((name, rest)) if args.trim_start().starts_with("@\"") => (Some(name), rest),
        _ => (None, args),
    };
    let tokens: Vec<&str> = rest.split_whitespace().collect();
    let mut parsed =
        board::parse_item_input(&tokens).map_err(|e| ParseError::new(e.to_string()))?;
    if let Some(name) = quoted {
        parsed.boards.insert(0, name);
    }
    Ok(parsed)
}

fn parse_task(args: &str) -> Result<ParsedCommand, ParseError> {
    if args.trim().is_empty() {
        return Err(ParseError::new(
            "Usage: /task [@board] description [+tag] [p:1-3] [due:YYYY-MM-DD[THH:MM]]",
        ));
    }
    let parsed = parse_item_args(args)?;
    if parsed.description.is_empty() {
        return Err(ParseError::new("Task description cannot be empty"));
    }
    Ok(ParsedCommand::Task {
        boards: parsed.boards,
        description: parsed.description,
        priority: parsed.priority.unwrap_or_default(),
        tags: parsed.tags,
        due_date: parsed.due_date,
    })
}

fn parse_note(args: &str) -> Result<ParsedCommand, ParseError> {
    if args.trim().is_empty() {
        return Err(ParseError::new("Usage: /note [@board] [+tag] title"));
    }
    let parsed = parse_item_args(args)?;
    if parsed.priority.is_some() {
        return Err(ParseError::new("Notes cannot have a priority"));
    }
    if parsed.due_date.is_some() {
        return Err(ParseError::new("Notes cannot have a due date"));
    }
    if parsed.description.is_empty() {
        return Err(ParseError::new("Note title cannot be empty"));
    }
    Ok(ParsedCommand::Note {
        boards: parsed.boards,
        description: parsed.description,
        tags: parsed.tags,
    })
}

/// Split off the first whitespace-separated token as an id.
fn split_id(args: &str, usage: &str) -> Result<(u64, String), ParseError> {
    let args = args.trim();
    let (id_token, rest) = args.split_once(char::is_whitespace).unwrap_or((args, ""));
    if id_token.is_empty() {
        return Err(ParseError::new(usage));
    }
    Ok((id::parse_id(id_token)?, rest.trim().to_string()))
}

fn parse_edit(args: &str) -> Result<ParsedCommand, ParseError> {
    const USAGE: &str = "Usage: /edit <id> <new description>";
    let (id, description) = split_id(args, USAGE)?;
    if description.is_empty() {
        return Err(ParseError::new(USAGE));
    }
    Ok(ParsedCommand::Edit { id, description })
}

fn parse_move(args: &str) -> Result<ParsedCommand, ParseError> {
    const USAGE: &str = "Usage: /move <id> @<board>";
    let (id, rest) = split_id(args, USAGE)?;
    if rest.is_empty() {
        return Err(ParseError::new(USAGE));
    }

    // Board name: @"quoted name", @word, or a bare word
    let board = match extract_at_board(&rest) {
        Some((name, _)) => name,
        None => rest
            .trim_start_matches('@')
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_string(),
    };
    if board.is_empty() {
        return Err(ParseError::new("Board name cannot be empty"));
    }

    Ok(ParsedCommand::Move { id, board })
}

fn parse_priority(args: &str) -> Result<ParsedCommand, ParseError> {
    const USAGE: &str = "Usage: /priority <id> <1-3> (1 = high, 3 = normal)";
    let (id, level) = split_id(args, USAGE)?;
    if level.is_empty() {
        return Err(ParseError::new(USAGE));
    }
    let level = level
        .parse::<Priority>()
        .map_err(|e| ParseError::new(e.to_string()))?;
    Ok(ParsedCommand::Priority { id, level })
}

fn parse_rename_board(args: &str) -> Result<ParsedCommand, ParseError> {
    let args = args.trim();
    if args.is_empty() {
        return Err(ParseError {
            message: "Usage: /rename-board @\"old name\" @\"new name\"".to_string(),
        });
    }

    // Extract old board name
    let (old_name, rest) = if args.starts_with('@') {
        match extract_at_board(args) {
            Some((name, remaining)) => (name, remaining),
            None => {
                return Err(ParseError {
                    message: "Usage: /rename-board @\"old name\" @\"new name\"".to_string(),
                })
            }
        }
    } else {
        let end = args.find(char::is_whitespace).ok_or(ParseError {
            message: "Usage: /rename-board @\"old name\" @\"new name\"".to_string(),
        })?;
        (args[..end].to_string(), &args[end..])
    };

    let rest = rest.trim();

    // Extract new board name
    let new_name = if rest.starts_with('@') {
        match extract_at_board(rest) {
            Some((name, _)) => name,
            None => {
                return Err(ParseError {
                    message: "Board names cannot be empty".to_string(),
                })
            }
        }
    } else {
        rest.to_string()
    };

    if old_name.is_empty() || new_name.is_empty() {
        return Err(ParseError {
            message: "Board names cannot be empty".to_string(),
        });
    }

    Ok(ParsedCommand::RenameBoard { old_name, new_name })
}

/// Extract a board name from input starting with `@`.
///
/// Supports two forms:
/// - `@board` — single word (up to next whitespace)
/// - `@"board name"` — quoted, may contain spaces
///
/// Returns `(board_name, remaining_input)` on success.
fn extract_at_board(input: &str) -> Option<(String, &str)> {
    let input = input.trim_start();
    if !input.starts_with('@') || input.len() < 2 {
        return None;
    }

    let after_at = &input[1..]; // safe: '@' is ASCII

    if let Some(after_quote) = after_at.strip_prefix('"') {
        // Quoted: @"board name"
        if let Some(end_quote) = after_quote.find('"') {
            let board_name = &after_quote[..end_quote];
            let remaining = &after_quote[end_quote + 1..];
            if board_name.is_empty() {
                return None;
            }
            Some((board_name.to_string(), remaining))
        } else {
            // No closing quote — treat rest of string as board name
            if after_quote.is_empty() {
                return None;
            }
            Some((after_quote.to_string(), ""))
        }
    } else {
        // Unquoted: @word
        let end = after_at.find(char::is_whitespace).unwrap_or(after_at.len());
        let board_name = &after_at[..end];
        if board_name.is_empty() {
            return None;
        }
        Some((board_name.to_string(), &after_at[end..]))
    }
}

fn parse_tag(args: &str) -> Result<ParsedCommand, ParseError> {
    const USAGE: &str = "Usage: /tag <id> +tag1 +tag2 -tag3";
    let (id, rest) = split_id(args, USAGE)?;
    if rest.is_empty() {
        return Err(ParseError::new(USAGE));
    }

    let mut add = Vec::new();
    let mut remove = Vec::new();
    for token in rest.split_whitespace() {
        if let Some(tag) = token.strip_prefix('+').filter(|t| !t.is_empty()) {
            add.push(board::normalize_tag(tag));
        } else if let Some(tag) = token.strip_prefix('-').filter(|t| !t.is_empty()) {
            remove.push(board::normalize_tag(tag));
        } else {
            return Err(ParseError::new(format!(
                "Expected +tag or -tag, got '{token}'"
            )));
        }
    }

    Ok(ParsedCommand::Tag { id, add, remove })
}

fn parse_id_list(args: &str) -> Result<Vec<u64>, ParseError> {
    let tokens: Vec<&str> = args.split_whitespace().collect();
    Ok(id::parse_ids(&tokens)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_at_board_unquoted() {
        let (name, rest) = extract_at_board("@coding rest").unwrap();
        assert_eq!(name, "coding");
        assert_eq!(rest.trim(), "rest");
    }

    #[test]
    fn test_extract_at_board_quoted() {
        let (name, rest) = extract_at_board("@\"MiST: IT-Leder\" rest").unwrap();
        assert_eq!(name, "MiST: IT-Leder");
        assert_eq!(rest.trim(), "rest");
    }

    #[test]
    fn test_extract_at_board_quoted_no_remaining() {
        let (name, rest) = extract_at_board("@\"My Board\"").unwrap();
        assert_eq!(name, "My Board");
        assert_eq!(rest, "");
    }

    #[test]
    fn test_extract_at_board_empty_quoted() {
        assert!(extract_at_board("@\"\"").is_none());
    }

    #[test]
    fn test_extract_at_board_no_at() {
        assert!(extract_at_board("coding").is_none());
    }

    #[test]
    fn test_extract_at_board_just_at() {
        assert!(extract_at_board("@").is_none());
    }

    #[test]
    fn test_extract_at_board_unclosed_quote() {
        let (name, rest) = extract_at_board("@\"unclosed board").unwrap();
        assert_eq!(name, "unclosed board");
        assert_eq!(rest, "");
    }

    #[test]
    fn test_parse_task_quoted_board() {
        let result = parse_command("/task @\"MiST: IT-Leder\" Fix the bug").unwrap();
        match result {
            ParsedCommand::Task {
                boards,
                description,
                priority,
                ..
            } => {
                assert_eq!(boards, ["MiST: IT-Leder"]);
                assert_eq!(description, "Fix the bug");
                assert_eq!(priority, Priority::Normal);
            }
            _ => panic!("Expected Task"),
        }
    }

    #[test]
    fn test_parse_task_quoted_board_with_priority() {
        let result = parse_command("/task @\"Dev Ops\" Deploy service p:3").unwrap();
        match result {
            ParsedCommand::Task {
                boards,
                description,
                priority,
                ..
            } => {
                assert_eq!(boards, ["Dev Ops"]);
                assert_eq!(description, "Deploy service");
                assert_eq!(priority, Priority::Normal);
            }
            _ => panic!("Expected Task"),
        }
    }

    #[test]
    fn test_parse_task_unquoted_board() {
        let result = parse_command("/task @coding Fix bug").unwrap();
        match result {
            ParsedCommand::Task {
                boards,
                description,
                ..
            } => {
                assert_eq!(boards, ["coding"]);
                assert_eq!(description, "Fix bug");
            }
            _ => panic!("Expected Task"),
        }
    }

    #[test]
    fn test_parse_note_quoted_board() {
        let result = parse_command("/note @\"MiST: IT-Leder\" Important note").unwrap();
        match result {
            ParsedCommand::Note {
                boards,
                description,
                ..
            } => {
                assert_eq!(boards, ["MiST: IT-Leder"]);
                assert_eq!(description, "Important note");
            }
            _ => panic!("Expected Note"),
        }
    }

    #[test]
    fn test_parse_move_quoted_board() {
        let result = parse_command("/move 1 @\"MiST: IT-Leder\"").unwrap();
        match result {
            ParsedCommand::Move { id, board } => {
                assert_eq!(id, 1);
                assert_eq!(board, "MiST: IT-Leder");
            }
            _ => panic!("Expected Move"),
        }
    }

    #[test]
    fn test_parse_rename_board_quoted() {
        let result = parse_command("/rename-board @\"Old Board\" @\"New Board Name\"").unwrap();
        match result {
            ParsedCommand::RenameBoard { old_name, new_name } => {
                assert_eq!(old_name, "Old Board");
                assert_eq!(new_name, "New Board Name");
            }
            _ => panic!("Expected RenameBoard"),
        }
    }

    #[test]
    fn test_parse_task_with_due_date() {
        let cmd = parse_command("/task Pay rent due:2026-07-01").unwrap();
        match cmd {
            ParsedCommand::Task {
                due_date,
                description,
                ..
            } => {
                assert_eq!(description, "Pay rent");
                assert_eq!(due_date, taskbook_common::due::parse_due_date("2026-07-01"));
            }
            _ => panic!("expected Task"),
        }
    }

    #[test]
    fn test_parse_task_invalid_due_date_errors() {
        let err = parse_command("/task Pay rent due:whenever").unwrap_err();
        assert!(err.message.contains("invalid due date"), "{}", err.message);
    }

    #[test]
    fn test_parse_task_no_board() {
        let result = parse_command("/task Simple task").unwrap();
        match result {
            ParsedCommand::Task {
                boards,
                description,
                ..
            } => {
                assert!(boards.is_empty());
                assert_eq!(description, "Simple task");
            }
            _ => panic!("Expected Task"),
        }
    }

    #[test]
    fn test_ids_are_bare_numbers() {
        assert!(matches!(
            parse_command("/check 1 2").unwrap(),
            ParsedCommand::Check { ids } if ids == [1, 2]
        ));
        let err = parse_command("/check @1").unwrap_err();
        assert!(err.message.contains("use `1`"), "{}", err.message);
        let err = parse_command("/edit @3 new text").unwrap_err();
        assert!(err.message.contains("use `3`"), "{}", err.message);
        assert!(parse_command("/check 1 x").is_err());
    }

    #[test]
    fn test_parse_priority_high_is_one() {
        for input in ["/priority 3 1", "/priority 3 p1", "/priority 3 high"] {
            assert!(matches!(
                parse_command(input).unwrap(),
                ParsedCommand::Priority {
                    id: 3,
                    level: Priority::High
                }
            ));
        }
        assert!(parse_command("/priority 3 4").is_err());
    }

    #[test]
    fn test_parse_task_invalid_priority_errors() {
        assert!(parse_command("/task Fix p:4").is_err());
        let ParsedCommand::Task { priority, .. } = parse_command("/task Fix p:1").unwrap() else {
            panic!("expected Task");
        };
        assert_eq!(priority, Priority::High);
    }

    #[test]
    fn test_parse_task_multiple_boards() {
        let ParsedCommand::Task { boards, .. } =
            parse_command("/task @\"Dev Ops\" @infra Deploy").unwrap()
        else {
            panic!("expected Task");
        };
        assert_eq!(boards, ["Dev Ops", "infra"]);
    }

    #[test]
    fn test_parse_note_rejects_task_fields() {
        assert!(parse_command("/note Idea p:1").is_err());
        assert!(parse_command("/note Idea due:today").is_err());
    }

    #[test]
    fn test_parse_tag_rejects_stray_words() {
        assert!(parse_command("/tag 3 +a b").is_err());
        assert!(matches!(
            parse_command("/tag 3 +A -b").unwrap(),
            ParsedCommand::Tag { id: 3, ref add, ref remove } if add == &["a"] && remove == &["b"]
        ));
    }
}
