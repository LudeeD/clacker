//! clacker — browse Hacker News through a real agent harness.

mod brain;
mod harness;
mod hn;
mod intent;
mod mcp;
mod protocol;
mod server;

use std::process::ExitCode;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    version,
    about = "Browse Hacker News through an agent harness",
    args_conflicts_with_subcommands = true,
    after_help = "Harnesses:\n  claude  Claude Code (impersonates the Anthropic Messages API)"
)]
struct Args {
    /// Harness to launch
    #[arg(long, value_name = "ID")]
    harness: Option<String>,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Run only the fake provider API (for debugging)
    Serve {
        /// Harness protocol to serve
        #[arg(long, value_name = "ID")]
        harness: Option<String>,
    },

    /// Run as a stdio MCP server (the harness does this)
    Mcp,
}

fn main() -> ExitCode {
    let args = Args::parse();

    let result = match args.command {
        Some(Command::Mcp) => mcp::run().map(|_| 0).map_err(|e| e.to_string()),
        Some(Command::Serve { harness }) => serve(harness.as_deref()),
        None => launch(args.harness.as_deref()),
    };

    match result {
        Ok(code) => ExitCode::from(code),
        Err(message) => {
            eprintln!("clacker: {message}");
            ExitCode::FAILURE
        }
    }
}

fn pick(id: Option<&str>) -> Result<(Box<dyn harness::Harness>, std::path::PathBuf), String> {
    match id {
        Some(id) => {
            let h = harness::by_id(id).ok_or_else(|| format!("unknown harness '{id}'"))?;
            let bin = h
                .locate()
                .ok_or_else(|| format!("{} isn't installed or isn't on PATH", h.name()))?;
            Ok((h, bin))
        }
        None => harness::detect()
            .ok_or_else(|| "no supported harness found on PATH (install Claude Code)".to_string()),
    }
}

fn launch(id: Option<&str>) -> Result<u8, String> {
    let (harness, bin) = pick(id)?;
    let self_exe = std::env::current_exe().map_err(|e| format!("can't find my own path: {e}"))?;

    let running = server::spawn(harness.protocol()).map_err(|e| e.to_string())?;
    let mut cmd = harness.command(&bin, running.addr, &self_exe);

    let status = cmd
        .status()
        .map_err(|e| format!("couldn't start {}: {e}", harness.name()))?;

    Ok(status.code().unwrap_or(0).clamp(0, 255) as u8)
}

fn serve(id: Option<&str>) -> Result<u8, String> {
    let (harness, _) = match pick(id) {
        Ok(found) => found,
        // For plain `serve` the harness needn't be installed — we only need
        // its protocol.
        Err(_) if id.is_none() => {
            let h = harness::all().into_iter().next().expect("one harness");
            (h, std::path::PathBuf::new())
        }
        Err(e) => return Err(e),
    };

    let protocol = harness.protocol();
    println!("clacker: serving the {} API", protocol.id());
    let running = server::spawn(protocol).map_err(|e| e.to_string())?;
    println!("clacker: listening on http://{}", running.addr);
    println!("clacker: point a harness at it with ANTHROPIC_BASE_URL, or ^C to stop");

    loop {
        std::thread::park();
    }
}
