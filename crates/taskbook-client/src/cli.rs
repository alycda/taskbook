//! Command-line definition (clap) and the 1.x flag compatibility shim.

use std::ffi::OsString;
use std::path::PathBuf;

use clap::{ArgAction, CommandFactory, Parser, Subcommand};

use taskbook_common::id::{self, IdError};

const AFTER_HELP: &str = "\
Item ids are bare numbers (3). @name always names a board.
Priorities: p1 = high, p2 = medium, p3 = normal (the default).
Running tb with no command opens the interactive board.

Examples:
  tb task @coding +urgent Review PR #42 p:1 due:tomorrow+0930
  tb note @coding Mergesort worst case is O(n log n)
  tb show 3
  tb check 1 2
  tb priority 3 4 p1
  tb move 3 @reviews
  tb due 3 2026-07-01
  tb due 3 none
  tb tag 3 +frontend -urgent
  tb edit 3 Merge PR #42
  tb list pending @coding +urgent
  tb find documentation";

#[derive(Parser, Debug)]
#[command(
    name = "tb",
    version,
    about = "Tasks, boards & notes for the command-line habitat",
    after_help = AFTER_HELP,
    disable_version_flag = true
)]
pub struct Cli {
    /// Use a custom taskbook directory
    #[arg(long = "taskbook-dir", value_name = "PATH", global = true)]
    pub taskbook_dir: Option<PathBuf>,

    /// Print version
    #[arg(short = 'v', long, action = ArgAction::Version)]
    version: Option<bool>,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Show items grouped by board
    Board,

    /// Show items grouped by creation date
    Timeline,

    /// Show archived (deleted) items
    Archive,

    /// Show every field of the given items, including note bodies
    Show {
        #[arg(required = true, value_parser = parse_id_arg, value_name = "ID")]
        ids: Vec<u64>,
        /// Look up archive ids (as shown by `tb archive`)
        #[arg(long)]
        archived: bool,
    },

    /// Create a task
    #[command(
        override_usage = "tb task [@BOARD]... [+TAG]... [p:1-3] [due:DATE] <DESCRIPTION>..."
    )]
    Task {
        #[arg(
            required = true,
            trailing_var_arg = true,
            allow_hyphen_values = true,
            value_name = "INPUT"
        )]
        input: Vec<String>,
    },

    /// Create a note (opens $EDITOR when no text is given)
    #[command(override_usage = "tb note [@BOARD]... [+TAG]... [TEXT]...")]
    Note {
        #[arg(
            trailing_var_arg = true,
            allow_hyphen_values = true,
            value_name = "INPUT"
        )]
        input: Vec<String>,
    },

    /// Toggle tasks between done and pending
    Check {
        #[arg(required = true, value_parser = parse_id_arg, value_name = "ID")]
        ids: Vec<u64>,
    },

    /// Toggle tasks between in-progress and pending
    Begin {
        #[arg(required = true, value_parser = parse_id_arg, value_name = "ID")]
        ids: Vec<u64>,
    },

    /// Toggle the star on items
    Star {
        #[arg(required = true, value_parser = parse_id_arg, value_name = "ID")]
        ids: Vec<u64>,
    },

    /// Move items to the archive
    Delete {
        #[arg(required = true, value_parser = parse_id_arg, value_name = "ID")]
        ids: Vec<u64>,
    },

    /// Restore items from the archive (ids as shown by `tb archive`)
    Restore {
        #[arg(required = true, value_parser = parse_id_arg, value_name = "ID")]
        ids: Vec<u64>,
    },

    /// Copy item descriptions to the clipboard
    Copy {
        #[arg(required = true, value_parser = parse_id_arg, value_name = "ID")]
        ids: Vec<u64>,
    },

    /// Replace an item's description
    Edit {
        #[arg(value_parser = parse_id_arg, value_name = "ID")]
        id: u64,
        #[arg(
            required = true,
            trailing_var_arg = true,
            allow_hyphen_values = true,
            value_name = "DESCRIPTION"
        )]
        description: Vec<String>,
    },

    /// Edit a note in $EDITOR
    EditNote {
        #[arg(value_parser = parse_id_arg, value_name = "ID")]
        id: u64,
    },

    /// Put items on the given boards (replacing their current boards)
    #[command(override_usage = "tb move <ID>... <@BOARD>...")]
    Move {
        #[arg(required = true, value_name = "ARGS")]
        args: Vec<String>,
    },

    /// Set task priority (1/high, 2/medium, 3/normal; p1 and p:1 also work)
    #[command(override_usage = "tb priority <ID>... <LEVEL>")]
    Priority {
        #[arg(required = true, value_name = "ARGS")]
        args: Vec<String>,
    },

    /// Set or clear task due dates
    #[command(
        override_usage = "tb due <ID>... <DATE|none>",
        after_help = "DATE: YYYY-MM-DD, \"YYYY-MM-DD HH:MM\", today, tomorrow, now, \
                      or today/tomorrow/now+HHMM"
    )]
    Due {
        #[arg(required = true, value_name = "ARGS")]
        args: Vec<String>,
    },

    /// Add (+tag) or remove (-tag) tags on items
    #[command(override_usage = "tb tag <ID>... <+TAG|-TAG>...")]
    Tag {
        #[arg(required = true, allow_hyphen_values = true, value_name = "ARGS")]
        args: Vec<String>,
    },

    /// Search descriptions, note bodies and tags (any term matches)
    Find {
        #[arg(required = true, allow_hyphen_values = true, value_name = "TERM")]
        terms: Vec<String>,
    },

    /// Show items matching every filter
    #[command(
        after_help = "FILTER: pending, progress, done, task, note, starred, +tag, @board \
                      (or a bare board name)"
    )]
    List {
        #[arg(value_name = "FILTER")]
        filters: Vec<String>,
    },

    /// Archive all completed tasks
    Clear,

    /// Register a new sync server account
    Register {
        #[arg(long)]
        server: Option<String>,
        #[arg(long)]
        username: Option<String>,
        #[arg(long)]
        email: Option<String>,
        #[arg(long)]
        password: Option<String>,
    },

    /// Log in to a sync server account
    Login {
        #[arg(long)]
        server: Option<String>,
        #[arg(long)]
        username: Option<String>,
        #[arg(long)]
        password: Option<String>,
        /// Encryption key (base64)
        #[arg(long)]
        key: Option<String>,
    },

    /// Log out and delete stored credentials
    Logout,

    /// Show sync status
    Status,

    /// Push local data to the sync server
    Migrate,

    /// Run as an MCP stdio server (for Claude Code and other agents)
    Mcp,
}

