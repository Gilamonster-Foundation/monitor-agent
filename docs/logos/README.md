# monitor-agent Logos

Source image: `Monty_Lizard_Large.png` (1536×1024, RGBA).

Generated with `chafa 1.18.2` from that source.

## Assets

| File | Width | Type |
|---|---|---|
| `monty-ansi-10.txt` | 10 cols | Truecolor ANSI half-blocks |
| `monty-ansi-20.txt` | 20 cols | Truecolor ANSI half-blocks |
| `monty-ansi-40.txt` | 40 cols | Truecolor ANSI half-blocks |
| `monty-ansi-80.txt` | 80 cols | Truecolor ANSI half-blocks |
| `monty-ansi-120.txt` | 120 cols | Truecolor ANSI half-blocks |
| `monty-ansi-160.txt` | 160 cols | Truecolor ANSI half-blocks |
| `monty-ansi-full.txt` | 160 cols | Truecolor ANSI half-blocks (alias) |
| `monty-ascii-10.txt` | 10 cols | Plain ASCII, no color |
| `monty-ascii-20.txt` | 20 cols | Plain ASCII, no color |
| `monty-ascii-40.txt` | 40 cols | Plain ASCII, no color |
| `monty-ascii-80.txt` | 80 cols | Plain ASCII, no color |
| `monty-ascii-color-10.txt` | 10 cols | ASCII chars + ANSI color |
| `monty-ascii-color-20.txt` | 20 cols | ASCII chars + ANSI color |
| `monty-ascii-color-40.txt` | 40 cols | ASCII chars + ANSI color |
| `monty-ascii-color-80.txt` | 80 cols | ASCII chars + ANSI color |

## Regenerating

```bash
IMG=docs/logos/Monty_Lizard_Large.png
OUT=docs/logos

# ANSI half-block (truecolor)
for W in 10 20 40 80 120 160; do
    chafa --format ansi --colors full --symbols half --size ${W}x999 "$IMG" > "${OUT}/monty-ansi-${W}.txt"
done
cp "${OUT}/monty-ansi-160.txt" "${OUT}/monty-ansi-full.txt"

# Plain ASCII
for W in 10 20 40 80; do
    chafa --format symbols --colors none --symbols ascii --size ${W}x999 "$IMG" > "${OUT}/monty-ascii-${W}.txt"
done

# Color ASCII
for W in 10 20 40 80; do
    chafa --format symbols --colors full --symbols ascii --size ${W}x999 "$IMG" > "${OUT}/monty-ascii-color-${W}.txt"
done
```

The TUI splash screen selects the appropriate file based on terminal width:
`≤20 → 10 | ≤40 → 20 | ≤80 → 40 | ≤120 → 80 | ≤160 → 120 | >160 → 160`
