# Monty source art (raw)

Original character art for Monty. **These are sources, not runtime assets** —
the egui loader reads `<state>.gif` / `<state>.png` from the parent directory
(`../README.md`), not from here. Nothing in this folder is loaded at runtime.

Committed **2026-07-15** as a backup: until now these existed only as loose
files on a single Windows box, in no repository and in no backup. They are
generated art and cannot be re-fetched. `docs/logos/` covers the *logo*
pipeline (`Monty_Lizard_Large.png` → square PNGs + ANSI); this folder covers
the *character pose/animation* set, which had no home.

## What's here

Stills (PNG) and animations (MP4), full-size:

| File | Reads as |
|---|---|
| `Monty_Character.png` | base character / neutral reference |
| `Monty_Scurry.png`, `monty_scurrying_anim.mp4` | scurrying |
| `monty_walking_around.mp4` | walking / ambient movement |
| `monty_looking_around.mp4`, `monty_interested.mp4` | looking around, curious |
| `monty_waving.png`, `monty_waving.mp4` | waving |
| `monty_alert_tall.png`, `monty_happy_alert_tall.png` | alert (neutral / happy) |
| `monty_sleeping.png`, `monty_falls_asleep.mp4` | asleep, falling asleep |
| `monty_wakes_surprised.png` | waking, surprised |
| `monty_animated_sound.mp4` | animation with audio |

## Not yet wired to states

The runtime loader wants ~128×128 transparent `<state>.gif` for the six states
in `../README.md` (`sleeping`, `idle`, `listening`, `thinking`, `active`,
`superactive`). This art is full-size PNG/MP4 under other names, so no state is
satisfied by it as-is — the GUI still falls back to built-in ASCII.

Deriving the state GIFs from these sources (e.g. `monty_falls_asleep.mp4` →
`sleeping.gif`, `monty_looking_around.mp4` → `idle.gif`,
`monty_animated_sound.mp4` → `listening.gif`) is a follow-up. The mapping is a
design call, not a mechanical one, so it is deliberately left undone here —
this commit only ensures the art survives.