fn parse_id_arg(s: &str) -> Result<u64, String> {
    id::parse_id(s).map_err(|e| match e {
        IdError::AtPrefixed(t) => format!(
            "ids are bare numbers and @ names a board; use `{}`",
            &t[1..]
        ),
        _ => "ids are numbers".to_string(),
    })
}

/// 1.x action flags and the subcommand each one became.
const LEGACY_FLAGS: &[(&str, Option<&str>, &str)] = &[
    ("--archive", Some("-a"), "archive"),
    ("--begin", Some("-b"), "begin"),
    ("--check", Some("-c"), "check"),
    ("--clear", None, "clear"),
    ("--copy", Some("-y"), "copy"),
    ("--delete", Some("-d"), "delete"),
    ("--due", None, "due"),
    ("--edit", Some("-e"), "edit"),
    ("--edit-note", None, "edit-note"),
    ("--find", Some("-f"), "find"),
    ("--list", Some("-l"), "list"),
    ("--mcp", None, "mcp"),
    ("--move", Some("-m"), "move"),
    ("--note", Some("-n"), "note"),
    ("--priority", Some("-p"), "priority"),
    ("--restore", Some("-r"), "restore"),
    ("--star", Some("-s"), "star"),
    ("--tag", None, "tag"),
    ("--task", Some("-t"), "task"),
    ("--timeline", Some("-i"), "timeline"),
    ("--register", None, "register"),
    ("--login", None, "login"),
    ("--logout", None, "logout"),
    ("--status", None, "status"),
    ("--migrate", None, "migrate"),
];

fn legacy_subcommand(arg: &str) -> Option<&'static str> {
    LEGACY_FLAGS
        .iter()
        .find(|(long, short, _)| arg == *long || Some(arg) == *short)
        .map(|(_, _, sub)| *sub)
}

