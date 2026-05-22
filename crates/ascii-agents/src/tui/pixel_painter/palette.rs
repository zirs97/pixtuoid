//! Per-agent palette (shirt / hair / skin) + frame recolor + color math
//! primitives (blend / lerp / bell / mix_lab).
//!
//! `agent_palette` picks a deterministic shirt/hair/skin from per-agent
//! hashes; `recolor_frame` rewrites a frame's pixels by RGB-equality
//! against the base pack palette. The color-math helpers live here too
//! because the palette tint code uses them directly and they're widely
//! shared with background/effects.

use ascii_agents_core::sprite::{Frame, Palette, Pixel, Rgb};
use ascii_agents_core::state::ActivityState;
use ascii_agents_core::AgentSlot;

use crate::tui::pose;

/// Warm / extroverted shirt palette — agents with higher trip_chance_pct
/// pick from here. Expanded from 4 → 8 colors so a crowded office of
/// 16 agents has visibly distinct silhouettes.
const SHIRT_PRESETS_WARM: &[Rgb] = &[
    Rgb(0x9c, 0x27, 0x27), // crimson
    Rgb(0xc6, 0x6a, 0x1e), // burnt orange
    Rgb(0xb0, 0x32, 0xa8), // magenta
    Rgb(0xd0, 0x9c, 0x32), // mustard
    Rgb(0xe0, 0x46, 0x46), // tomato
    Rgb(0xa8, 0x4e, 0x9c), // rose violet
    Rgb(0xcf, 0x7b, 0x2c), // pumpkin
    Rgb(0xc4, 0x39, 0x6f), // raspberry
];
/// Cool / homebody shirt palette — used for lower-trip-chance agents.
const SHIRT_PRESETS_COOL: &[Rgb] = &[
    Rgb(0x2e, 0x62, 0xcf), // royal blue
    Rgb(0x16, 0xa0, 0x6e), // forest green
    Rgb(0x32, 0x82, 0x9b), // teal
    Rgb(0x6c, 0x4f, 0x9e), // violet
    Rgb(0x4a, 0x7a, 0xb8), // steel blue
    Rgb(0x2e, 0x8a, 0x84), // pine
    Rgb(0x3e, 0x52, 0x9c), // indigo
    Rgb(0x5c, 0x8a, 0x32), // moss green
];
/// 8 hair colors — was 5. Added silver/grey for older-coded agents,
/// ginger / strawberry blonde / jet black for more silhouette variety.
const HAIR_PRESETS: &[Rgb] = &[
    Rgb(0x14, 0x0a, 0x06), // jet black
    Rgb(0x2a, 0x1a, 0x0e), // near-black brown
    Rgb(0x52, 0x32, 0x10), // dark brown
    Rgb(0x8a, 0x5a, 0x36), // light brown
    Rgb(0xc7, 0xa3, 0x4a), // blond
    Rgb(0xd8, 0x68, 0x32), // ginger
    Rgb(0x7a, 0x32, 0x10), // auburn
    Rgb(0xa8, 0xa8, 0xb0), // silver-grey
];
const SKIN_PRESETS: &[Rgb] = &[
    Rgb(0xf4, 0xc7, 0x9a), // light peach (matches base palette S)
    Rgb(0xe0, 0xa8, 0x70), // medium
    Rgb(0xb8, 0x80, 0x50), // tan
    Rgb(0x8a, 0x5a, 0x36), // deep brown
    Rgb(0xc8, 0x9a, 0x64), // warm tan
];

