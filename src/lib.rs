pub mod amd;
pub mod cleanup;
pub mod completion;
pub mod diagnostics;
pub mod domain;
pub mod download;
pub mod driver_prefetch;
pub mod engine;
pub mod ipc;
pub mod models;
pub mod platform;
pub mod prerequisites;
pub mod progress;
pub mod relocation;
pub mod transfer;
pub mod wim;

pub mod setup_recipe;
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
