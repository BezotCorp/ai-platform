//! Peekaboo helper functions for macOS GUI automation via the Peekaboo CLI.
//!
//! These are used by `ComputerControllerServer` on macOS to auto-install
//! and invoke peekaboo. This module does not expose its own MCP server —
//! peekaboo is accessed through the `computer_control` tool on macOS.

mod peekaboo_installer;

pub use peekaboo_installer::{auto_install_peekaboo, is_peekaboo_installed, resolve_brew};
