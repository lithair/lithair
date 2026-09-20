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
    /// Experimental offline cluster configuration and storage tools.
    #[cfg(feature = "cluster-ops")]
    Cluster {
        #[command(subcommand)]
        command: ClusterCommand,
    },
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

#[cfg(feature = "cluster-ops")]
#[derive(Subcommand)]
enum ClusterCommand {
    /// Validate enrollment and local TLS material without changing storage.
    Check {
        #[arg(long)]
        config: PathBuf,
    },
    /// Explicitly create an empty, identity-bound consensus store.
    Provision {
        #[arg(long)]
        config: PathBuf,
    },
    /// Validate an offline consensus store without repairing or cleaning it.
    Inspect {
        #[arg(long)]
        config: PathBuf,
    },
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        #[cfg(feature = "cluster-ops")]
        Commands::Cluster { command } => {
            use lithair_core::cluster::operator::{run, OperatorCommand};
            let (command, config) = match command {
                ClusterCommand::Check { config } => (OperatorCommand::Check, config),
                ClusterCommand::Provision { config } => (OperatorCommand::Provision, config),
                ClusterCommand::Inspect { config } => (OperatorCommand::Inspect, config),
            };
            let result = tokio::runtime::Runtime::new()
                .map_err(|error| error.to_string())
                .and_then(|runtime| {
                    runtime.block_on(run(command, config)).map_err(|error| format!("{error:#}"))
                })
                .and_then(|report| {
                    serde_json::to_string_pretty(&report).map_err(|error| error.to_string())
                });
            match result {
                Ok(report) => println!("{report}"),
                Err(error) => {
                    eprintln!("cluster: {error}");
                    std::process::exit(2);
                }
            }
        }
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
