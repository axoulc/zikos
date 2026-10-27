//! `zikos_bridge`: the flutter_rust_bridge surface over the Zikos Rust
//! core. The Flutter app renders and routes only; every piece of domain
//! state and behavior crosses this boundary as a plain Rust type or
//! function (specs/platform-app.md, "UI bound to the Rust core").

pub mod api;
pub mod listing;
pub mod settings;
mod state;

pub use api::files::{NodeKindView, TreeNodeView};
pub use listing::folder_listing;
pub use settings::{SETTINGS_JSON, Settings, SettingsError};

#[allow(clippy::all)]
#[allow(rustdoc::all)]
mod frb_generated;
