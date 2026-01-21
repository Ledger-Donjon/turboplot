//! Input handling: CLI arguments and file selection.

mod args;
mod file_manager;

pub use args::Args;
pub use file_manager::{FileManager, FileManagerResult};
