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
    let cli = cli::Cli::parse_from(args);

    if let Some((flag, subcommand)) = legacy {
        eprintln!(
            "{} `tb {flag}` is deprecated and will be removed in 3.0; use `tb {subcommand}`",
            "warning:".yellow().bold(),
        );
    }

    if let Err(e) = commands::run(cli) {
        eprintln!("\n {} {}\n", "✖".red(), e);
        process::exit(1);
    }
}
