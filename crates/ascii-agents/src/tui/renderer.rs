//! Top-down scene renderer.
//!
//! Each CC session shows up as a chibi character sitting at a desk, viewed
//! from above with a 3/4-perspective tilt so the face stays readable. Hair
//! and shirt are recolored per agent so distinct sessions are visually
//! distinguishable.

use std::io::{stdout, Stdout};
use std::time::SystemTime;

use anyhow::Result;
use ascii_agents_core::sprite::animator::frame_index_at;
use ascii_agents_core::sprite::blit::blit_frame;
use ascii_agents_core::sprite::format::Pack;
use ascii_agents_core::sprite::{Frame, Palette, Pixel, Rgb, RgbBuffer};
use ascii_agents_core::state::ActivityState;
use ascii_agents_core::{AgentSlot, SceneState};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::{Backend, CrosstermBackend};
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Terminal;

pub type Term = Terminal<CrosstermBackend<Stdout>>;

pub fn setup_terminal() -> Result<Term> {
    enable_raw_mode()?;
    let mut out = stdout();
    execute!(out, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(out);
    Ok(Terminal::new(backend)?)
}

pub fn teardown_terminal(term: &mut Term) -> Result<()> {
    disable_raw_mode()?;
    execute!(term.backend_mut(), LeaveAlternateScreen)?;
    term.show_cursor()?;
    Ok(())
}

const BG: Rgb = Rgb(28, 32, 40);
// Wood-plank floor: two warm browns alternating in horizontal bands.
const PLANK_A: Rgb = Rgb(120, 84, 50);
const PLANK_B: Rgb = Rgb(100, 70, 38);
const PLANK_LINE: Rgb = Rgb(72, 48, 24);
// Walls and baseboards frame the room.
const WALL: Rgb = Rgb(56, 56, 70);
const WALL_TRIM: Rgb = Rgb(80, 80, 100);
const BASEBOARD: Rgb = Rgb(40, 40, 52);
// Per-cubicle rug behind the desk.
const RUG_PALETTE: &[Rgb] = &[
    Rgb(0x4a, 0x55, 0x80),
    Rgb(0x6a, 0x3f, 0x55),
    Rgb(0x40, 0x60, 0x4f),
    Rgb(0x6e, 0x4d, 0x2e),
];

// Per-agent recolor presets, reused from the side-view renderer's palette
// concept so each session gets a stable hair + shirt combo.
const SHIRT_PRESETS: &[Rgb] = &[
    Rgb(0x2e, 0x62, 0xcf),
    Rgb(0x16, 0xa0, 0x6e),
    Rgb(0xb0, 0x32, 0xa8),
    Rgb(0xc6, 0x6a, 0x1e),
    Rgb(0x6c, 0x4f, 0x9e),
    Rgb(0x9c, 0x27, 0x27),
    Rgb(0x32, 0x82, 0x9b),
    Rgb(0x80, 0x55, 0x32),
];
const HAIR_PRESETS: &[Rgb] = &[
    Rgb(0x2a, 0x1a, 0x0e),
    Rgb(0x52, 0x32, 0x10),
    Rgb(0xc7, 0xa3, 0x4a),
    Rgb(0x7a, 0x32, 0x10),
    Rgb(0x3a, 0x3a, 0x3a),
];

/// Sprite footprint (matches what the .sprite file declares: 12×14).
const SPRITE_W: u16 = 12;
const SPRITE_H: u16 = 14;
/// Desk footprint (matches desk.sprite: 16×8).
const DESK_W: u16 = 16;
const DESK_H: u16 = 8;

/// Each cubicle slot holds: 1 label cell + character + desk, with small gaps.
const SLOT_W: u16 = 18;
const SLOT_H: u16 = SPRITE_H + DESK_H - 4; // character overlaps desk top by 4 px
const SLOT_GAP_X: u16 = 2;
const SLOT_GAP_Y: u16 = 4;

struct Slot {
    /// Buf-pixel coords of the top-left of this slot.
    x: u16,
    y: u16,
}

fn cubicle_grid(buf_w: u16, buf_h: u16, n: usize) -> Vec<Slot> {
    let col_w = SLOT_W + SLOT_GAP_X;
    let row_h = SLOT_H + SLOT_GAP_Y;
    let cols = (buf_w / col_w).max(1);
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let row = (i as u16) / cols;
        let col = (i as u16) % cols;
        let x = col * col_w + 2;
        let y = row * row_h + 2;
        if y + SLOT_H > buf_h {
            break;
        }
        out.push(Slot { x, y });
    }
    out
}

