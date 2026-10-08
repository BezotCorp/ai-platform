//! The BCAIP Development Kit (BDK).
//!
//! With default features this crate re-exports the shared BDK wire types from
//! `bcaip-sdk-types` so you can build an Agent Client Protocol (ACP) client
//! that talks to `bcaip acp` over stdio.
//!
//! With `--features uniffi` the crate additionally compiles as a
//! `cdylib`/`staticlib` and exposes an in-process API to Python and Kotlin via
//! [uniffi-rs](https://github.com/mozilla/uniffi-rs). The current uniffi surface
//! lets callers construct declarative providers from JSON and stream provider
//! completions.

#[cfg(feature = "uniffi")]
uniffi::setup_scaffolding!("bcaip");

#[cfg(feature = "uniffi")]
pub mod bindings;

#[cfg(feature = "uniffi")]
pub mod observability;
