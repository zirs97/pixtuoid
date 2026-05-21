# CLAUDE.md

Instructions for Claude Code (or any AI coding agent) working in this repo.

## What this is

Terminal-native, multi-agent pixel-art visualizer for AI coding agents. Each running CC (Claude Code) session shows up as an animated half-block sprite in an ASCII office. Built in Rust as a Cargo workspace of three crates.

User-facing overview: [`README.md`](README.md).
v1 spec: [`docs/superpowers/specs/2026-05-20-ascii-agents-design.md`](docs/superpowers/specs/2026-05-20-ascii-agents-design.md).
v1 plan (28 TDD-shaped tasks): [`docs/superpowers/plans/2026-05-20-ascii-agents-v1.md`](docs/superpowers/plans/2026-05-20-ascii-agents-v1.md).

## Layout

```
crates/
├── ascii-agents-core/      headless lib — no terminal deps (ratatui/crossterm forbidden here)
│   ├── source/             Source trait, hook+jsonl decoders, listeners
│   ├── state/              SceneState + Reducer (with Transport-tagged dedup)
│   ├── sprite/             .sprite parser, pack.toml loader, half-block blitter, animator
│   ├── render/             Renderer trait + TestRenderer (feature = "test-renderer")
│   └── tests/              one integration test per concern
├── ascii-agents/           binary — ratatui + crossterm + tokio + clap
│   ├── cli.rs              clap subcommands (run / install-hooks / uninstall-hooks)
│   ├── runtime.rs          tokio task wiring (source ── (Transport, AgentEvent) ──► reducer ──► renderer)
│   ├── install/            settings.json merge, atomic write, advisory lock, stow-symlink safe
│   └── tui/                ratatui App + draw_scene (generic over Backend)
└── ascii-agents-hook/      tiny shim CC invokes — stdin JSON → Unix socket, 200ms write timeout
assets/sprites/default/     bundled character pack (idle, typing x3, waiting) at 12×16 px
```

## Build & test

```
cargo build --workspace                                              # debug build
cargo build --release --workspace                                    # release build
cargo test --workspace --features ascii-agents-core/test-renderer    # all tests (46)
cargo run --release --example snapshot -- /tmp/snap.png              # render TUI to PNG
./target/release/ascii-agents run --headless --projects-root ~/.claude/projects   # live test against real CC
```

The `test-renderer` feature is needed for the `e2e.rs` integration test. The dev workspace test alias is just `cargo test`.

## Conventions

- **TDD first.** Plan and existing tests are TDD-shaped — failing test → minimal impl → commit. Don't add code without a test that exercises it.
- **DRY, YAGNI.** No features beyond what v1 specifies. v2 items are deferred — adding them in v1 code is a regression.
- **No comments unless WHY.** Don't write comments that restate what the code does. Comment only when a future reader can't tell from the code why something is the way it is (a workaround, a non-obvious constraint, a surprising invariant).
- **Errors propagate via `anyhow::Result` in app code, `thiserror` in core if a typed error becomes load-bearing.** The hook listener and JSONL watcher log + continue on malformed input — they never panic.
- **No `unwrap()` in non-test code.** Tests can unwrap freely.
- **Match the surrounding shell:** scripts in this repo target zsh (interactive) or POSIX sh. `shellcheck` any `.sh` you touch.
- **macOS first.** BSD-flavored CLI, brew, launchd for daemons. The hook shim is Unix-socket specific (`std::os::unix::net::UnixStream`).

## Architecture invariants

These are load-bearing; don't break them without updating the spec.

1. **`ascii-agents-core` has no terminal dependencies.** No `ratatui`, no `crossterm`, no `stdout` writes. If you need one, the abstraction belongs behind the `Renderer` trait.
2. **Events flow through ONE channel** typed `mpsc::Sender<(Transport, AgentEvent)>`. The `Transport` tag is load-bearing — the reducer uses it for hook-wins dedup. Do not hardcode `Transport::Hook` on the consumer side; the producer (each Source impl) tags its own events.
3. **`Source` trait is the only seam for adding agent CLIs** (Codex / Cursor / Gemini / Copilot). Don't bypass it.
4. **`install-hooks` writes through symlinks.** `resolve_symlink` in `install/io.rs` is critical for stow-managed `~/.claude/settings.json`. Don't replace it with `fs::rename` on the symlink path.
5. **The hook shim must never block CC.** Always exit 0 silently on any error. The 200ms write timeout is non-negotiable.

## Known sharp edges (don't be surprised by these)

- **CC hook payloads don't include `tool_use_id`.** Only the JSONL transcript carries the model-assigned id. The reducer's hook-wins dedup window therefore rarely fires for the common case. This is accepted — hooks fire ~ms before JSONL, so duplicate state writes re-set the same value rather than causing wrong state. A coarser per-session silencer is a candidate refinement.
- **`AgentSlot.state_started_at` is `std::time::Instant`** — process-local, not serializable. Will need to swap to `SystemTime` (or epoch-ms `u64`) before the v2 daemon split.
- **`draw_scene` is a free function on the binary side**, not a `Renderer` impl. The trait exists in `core`, and `draw_scene` is generic over `Backend`, but the production runtime calls it directly. Wire through the trait when the daemon split lands.
- **The `recolor_frame` shortcut substitutes by RGB equality.** Works because each palette key in the default pack maps to a unique RGB. If you add a sprite pack where two keys share a color, swap to a palette-key-indexed approach instead.

## Things NOT to do

- Don't add `ratatui` / `crossterm` / terminal anything to `ascii-agents-core`.
- Don't write to `~/.claude/settings.json` directly. Always go through `install/io.rs::write_settings_atomic` (advisory lock + atomic rename + symlink resolution).
- Don't add `println!` / `eprintln!` to any production path other than the headless summary and explicit user-facing CLI output. Use `tracing::{info, warn, error}` instead.
- Don't relax the hook shim's "always exit 0" contract. Blocking CC = breaking the user's primary workflow.
- Don't add `--no-verify` / hook-skipping flags to any git operations performed in this repo.
- Don't generate a README / CLAUDE.md / CHANGELOG / docs in PRs unless explicitly asked.
- Don't `git push` without explicit user confirmation, even after committing.

## Where to look

- "How does a CC tool call become a moving sprite?" → trace `runtime::run_async` → `ClaudeCodeSource::run` → `HookSocketListener::run` → `decoder::decode_hook_payload` → `reducer::Reducer::apply` → `tui::renderer::draw_scene`.
- "How is the office laid out?" → `tui::renderer::draw_scene`. Desks are a fixed-width slot grid; sprite anchors to `desk_y - 16`.
- "How does the default character pack get into the binary?" → `tui::embedded_pack` does the `include_str!` at compile time; `sprite::format::load_pack_from_strings` parses it.
- "How do hooks get installed?" → `install::merge::merge_install` for the JSON merge logic, `install::io::write_settings_atomic` for the safe filesystem write.

## When refactoring

If you change anything in the channel type, source trait, or reducer signature, update **all four** test files that exercise them: `tests/reducer.rs`, `tests/e2e.rs`, `tests/hook_socket.rs`, `tests/jsonl_watcher.rs`, plus `runtime.rs` on the binary side.
