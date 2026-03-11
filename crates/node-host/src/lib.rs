//! Headless node host: connects to a gateway as a node and executes commands.
//!
//! Usage: `moltis node run --host <gateway> --token <device-token>`
//!
//! The node host establishes a WebSocket connection to the gateway,
//! authenticates with a device token, and handles `system.run` commands
//! by executing them locally. Nodes can also advertise custom tools loaded
//! from `~/.moltis/node-tools/*.json` — see [`tool_def`] for the format.

pub mod runner;
pub mod service;
pub mod tool_def;
pub mod tool_loader;

pub use {
    runner::{NodeConfig, NodeHost},
    service::ServiceConfig,
    tool_def::{NodeToolDef, NodeToolSchema},
};
