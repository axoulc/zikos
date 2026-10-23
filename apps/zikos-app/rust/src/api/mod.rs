//! The flutter_rust_bridge surface. Functions here are thin: they lock
//! the global [`crate::state::AppState`], delegate, and map errors to
//! strings. View structs live next to the functions that emit them so
//! codegen mirrors them.

pub mod app;
pub mod conversations;
pub mod files;
pub mod recorder;
pub mod settings;