pub(super) fn agent_palette(base: &Palette, agent: &AgentSlot) -> Palette {
    let seed = agent.agent_id.raw() as usize;
    // Personality nudges aesthetic choice: extroverted (high trip_chance)
    // agents pick from the warm shirt palette, homebodies from cool.
    let p = pose::personality_for(agent.agent_id);
    let shirts = if p.trip_chance_pct >= 30 {
        SHIRT_PRESETS_WARM
    } else {
        SHIRT_PRESETS_COOL
    };
    let shirt = shirts[seed % shirts.len()];
    let hair = HAIR_PRESETS[(seed / 7) % HAIR_PRESETS.len()];
    let skin = SKIN_PRESETS[(seed / 13) % SKIN_PRESETS.len()];
    // Active = monitor is lit, light reflects on the user's face. Tint the
    // skin slightly toward the glow color so the eye reads "the monitor is
    // actually lighting them up", not just "there's a green dot below".
    let final_skin = if matches!(agent.state, ActivityState::Active { .. }) {
        const GLOW_TINT: Rgb = Rgb(140, 240, 170);
        Rgb(
            blend(skin.0, GLOW_TINT.0, 0.18),
            blend(skin.1, GLOW_TINT.1, 0.18),
            blend(skin.2, GLOW_TINT.2, 0.18),
        )
    } else {
        skin
    };
    base.with_override('B', Some(shirt))
        .with_override('H', Some(hair))
        .with_override('S', Some(final_skin))
}

pub(super) fn recolor_frame(frame: &Frame, pal: &Palette, base_pal: &Palette) -> Frame {
    let base_shirt = base_pal.get('B').flatten();
    let base_hair = base_pal.get('H').flatten();
    let base_skin = base_pal.get('S').flatten();
    let agent_shirt = pal.get('B').flatten();
    let agent_hair = pal.get('H').flatten();
    let agent_skin = pal.get('S').flatten();
    let pixels: Vec<Pixel> = frame
        .pixels
        .iter()
        .map(|p| match p {
            Some(rgb) if Some(*rgb) == base_shirt => agent_shirt,
            Some(rgb) if Some(*rgb) == base_hair => agent_hair,
            Some(rgb) if Some(*rgb) == base_skin => agent_skin,
            other => *other,
        })
        .collect();
    Frame {
        width: frame.width,
        height: frame.height,
        pixels,
    }
}

// --- Color math primitives -----------------------------------------------

pub(super) fn lerp_rgb(a: Rgb, b: Rgb, t: f32) -> Rgb {
    mix_lab(a, b, t)
}

/// Bell curve centered at `c` with half-width `w` (so the bell is 0 at
/// `c ± w` and 1 at `c`). Used for dawn/dusk twilight tint.
pub(super) fn bell(x: f32, c: f32, w: f32) -> f32 {
    let d = (x - c) / w;
    (1.0 - d * d).max(0.0)
}

/// Per-channel sRGB lerp. Cheap; used for low-strength tints where
/// perceptual error doesn't matter (e.g. agent skin glow).
pub(super) fn blend(a: u8, b: u8, t: f32) -> u8 {
    ((a as f32) * (1.0 - t) + (b as f32) * t)
        .round()
        .clamp(0.0, 255.0) as u8
}

/// Perceptually-correct Lab-space mix between two sRGB colors. Twilight
/// (orange → navy) and dim overlays travel cleanly through Lab without the
/// muddy desaturated midpoint that naive sRGB lerp produces. Slower than
/// `blend()` but only used where the perceptual difference is visible.
pub(super) fn mix_lab(a: Rgb, b: Rgb, t: f32) -> Rgb {
    use palette::{FromColor, IntoColor, Lab, Mix, Srgb};
    let sa = Srgb::new(a.0 as f32 / 255.0, a.1 as f32 / 255.0, a.2 as f32 / 255.0);
    let sb = Srgb::new(b.0 as f32 / 255.0, b.1 as f32 / 255.0, b.2 as f32 / 255.0);
    let la = Lab::from_color(sa);
    let lb = Lab::from_color(sb);
    let mixed: Srgb = la.mix(lb, t.clamp(0.0, 1.0)).into_color();
    Rgb(
        (mixed.red.clamp(0.0, 1.0) * 255.0).round() as u8,
        (mixed.green.clamp(0.0, 1.0) * 255.0).round() as u8,
        (mixed.blue.clamp(0.0, 1.0) * 255.0).round() as u8,
    )
}
