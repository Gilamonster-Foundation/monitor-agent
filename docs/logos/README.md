# monitor-agent Logos

Source image: `Monty_Lizard_Large.png` (1536×1024, RGBA).

Square PNG assets generated with Pillow (centered, transparent background).
ANSI/ASCII art generated with `chafa 1.18.2`.

## Square PNG Assets

| File | Size | Use case |
|---|---|---|
| `monty-256.png` | 256×256 | README header, docs |
| `monty-128.png` | 128×128 | App icon, sidebar |
| `monty-64.png` | 64×64 | Toolbar, avatar |
| `monty-32.png` | 32×32 | Favicon, small icon |
| `monty-16.png` | 16×16 | Browser tab favicon |

<p align="center">
  <img src="monty-256.png" width="256" />
  &nbsp;&nbsp;
  <img src="monty-128.png" width="128" />
  &nbsp;&nbsp;
  <img src="monty-64.png" width="64" />
  &nbsp;&nbsp;
  <img src="monty-32.png" width="32" />
  &nbsp;&nbsp;
  <img src="monty-16.png" width="16" />
</p>

### Regenerating square PNGs

```bash
~/venv/bin/python3 - << 'EOF'
from PIL import Image

src = "docs/logos/Monty_Lizard_Large.png"
out = "docs/logos"

img = Image.open(src).convert("RGBA")
w, h = img.size
side = max(w, h)
square = Image.new("RGBA", (side, side), (0, 0, 0, 0))
square.paste(img, ((side - w) // 2, (side - h) // 2))

for size in [256, 128, 64, 32, 16]:
    square.resize((size, size), Image.LANCZOS).save(f"{out}/monty-{size}.png")
EOF
```

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
