//! Lithair CLI — project scaffolding tool.
//!
//! Install with `cargo install lithair-cli`, then run:
//!
//! ```bash
//! lithair new my-app
//! ```
//!
//! Verify a restored backup's integrity before starting the server:
//!
//! ```bash
//! lithair verify ./data
//! ```
//!
//! See `lithair --help` for all available commands and options.

mod commands;
mod templates;

use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "lithair",
    about = "Lithair project scaffolding tool",
    version,
    after_help = "See https://github.com/lithair/lithair for full documentation."
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Create a new Lithair project
    New {
        /// Project name (used as directory name and Cargo package name)
        name: String,

        /// Skip generating the frontend/ directory
        #[arg(long)]
        no_frontend: bool,
    },

    /// Verify the event-store hash chain of a data directory (offline).
    ///
    /// Run this against a restored backup before starting the server. Exits
    /// 0 if the chain is valid, 1 if tampering/corruption is detected, 2 if
    /// the store cannot be opened.
    Verify {
        /// Path to the event-store data directory (the one containing
        /// events.raftlog).
        data_dir: PathBuf,
    },
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::New { name, no_frontend } => {
            let base = PathBuf::from(".");
            if let Err(e) = commands::new::run(&name, &base, no_frontend) {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }
        Commands::Verify { data_dir } => {
            // `verify` owns its exit code (0 valid, 1 invalid, 2 unreadable)
            // so it is scriptable in restore drills.
            std::process::exit(commands::verify::run(&data_dir));
        }
    }
}
