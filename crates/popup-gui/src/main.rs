use anyhow::{bail, Result};
use clap::Parser;
use popup_gui::{inject_other_options, mcp_server, parse_popup_json, render_popup};
use std::fs;
use std::io::{self, Read};

#[derive(Parser)]
#[command(name = "popup")]
#[command(about = "Native GUI popups with MCP server support", long_about = None)]
struct Args {
    /// Read JSON from stdin and show popup
    #[arg(long)]
    stdin: bool,

    /// Read JSON from file and show popup
    #[arg(long, value_name = "PATH")]
    file: Option<String>,

    /// Force TUI renderer (requires terminal)
    #[arg(long)]
    tui: bool,

    /// Force GUI renderer (requires display server)
    #[arg(long)]
    gui: bool,

    /// Write result JSON to this path instead of stdout (used with FIFO for zellij)
    #[arg(long, value_name = "PATH")]
    result_pipe: Option<String>,

    /// Include only these templates (comma-separated)
    #[arg(long, value_delimiter = ',')]
    include_only: Option<Vec<String>>,

    /// Exclude these templates (comma-separated)
    #[arg(long, value_delimiter = ',')]
    exclude: Option<Vec<String>>,

    /// List available templates and exit
    #[arg(long)]
    list_templates: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RenderMode {
    Gui,
    Tui,
    ZellijTui,
}

fn detect_render_mode(args: &Args) -> Result<RenderMode> {
    if args.tui && args.gui {
        bail!("Cannot specify both --tui and --gui");
    }

    if args.tui {
        return Ok(RenderMode::Tui);
    }

    if args.gui {
        return Ok(RenderMode::Gui);
    }

    // Auto-detect with SSH/remote awareness:
    // 1. SSH env vars (fast path for direct SSH sessions)
    // 2. Remote `who` entries + zellij (catches zellij-attach-over-SSH)
    // 3. Display server available → GUI
    // 4. Zellij available → TUI (local, no display)
    let is_ssh = std::env::var("SSH_CONNECTION").is_ok()
        || std::env::var("SSH_TTY").is_ok();
    let has_display = std::env::var("WAYLAND_DISPLAY").is_ok()
        || std::env::var("DISPLAY").is_ok();
    let has_zellij = std::env::var("ZELLIJ").is_ok();
    let has_remote_users = has_remote_who_entries();

    // Direct SSH session → TUI
    if is_ssh && has_zellij {
        return Ok(RenderMode::ZellijTui);
    }
    if is_ssh {
        bail!(
            "SSH session detected but no zellij session (ZELLIJ) found. \
             Run inside a zellij session for TUI popups over SSH, or use --gui to force GUI."
        )
    }

    // Zellij-attach-over-SSH: we're in zellij and someone is remoted in.
    // The display env vars are from the host session, not the remote client,
    // so prefer TUI to show the popup where the user actually is.
    if has_remote_users && has_zellij {
        return Ok(RenderMode::ZellijTui);
    }

    if has_display {
        return Ok(RenderMode::Gui);
    }

    if has_zellij {
        return Ok(RenderMode::ZellijTui);
    }

    bail!(
        "No display server (WAYLAND_DISPLAY/DISPLAY) and no zellij session (ZELLIJ) detected. \
         Use --gui or --tui to force a renderer, or run inside a zellij session for TUI mode."
    )
}

/// Check if `who` reports any remote (non-local) login sessions.
/// This catches the case where someone SSHed in and attached to an existing
/// zellij session — SSH_CONNECTION won't be set in the zellij env, but `who`
/// will show the remote IP.
fn has_remote_who_entries() -> bool {
    use std::process::Command;
    let output = match Command::new("who").output() {
        Ok(o) => o,
        Err(_) => return false,
    };
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout.lines().any(|line| {
        // `who` format: "user pts/N YYYY-MM-DD HH:MM (IP)"
        // Local sessions show (:0), (:1), etc. Remote sessions show an IP.
        if let Some(paren_start) = line.rfind('(') {
            let host = &line[paren_start + 1..].trim_end_matches(')');
            !host.starts_with(':') && !host.is_empty()
        } else {
            false
        }
    })
}

fn render_definition(
    json_input: &str,
    mode: RenderMode,
    result_pipe: Option<&str>,
) -> Result<()> {
    let definition = parse_popup_json(json_input).map(inject_other_options)?;

    let result = match mode {
        RenderMode::Gui => render_popup(definition)?,
        RenderMode::Tui => popup_tui::render_popup_tui(definition)?,
        RenderMode::ZellijTui => {
            return run_zellij_tui(json_input);
        }
    };

    let output = serde_json::to_string_pretty(&result)?;

    if let Some(pipe_path) = result_pipe {
        fs::write(pipe_path, &output)?;
    } else {
        println!("{}", output);
    }

    Ok(())
}

fn run_zellij_tui(json_input: &str) -> Result<()> {
    use std::process::Command;
    use uuid::Uuid;

    let id = Uuid::new_v4();
    let fifo_path = format!("/tmp/popup-mcp-{}", id);
    let json_path = format!("/tmp/popup-mcp-{}.json", id);

    // Create FIFO
    let status = Command::new("mkfifo").arg(&fifo_path).status()?;
    if !status.success() {
        bail!("Failed to create FIFO at {}", fifo_path);
    }

    // Write JSON to temp file (zellij can't pipe stdin to spawned process)
    fs::write(&json_path, json_input)?;

    // Cleanup guard
    struct Cleanup {
        fifo: String,
        json: String,
    }
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.fifo);
            let _ = fs::remove_file(&self.json);
        }
    }
    let _cleanup = Cleanup {
        fifo: fifo_path.clone(),
        json: json_path.clone(),
    };

    // Find our own binary path
    let exe = std::env::current_exe()?;

    // Spawn in zellij floating pane
    let status = Command::new("zellij")
        .args([
            "action",
            "new-pane",
            "--floating",
            "--close-on-exit",
            "--",
        ])
        .arg(&exe)
        .args(["--tui", "--file"])
        .arg(&json_path)
        .args(["--result-pipe"])
        .arg(&fifo_path)
        .status()?;

    if !status.success() {
        bail!("Failed to spawn zellij floating pane");
    }

    // `zellij action new-pane` returns immediately; the child runs asynchronously.
    // Read the FIFO with a timeout so we don't hang forever if the child dies
    // without writing (e.g., crash, OOM, terminal closed).
    let fifo_path_thread = fifo_path.clone();
    let (tx, rx) = std::sync::mpsc::channel::<anyhow::Result<String>>();
    std::thread::spawn(move || {
        let result = fs::read_to_string(&fifo_path_thread).map_err(anyhow::Error::from);
        let _ = tx.send(result);
    });

    // 5-minute timeout — enough for any reasonable human interaction.
    let result_json = match rx.recv_timeout(std::time::Duration::from_secs(300)) {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => bail!("failed to read result from FIFO: {}", e),
        Err(_timeout) => {
            // Timeout expired — treat as cancel.
            String::new()
        }
    };

    if result_json.is_empty() {
        // Child exited without writing (or timed out) — treat as cancel
        let cancelled = popup_common::PopupResult::Cancelled;
        println!("{}", serde_json::to_string_pretty(&cancelled)?);
    } else {
        println!("{}", result_json);
    }

    Ok(())
}

