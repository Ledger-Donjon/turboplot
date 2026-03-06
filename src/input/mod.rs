//! Input handling: application configuration, file selection, and trace loading.

mod args;
pub mod loading;

#[cfg(not(target_arch = "wasm32"))]
mod file_manager;
#[cfg(target_arch = "wasm32")]
mod web_file_manager;

pub use args::Args;
#[cfg(target_arch = "wasm32")]
pub use args::parse_url_params;
pub use loading::LoadResult;

#[cfg(not(target_arch = "wasm32"))]
pub use file_manager::FileManager;
#[cfg(target_arch = "wasm32")]
pub use web_file_manager::WebFileManager;
