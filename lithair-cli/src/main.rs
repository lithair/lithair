//! Lithair CLI — project scaffolding tool.
//!
//! Install with `cargo install lithair-cli`, then run:
//!
//! ```bash
//! lithair new my-app
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
}

fn main() {
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::New { name, no_frontend } => {
            let base = PathBuf::from(".");
            commands::new::run(&name, &base, no_frontend)
        }
    };

    if let Err(e) = result {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}