fn run_stdin_mode(mode: RenderMode, result_pipe: Option<&str>) -> Result<()> {
    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;

    match render_definition(&input, mode, result_pipe) {
        Ok(()) => std::process::exit(0),
        Err(e) => {
            let error = serde_json::json!({"error": e.to_string()});
            println!("{}", serde_json::to_string(&error)?);
            std::process::exit(1);
        }
    }
}

fn run_file_mode(path: &str, mode: RenderMode, result_pipe: Option<&str>) -> Result<()> {
    let input = fs::read_to_string(path)?;

    match render_definition(&input, mode, result_pipe) {
        Ok(()) => std::process::exit(0),
        Err(e) => {
            let error = serde_json::json!({"error": e.to_string()});
            println!("{}", serde_json::to_string(&error)?);
            std::process::exit(1);
        }
    }
}

fn main() -> Result<()> {
    let args = Args::parse();

    if args.stdin || args.file.is_some() {
        let mode = detect_render_mode(&args)?;
        let result_pipe = args.result_pipe.as_deref();

        if args.stdin {
            run_stdin_mode(mode, result_pipe)
        } else {
            run_file_mode(args.file.as_ref().unwrap(), mode, result_pipe)
        }
    } else {
        // MCP server mode (default)
        let server_args = mcp_server::ServerArgs {
            include_only: args.include_only,
            exclude: args.exclude,
            list_templates: args.list_templates,
        };
        mcp_server::run(server_args)
    }
}
