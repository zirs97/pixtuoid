use std::fs::OpenOptions;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use anyhow::Result;
use ascii_agents::cli::{Cli, Cmd};
use ascii_agents::{install, runtime};
use clap::Parser;
use tracing_subscriber::EnvFilter;

fn main() -> Result<()> {
    let (log_level, cmd) = Cli::parse().cmd_or_default();
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(&log_level));

    // Log routing:
    //   `run` in TUI mode (headless=false) → file at $ASCII_AGENTS_LOG /
    //     $XDG_STATE_HOME/ascii-agents/log / ~/.cache/ascii-agents/log,
    //     because writing ANSI logs to stderr while crossterm raw mode owns
    //     the screen corrupts the TUI output.
    //   everything else (install-hooks, uninstall-hooks, --headless) →
    //     stderr as before.
    let tui_active = matches!(&cmd, Cmd::Run { headless, .. } if !*headless);
    if tui_active {
        if let Ok(path) = log_file_path() {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Ok(f) = OpenOptions::new().create(true).append(true).open(&path) {
                let writer = Arc::new(Mutex::new(f));
                tracing_subscriber::fmt()
                    .with_env_filter(filter)
                    .with_ansi(false)
                    .with_writer(move || MutexFileWriter(writer.clone()))
                    .init();
                eprintln!("logging to {}", path.display());
            }
        }
    } else {
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_writer(std::io::stderr)
            .init();
    }

    match cmd {
        Cmd::Run {
            socket,
            projects_root,
            max_desks,
            headless,
        } => runtime::run(socket, projects_root, max_desks, headless),
        Cmd::InstallHooks {
            hook_path,
            settings,
        } => install::install(hook_path, settings),
        Cmd::UninstallHooks { settings } => install::uninstall(settings),
    }
}

fn log_file_path() -> Result<PathBuf> {
    if let Ok(p) = std::env::var("ASCII_AGENTS_LOG") {
        return Ok(PathBuf::from(p));
    }
    if let Ok(state) = std::env::var("XDG_STATE_HOME") {
        return Ok(PathBuf::from(format!("{state}/ascii-agents/log")));
    }
    let home = std::env::var("HOME")?;
    Ok(PathBuf::from(format!("{home}/.cache/ascii-agents/log")))
}

/// Adapter that gives `tracing-subscriber` a `Write`-able file behind a Mutex.
struct MutexFileWriter(Arc<Mutex<std::fs::File>>);

impl std::io::Write for MutexFileWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0
            .lock()
            .map_err(|_| std::io::Error::other("poisoned"))?
            .write(buf)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.0
            .lock()
            .map_err(|_| std::io::Error::other("poisoned"))?
            .flush()
    }
}