/// Rewrite a 1.x flag-style invocation (`tb --check 1`, `tb -t @work x`,
/// `tb --cli`) into subcommand form (`tb check 1`, `tb task @work x`,
/// `tb board`).
///
/// Returns the rewritten arguments plus, when a legacy flag was replaced, the
/// `(flag, subcommand)` pair so the caller can print a deprecation notice.
/// Arguments that already name a subcommand are returned unchanged.
pub fn rewrite_legacy_args(
    mut args: Vec<OsString>,
) -> (Vec<OsString>, Option<(String, &'static str)>) {
    // Skip leading global options; drop the 1.x `--cli` switch.
    let mut start = 1;
    let mut saw_cli = false;
    while let Some(arg) = args.get(start).and_then(|a| a.to_str()) {
        if arg == "--taskbook-dir" {
            start += 2;
        } else if arg.starts_with("--taskbook-dir=") {
            start += 1;
        } else if arg == "--cli" {
            args.remove(start);
            saw_cli = true;
        } else {
            break;
        }
    }

    let first = args.get(start).and_then(|a| a.to_str()).unwrap_or("");
    let is_subcommand = Cli::command()
        .get_subcommands()
        .any(|sub| sub.get_name() == first);
    if !is_subcommand {
        let legacy = args[start.min(args.len())..]
            .iter()
            .take_while(|a| *a != "--")
            .position(|a| a.to_str().and_then(legacy_subcommand).is_some());
        if let Some(offset) = legacy {
            let flag = args.remove(start + offset);
            let flag = flag.to_string_lossy().into_owned();
            let sub = legacy_subcommand(&flag).expect("matched above");
            args.insert(start, sub.into());
            return (args, Some((flag, sub)));
        }
    }

    if saw_cli {
        if args.len() <= start {
            args.push("board".into());
        }
        return (args, Some(("--cli".to_string(), "board")));
    }
    (args, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rewrite(args: &[&str]) -> (Vec<String>, Option<String>) {
        let (out, legacy) = rewrite_legacy_args(args.iter().map(OsString::from).collect());
        (
            out.into_iter().map(|a| a.into_string().unwrap()).collect(),
            legacy.map(|(flag, _)| flag),
        )
    }

    fn parse(args: &[&str]) -> Result<Cli, clap::Error> {
        Cli::try_parse_from(args)
    }

    #[test]
    fn test_clap_definition_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn test_rewrite_flag_to_subcommand() {
        assert_eq!(
            rewrite(&["tb", "--check", "1", "2"]),
            (
                vec!["tb".into(), "check".into(), "1".into(), "2".into()],
                Some("--check".into())
            )
        );
        assert_eq!(rewrite(&["tb", "-t", "x"]).0, ["tb", "task", "x"]);
    }

    #[test]
    fn test_rewrite_keeps_global_dir_in_front() {
        assert_eq!(
            rewrite(&["tb", "--taskbook-dir", "/d", "--star", "1"]).0,
            ["tb", "--taskbook-dir", "/d", "star", "1"]
        );
    }

    #[test]
    fn test_rewrite_finds_flag_after_options() {
        assert_eq!(
            rewrite(&["tb", "--server", "u", "--register"]).0,
            ["tb", "register", "--server", "u"]
        );
    }

    #[test]
    fn test_rewrite_leaves_subcommands_alone() {
        assert_eq!(
            rewrite(&["tb", "task", "use", "-c", "flag"]),
            (
                vec![
                    "tb".into(),
                    "task".into(),
                    "use".into(),
                    "-c".into(),
                    "flag".into()
                ],
                None
            )
        );
    }

    #[test]
    fn test_rewrite_cli_flag() {
        assert_eq!(
            rewrite(&["tb", "--cli"]),
            (vec!["tb".into(), "board".into()], Some("--cli".into()))
        );
        assert_eq!(
            rewrite(&["tb", "--cli", "--list", "done"]).0,
            ["tb", "list", "done"]
        );
    }

    #[test]
    fn test_ids_reject_at_prefix_with_hint() {
        let err = parse(&["tb", "check", "@3"]).unwrap_err().to_string();
        assert!(err.contains("use `3`"), "{err}");
        let err = parse(&["tb", "edit", "@3", "x"]).unwrap_err().to_string();
        assert!(err.contains("use `3`"), "{err}");
    }

    #[test]
    fn test_ids_reject_garbage() {
        assert!(parse(&["tb", "check", "1", "foo"]).is_err());
        assert!(parse(&["tb", "check"]).is_err());
    }

    #[test]
    fn test_hyphen_values_reach_commands() {
        let cli = parse(&["tb", "tag", "3", "-urgent"]).unwrap();
        assert!(
            matches!(cli.command, Some(Command::Tag { ref args }) if args == &["3", "-urgent"])
        );
        let cli = parse(&["tb", "task", "fix", "-v", "flag"]).unwrap();
        assert!(
            matches!(cli.command, Some(Command::Task { ref input }) if input == &["fix", "-v", "flag"])
        );
    }

    #[test]
    fn test_show_parses_ids_and_archive_flag() {
        let cli = parse(&["tb", "show", "3", "4", "--archived"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(Command::Show { ref ids, archived: true }) if ids == &[3, 4]
        ));
    }

    #[test]
    fn test_bare_words_are_not_a_command() {
        assert!(parse(&["tb", "buy", "milk"]).is_err());
    }

    #[test]
    fn test_no_command_means_tui() {
        assert!(parse(&["tb"]).unwrap().command.is_none());
    }
}