fn agent_palette(base: &Palette, agent: &AgentSlot) -> Palette {
    let seed = agent.agent_id.raw() as usize;
    let shirt = SHIRT_PRESETS[seed % SHIRT_PRESETS.len()];
    let hair = HAIR_PRESETS[(seed / 7) % HAIR_PRESETS.len()];
    base.with_override('B', Some(shirt))
        .with_override('H', Some(hair))
}

fn recolor_frame(frame: &Frame, pal: &Palette, base_pal: &Palette) -> Frame {
    // Substitute pixels whose color matches the base palette's `B` or `H`
    // with the per-agent recolored values from `pal`. Cheap because we
    // compare against just two RGB tuples.
    let base_shirt = base_pal.get('B').flatten();
    let base_hair = base_pal.get('H').flatten();
    let agent_shirt = pal.get('B').flatten();
    let agent_hair = pal.get('H').flatten();
    let pixels: Vec<Pixel> = frame
        .pixels
        .iter()
        .map(|p| match p {
            Some(rgb) if Some(*rgb) == base_shirt => agent_shirt,
            Some(rgb) if Some(*rgb) == base_hair => agent_hair,
            other => *other,
        })
        .collect();
    Frame {
        width: frame.width,
        height: frame.height,
        pixels,
    }
}

