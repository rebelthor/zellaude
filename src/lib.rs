//! Pure, host-independent logic shared with the plugin binary.
//!
//! The plugin binary links wasm-only zellij host imports, so it cannot be built
//! for the host target and its logic cannot be unit-tested directly. Anything
//! that is pure decision-making lives here instead, where `cargo test` can
//! reach it without a live zellij.

pub mod rename_guard;
