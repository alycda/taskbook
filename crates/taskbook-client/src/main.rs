use std::process;

use clap::Parser;
use colored::Colorize;

mod api_client;
mod auth;
mod cli;
mod commands;
mod config;
mod credentials;
mod directory;
mod editor;
mod error;
mod mcp;
mod render;
mod storage;
mod taskbook;
mod tui;

fn main() {
    let (args, legacy) = cli::rewrite_legacy_args(std::env::args_os().collect());

    if let Some((flag, subcommand)) = &legacy {
        let warning = "warning:".yellow().bold();
        eprintln!(
            "{warning} `tb {flag}` is deprecated and will be removed in 3.0; use `tb {subcommand}`"
        );
        // A 1.x script's priority numbers now mean the opposite.
        let mentions_priority = subcommand == "priority"
            || args
                .iter()
                .any(|a| a.to_str().is_some_and(|a| a.starts_with("p:")));
        if mentions_priority {
            eprintln!(
                "{warning} priority levels are inverted since 2.0: 1 = high, 2 = medium, 3 = normal"
            );
        }
    }

    let cli = cli::Cli::parse_from(args);

    if let Err(e) = commands::run(cli) {
        eprintln!("\n {} {}\n", "✖".red(), e);
        process::exit(1);
    }
}