pub fn draw_scene<B: Backend>(
    term: &mut Terminal<B>,
    scene: &SceneState,
    pack: &Pack,
    now: SystemTime,
    buf: &mut RgbBuffer,
) -> Result<()> {
    let agents: Vec<_> = scene.agents.values().cloned().collect();
    term.draw(|f| {
        let size = f.area();

        let title = Paragraph::new(Line::from(vec![
            Span::raw(" ascii-agents (top-down) — "),
            Span::raw(format!(
                "{} session{} ",
                agents.len(),
                if agents.len() == 1 { "" } else { "s" }
            )),
        ]));
        f.render_widget(
            title,
            Rect {
                x: size.x,
                y: size.y,
                width: size.width,
                height: 1,
            },
        );

        let footer = Paragraph::new(Span::raw(" [q] quit "))
            .style(Style::default().fg(Color::DarkGray));
        f.render_widget(
            footer,
            Rect {
                x: size.x,
                y: size.y + size.height.saturating_sub(1),
                width: size.width,
                height: 1,
            },
        );

        let scene_rect = Rect {
            x: size.x,
            y: size.y + 1,
            width: size.width,
            height: size.height.saturating_sub(2),
        };
        if scene_rect.width < 16 || scene_rect.height < 10 {
            return;
        }

        let buf_w = scene_rect.width;
        let buf_h = scene_rect.height * 2;
        buf.ensure_size(buf_w, buf_h, BG);

        paint_floor_and_walls(buf, buf_w, buf_h);

        let slots = cubicle_grid(buf_w, buf_h, agents.len());

        let base_pal = pack.palette.clone();
        let desk_anim = pack.animation("desk");
        let plant_anim = pack.animation("plant");

        // Pass 0: rugs behind each cubicle (under desk and character).
        for (slot, agent) in slots.iter().zip(agents.iter()) {
            let rug = RUG_PALETTE[(agent.agent_id.raw() as usize / 11) % RUG_PALETTE.len()];
            paint_rug(buf, slot.x, slot.y, SLOT_W, SLOT_H, rug);
        }

        // Pass 0.5: plants in the gaps between cubicles (deterministic
        // placement so they don't shift between frames).
        if let Some(plant) = plant_anim.and_then(|a| a.frames.first()) {
            paint_plants(buf, buf_w, buf_h, &slots, plant);
        }

        // Pass 1: desks (so character paints over them).
        for (slot, _agent) in slots.iter().zip(agents.iter()) {
            let dx = slot.x + (SLOT_W - DESK_W) / 2;
            let dy = slot.y + SLOT_H - DESK_H;
            if let Some(anim) = desk_anim {
                if let Some(frame) = anim.frames.first() {
                    blit_frame(frame, dx, dy, buf);
                }
            }
        }

        // Pass 2: characters.
        for (slot, agent) in slots.iter().zip(agents.iter()) {
            let anim_name = match &agent.state {
                ActivityState::Idle => "idle",
                ActivityState::Active { .. } => "typing",
                ActivityState::Waiting { .. } => "waiting",
            };
            let Some(anim) = pack.animation(anim_name).or_else(|| pack.animation("idle")) else {
                continue;
            };
            let fi = frame_index_at(
                agent.state_started_at,
                now,
                anim.frame_ms,
                anim.frames.len(),
            );
            let frame = &anim.frames[fi];
            let pal = agent_palette(&base_pal, agent);
            let recolored = recolor_frame(frame, &pal, &base_pal);

            let sx = slot.x + (SLOT_W - SPRITE_W) / 2;
            let sy = slot.y + SLOT_H - DESK_H - SPRITE_H + 4; // overlap desk top
            blit_frame(&recolored, sx, sy, buf);
        }

        // Map RGB pixel buffer onto the ratatui terminal buffer as ▀ half-blocks.
        let term_buf = f.buffer_mut();
        let w = buf.width as usize;
        let cell_rows = (buf.height / 2) as usize;
        for cy in 0..cell_rows {
            for cx in 0..(buf.width as usize) {
                let x = scene_rect.x + cx as u16;
                let y = scene_rect.y + cy as u16;
                if x >= scene_rect.x + scene_rect.width
                    || y >= scene_rect.y + scene_rect.height
                {
                    continue;
                }
                let py_top = cy * 2;
                let py_bot = cy * 2 + 1;
                let fg = buf.pixels[py_top * w + cx];
                let bg = buf.pixels[py_bot * w + cx];
                let cell = &mut term_buf[(x, y)];
                cell.set_symbol("▀");
                cell.fg = Color::Rgb(fg.0, fg.1, fg.2);
                cell.bg = Color::Rgb(bg.0, bg.1, bg.2);
            }
        }

        // Labels — one cell row above each cubicle.
        for (slot, agent) in slots.iter().zip(agents.iter()) {
            let lx = scene_rect.x + slot.x;
            let ly = scene_rect.y + slot.y / 2;
            let text = format!("{} {}", agent.label, summarize_state(&agent.state));
            let para = Paragraph::new(Span::styled(
                text,
                Style::default().fg(Color::White),
            ));
            f.render_widget(
                para,
                Rect {
                    x: lx,
                    y: ly,
                    width: SLOT_W,
                    height: 1,
                },
            );
        }
    })?;
    Ok(())
}

