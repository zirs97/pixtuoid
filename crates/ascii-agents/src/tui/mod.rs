use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use ascii_agents_core::SceneState;
use tokio::sync::RwLock;

pub async fn run_tui(_scene: Arc<RwLock<SceneState>>) -> Result<()> {
    println!("tui::run_tui placeholder — wired in Phase H");
    tokio::time::sleep(Duration::from_millis(100)).await;
    tokio::signal::ctrl_c().await?;
    Ok(())
}
