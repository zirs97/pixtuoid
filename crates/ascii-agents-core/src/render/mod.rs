use std::time::SystemTime;

use anyhow::Result;

use crate::layout::SceneLayout;
use crate::sprite::format::Pack;
use crate::state::SceneState;

/// Anything that can paint a `SceneState` for a user. The widened signature
/// (layout + pack + now) is what every real renderer needs — the binary's
/// half-block TUI, a future web canvas, a PNG snapshotter, a GIF capture.
/// The original 1-arg signature couldn't express that without forcing each
/// impl to recompute layout or load its own pack.
pub trait Renderer {
    fn render(
        &mut self,
        scene: &SceneState,
        layout: &SceneLayout,
        pack: &Pack,
        now: SystemTime,
    ) -> Result<()>;
}

#[cfg(feature = "test-renderer")]
pub mod test_renderer;