fn paint_floor_and_walls(buf: &mut RgbBuffer, buf_w: u16, buf_h: u16) {
    const PLANK_H: u16 = 6;
    const TOP_WALL_H: u16 = 6;
    const TOP_WALL_TRIM_H: u16 = 1;
    const BASEBOARD_H: u16 = 3;

    // Wood-plank floor across the full buffer.
    for y in 0..buf_h {
        // Stagger the plank seam x-positions across rows so the grain reads
        // as planks instead of a brick wall.
        let band = y / PLANK_H;
        let seam_offset = (band as u32 * 13) % 16;
        for x in 0..buf_w {
            let in_seam = y % PLANK_H == PLANK_H - 1
                || ((x as u32).wrapping_add(seam_offset)) % 16 == 0;
            let color = if in_seam {
                PLANK_LINE
            } else if band % 2 == 0 {
                PLANK_A
            } else {
                PLANK_B
            };
            buf.put(x, y, color);
        }
    }

    // Top wall band + trim line.
    for y in 0..TOP_WALL_H.min(buf_h) {
        for x in 0..buf_w {
            buf.put(x, y, WALL);
        }
    }
    let trim_y = TOP_WALL_H;
    if trim_y < buf_h {
        for x in 0..buf_w {
            for ty in 0..TOP_WALL_TRIM_H {
                let py = trim_y + ty;
                if py < buf_h {
                    buf.put(x, py, WALL_TRIM);
                }
            }
        }
    }

    // Bottom baseboard.
    let base_y = buf_h.saturating_sub(BASEBOARD_H);
    for y in base_y..buf_h {
        for x in 0..buf_w {
            buf.put(x, y, BASEBOARD);
        }
    }
}

fn paint_rug(buf: &mut RgbBuffer, x: u16, y: u16, w: u16, h: u16, color: Rgb) {
    // Rug spans the slot's footprint with a 1-px lighter border, slightly
    // smaller than the cubicle so it doesn't bleed into neighbors.
    let pad = 1;
    let lighter = Rgb(
        color.0.saturating_add(40),
        color.1.saturating_add(40),
        color.2.saturating_add(40),
    );
    for dy in pad..h.saturating_sub(pad) {
        for dx in pad..w.saturating_sub(pad) {
            let px = x + dx;
            let py = y + dy;
            if px >= buf.width || py >= buf.height {
                continue;
            }
            let on_border = dy == pad
                || dy == h - pad - 1
                || dx == pad
                || dx == w - pad - 1;
            buf.put(px, py, if on_border { lighter } else { color });
        }
    }
}

fn paint_plants(
    buf: &mut RgbBuffer,
    buf_w: u16,
    buf_h: u16,
    slots: &[Slot],
    frame: &Frame,
) {
    // For each adjacent pair of slots in the same row, drop a plant in the
    // gap between them at floor level.
    let col_w = SLOT_W + SLOT_GAP_X;
    let row_h = SLOT_H + SLOT_GAP_Y;
    let cols = (buf_w / col_w).max(1);

    for (i, slot) in slots.iter().enumerate() {
        let col = (i as u16) % cols;
        // Only the *first* slot of each row places a plant on its LEFT side
        // (corner of the room); other slots place a plant to their left in
        // the gap between cubicles.
        let plant_x = if col == 0 {
            // Left wall corner
            slot.x.saturating_sub(SLOT_GAP_X + frame.width)
        } else {
            slot.x.saturating_sub(frame.width + 1)
        };
        // Anchor plant just below the top wall so it sits in the floor area.
        let plant_y = slot.y.saturating_sub(frame.height + 1).max(8);
        // Only place if it actually fits in buf and doesn't trample a slot.
        if plant_x + frame.width <= buf_w && plant_y + frame.height <= buf_h {
            blit_frame(frame, plant_x, plant_y, buf);
        }
        // Also one to the right of the LAST slot in each row.
        if col == cols - 1 || i == slots.len() - 1 {
            let right_x = slot.x + SLOT_W + 1;
            if right_x + frame.width <= buf_w {
                let ry = (slot.y + row_h).saturating_sub(frame.height + 1).max(8);
                blit_frame(frame, right_x, ry, buf);
            }
        }
    }
}

fn summarize_state(state: &ActivityState) -> &'static str {
    match state {
        ActivityState::Idle => "idle",
        ActivityState::Active { .. } => "working",
        ActivityState::Waiting { .. } => "waiting",
    }
}
