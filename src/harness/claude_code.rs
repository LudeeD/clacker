use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::json;

use super::{Harness, which};
use crate::protocol::{Protocol, anthropic};

pub struct ClaudeCode;

impl Harness for ClaudeCode {
    fn id(&self) -> &'static str {
        "claude"
    }

    fn name(&self) -> &'static str {
        "Claude Code"
    }

    fn locate(&self) -> Option<PathBuf> {
        which("claude")
    }

    fn protocol(&self) -> Box<dyn Protocol> {
        Box::new(anthropic::Anthropic)
    }

    fn command(&self, bin: &Path, addr: SocketAddr, self_exe: &Path) -> Command {
        let mcp_config = json!({
            "mcpServers": {
                "hn": {
                    "command": self_exe.to_string_lossy(),
                    "args": ["mcp"],
                }
            }
        });

        let mut cmd = Command::new(bin);
        cmd.env("ANTHROPIC_BASE_URL", format!("http://{addr}"))
            .env("ANTHROPIC_AUTH_TOKEN", "clacker")
            .env("ANTHROPIC_MODEL", anthropic::MODEL)
            // Our model isn't in the CLI's table, and without a window it
            // warns on every launch.
            .env("CLAUDE_CODE_MAX_CONTEXT_TOKENS", "200000")
            // An API key outranks the auth token and would send it to the real
            // API instead of us.
            .env_remove("ANTHROPIC_API_KEY")
            .arg("--mcp-config")
            .arg(mcp_config.to_string())
            // Only our HN server — the user's own MCP servers have no business
            // in a Hacker News session.
            .arg("--strict-mcp-config")
            .arg("--allowedTools")
            .arg("mcp__hn");
        cmd
    }
}
