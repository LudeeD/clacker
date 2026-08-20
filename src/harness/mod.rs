//! Harnesses we can impersonate a provider for.
//!
//! A harness is just: a binary to find, a protocol to serve it, and the env
//! vars that point it at us instead of the real API.

pub mod claude_code;

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::protocol::Protocol;

pub trait Harness {
    fn id(&self) -> &'static str;

    /// Human-readable name for messages.
    fn name(&self) -> &'static str;

    /// The harness binary, if it's installed.
    fn locate(&self) -> Option<PathBuf>;

    /// The provider API this harness speaks.
    fn protocol(&self) -> Box<dyn Protocol>;

    /// How to launch it against our local server. `self_exe` is clacker's own
    /// path, so the harness can spawn `clacker mcp` for the tools.
    fn command(&self, bin: &Path, addr: SocketAddr, self_exe: &Path) -> Command;
}

pub fn all() -> Vec<Box<dyn Harness>> {
    vec![Box::new(claude_code::ClaudeCode)]
}

pub fn by_id(id: &str) -> Option<Box<dyn Harness>> {
    all().into_iter().find(|h| h.id() == id)
}

/// First installed harness, for the no-arguments case.
pub fn detect() -> Option<(Box<dyn Harness>, PathBuf)> {
    all().into_iter().find_map(|h| h.locate().map(|bin| (h, bin)))
}

/// Look up a bare command name on PATH.
pub fn which(bin: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(bin))
        .find(|candidate| candidate.is_file())
}
