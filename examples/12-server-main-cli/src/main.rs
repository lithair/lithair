//! End-to-end compile coverage for `#[server(main, cli)]` (issue #68).
//!
//! There is no hand-written `main()` here: the derive generates the whole
//! binary — the clap `Args` struct, the `Args::parse()` call, the tokio
//! runtime attribute, and the `LithairServer` wiring — reaching clap /
//! tokio / anyhow through `lithair_core::__private` (issue #66).
//!
//! This crate is a workspace member so `cargo build --workspace` compiles
//! the full expansion on every PR. Token-level tests in `lithair-macros`
//! assert what tokens are emitted; they cannot catch call-site resolution
//! failures (e.g. PR #67 dropping the `use ... clap::Parser as _;` import,
//! or #163 deleting the `serve_on_port` fn the macro still emitted). This
//! binary does.

use lithair_macros::DeclarativeModel;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, DeclarativeModel)]
#[server(main, cli, port = 8330)]
pub struct Note {
    #[http(expose)]
    pub id: String,

    #[http(expose)]
    pub title: String,
}
