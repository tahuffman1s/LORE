# LORE (Lovely Open Rust Engine)

Free, open-source engine for 3D games played in first or third person, built single-player first.
Rust core, Luau scripting, Vulkan 1.3. Licensed MIT OR Apache-2.0. Forked from FOSSPS, a multiplayer FPS engine.

## Pillars (earlier wins when they conflict)

1. **Easy** — install to a game you can walk around in minutes. Script API reads like plain English. Error messages say what to do next. This is the "lovely".
2. **Fast** — high, stable frame rate and low input latency on mid-range hardware; a town of NPCs has a known cost.
3. **Focused** — 3D worlds a player walks through in first or third person: adventures, action games, shooters, stories. Decline what those do not need (2D, VR, mobile, node graphs, visual scripting).
4. **Modern** — current rendering techniques, upscaling, first-class controllers.

## Decisions

- Rust stable, edition 2024. Cargo workspace; crates live in `crates/`.
- One render backend: Vulkan 1.3 (Windows + Linux). FSR needs raw Vulkan handles, so no abstraction layer that hides them.
- Upscaling: FSR 3.1 (MIT) only. No closed-source upscalers (XeSS was dropped: closed, Windows-only).
- Scripting: Luau through `mlua` (`luau` feature). Sandboxed, hot-reloadable, typed API definitions shipped for editor autocomplete.
- Both views are first-class: anything that works in first person works in third, and the other way round. Switching is one line.
- Single-player first, on the multiplayer core. A game played alone still runs on the server-authoritative, deterministic simulation, inside the program. That is what makes a save a snapshot, a replay free, `lore check` able to live a day in seconds, and co-op a later switch instead of a rewrite. Never add a single-player shortcut that breaks it.
- A game is a folder of text you can read: rules (`main.luau`), what is where (`map.toml`), and, as they are built, people, conversations and quests. All hot-reload, merge cleanly, and are understood by `lore check`. Story state lives in one place, `game.lore`.
- NPC behaviour is a readable list of rules, and every NPC can say why it is doing what it does.
- Netcode: server-authoritative, client prediction, lag compensation. The headless dedicated server stays a first-class binary; multiplayer features are built last (M8).
- Controller parity: every action is bindable on a gamepad; nothing ships keyboard/mouse-only.
- Targets: Linux (`.tar.gz`, AppImage optional), Windows (`.exe`), macOS (`.app` through MoltenVK — stretch goal, feasibility unverified). Keep code portable; CI builds all three.

## Autonomy

The owner wants to give very little input. `/next` builds the next roadmap issue end to end.

- Decide, don't ask. Pick the option that best serves the pillars and log non-obvious choices as one line in `docs/decisions.md`.
- Don't use `/feature-dev`'s question phase; use its `code-architect` and `code-reviewer` agents directly.
- Verify your own work (check, clippy, tests, run it) before calling anything done.
- Stop only when the user alone can unblock: system packages, credentials, licensing, irreversible actions. Label the issue `blocked` and move to the next one.

## GitHub (`gh`, repo `tahuffman1s/LORE`)

- Issues are the source of truth. Roadmap work carries the `roadmap` label and a milestone (M0–M8); issue numbers are the build order; `ROADMAP.md` is only an overview. New work found along the way becomes an issue, not a TODO.
- Every change goes through a PR from a branch named `<issue>-<slug>`, with `Closes #<issue>`. Never commit to `main` directly, never force-push.
- The owner has authorized Claude to open, review and merge its own PRs without asking. Review the diff with `code-reviewer` before pushing. On the PR, the Claude GitHub app (`claude[bot]`, workflow `claude-code-review.yml`) gives an independent review: wait for it, then fix or answer every comment it leaves. Squash-merge once that is settled and CI (Linux, Windows, macOS) is green.
- Ask for JSON and only the fields you need (`gh ... --json number,title`); never dump full issue or PR lists.
- `upstream` is FOSSPS (`tahuffman1s/FOSSPS`). Fetch from it to bring fixes over; never push to it. `FOSSPS#N` in docs means an issue or PR there.
- Never publish releases, change repo settings or visibility, or delete branches other than your own merged ones without being asked.
- The website is `site/` (plain HTML, no build step), deployed to GitHub Pages on merge. Keep its claims in line with what actually works.

## Commands

```sh
cargo check -q --workspace --message-format=short
cargo clippy -q --workspace --all-targets --message-format=short -- -D warnings
cargo test -q --workspace
cargo fmt --all
```

A hook runs rustfmt on every edited `.rs` file; don't format by hand.

## Research first

Before designing anything a user touches (script API, CLI, editor, error text, docs, onboarding), search the web for how the best tools solve it and for newer ideas, then design. Use the `researcher` agent so the reading stays out of the main context. Note the sources that shaped the design in the commit or PR description.

## Working efficiently

- Navigate with the rust-analyzer LSP (definitions, references) instead of reading whole files.
- Look up crate APIs with context7 instead of guessing or reading vendored source.
- Send broad searches and web reading to subagents (`Explore`, `researcher`); keep only conclusions.
- Run `api-reviewer` on any new or changed script-facing API before finishing.
- After a feature lands: `code-simplifier`. Commits: `/commit`.
- Never read `target/` or `Cargo.lock`. Keep cargo output quiet (`-q`, short messages).
- Keep this file short. Put subsystem detail in `docs/` and link it here only when needed.

## Quality bar

- Clippy clean with `-D warnings`; workspace lints are in the root `Cargo.toml`.
- No `unwrap`/`expect`/panics on engine runtime paths; return errors with actionable messages.
- `unsafe` only at FFI and GPU boundaries, each block with a `// SAFETY:` comment.
- Simulation and netcode are deterministic and unit-tested. No per-frame heap allocation in hot loops.
- A performance claim needs a benchmark or a frame capture.
