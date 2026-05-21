use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use ascii_agents_core::source::claude_code::ClaudeCodeSource;
use ascii_agents_core::{AgentEvent, Reducer, SceneState, Source};
use tokio::sync::{mpsc, RwLock};

pub fn run(
    socket: Option<PathBuf>,
    projects_root: Option<PathBuf>,
    max_desks: usize,
) -> Result<()> {
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build()?;
    rt.block_on(async move { run_async(socket, projects_root, max_desks).await })
}

async fn run_async(
    socket: Option<PathBuf>,
    projects_root: Option<PathBuf>,
    max_desks: usize,
) -> Result<()> {
    let mut src = ClaudeCodeSource::default_paths();
    if let Some(s) = socket {
        src.socket_path = s;
    }
    if let Some(p) = projects_root {
        src.projects_root = p;
    }

    let (tx, rx) = mpsc::channel::<AgentEvent>(256);
    let scene: Arc<RwLock<SceneState>> = Arc::new(RwLock::new(SceneState::new(max_desks)));

    let scene_for_reducer = scene.clone();
    tokio::spawn(reducer_task(rx, scene_for_reducer));

    let src_box: Box<dyn ascii_agents_core::source::Source> = Box::new(src);
    tokio::spawn(async move {
        if let Err(e) = src_box.run(tx).await {
            tracing::error!("source died: {e}");
        }
    });

    crate::tui::run_tui(scene).await
}

async fn reducer_task(mut rx: mpsc::Receiver<AgentEvent>, scene: Arc<RwLock<SceneState>>) {
    let mut reducer = Reducer::new();
    while let Some(ev) = rx.recv().await {
        let now = Instant::now();
        let mut s = scene.write().await;
        reducer.apply(&mut s, ev, now, Source::Hook);
    }
    tokio::time::sleep(Duration::from_secs(60)).await;
}
