# Monty character assets

Drop Monty's animation frames here. The egui GUI loads them at runtime by
**state name**, so you can replace/iterate on art without rebuilding.

## Naming

One file per state, named `<state>.gif` or `<state>.png`:

| File | State | Triggered when |
|------|-------|----------------|
| `sleeping.gif`     | Sleeping    | idle a long time / daemon disconnected |
| `idle.gif`         | Idle        | default resting state |
| `listening.gif`    | Listening   | voice capture active (mic open) |
| `thinking.gif`     | Thinking    | agent/LLM working a turn |
| `active.gif`       | Active      | chatting / handling an alert |
| `superactive.gif`  | SuperActive | critical: CPU > 80% or a crit alert firing |

(State names mirror monty-tui's `character.rs` for continuity — rename here and
in the loader if you'd prefer a different set.)

## Format

- **Animated `.gif` preferred**; static `.png` works too. The loader tries
  `<state>.gif` first, then `<state>.png`, then falls back to built-in ASCII art.
- **Size:** ~128×128 square, transparent background (matches `docs/logos/monty-128.png`).
- Animated GIFs advance automatically while the window is repainting.

Only the files you provide are used; missing states fall back to `idle`, then ASCII.
