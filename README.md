# svg-scale

A geometry-true SVG scaling tool with a built-in **VSCode Extension icon pipeline**.

## Why

- No Inkscape
- No viewBox fake scaling
- Real geometry scaling (path / stroke / shapes / transforms)
- CI friendly
- Handles complex SVG transforms correctly

## Install

```bash
cargo install --path .
```

## Usage

### VSCode Icon Mode (recommended)

Source of truth: **512x512 SVG**

```bash
svg-scale -i icon-512.svg --vscode
```

Custom output directory:

```bash
svg-scale -i icon-512.svg --vscode --out-dir ./custom-dist
```

Outputs:

```
images/dist/
├── icon.svg   # 128x128 SVG (geometry scaled)
└── icon.png   # 128x128 PNG (Marketplace ready)
```

### Direct Scale

```bash
# Specify scale ratio directly
svg-scale -i input.svg --scale 0.5 -o output.svg

# Calculate scale from source/target size
svg-scale -i input.svg --from 512 --to 128 -o output.svg

# Batch output multiple sizes
svg-scale -i input.svg --from 512 --to 16,32,48,128 --out-dir ./dist
```

### Options

| Option | Description |
|--------|-------------|
| `-i, --input <FILE>` | Input SVG file |
| `--vscode` | VSCode icon pipeline mode (512→128, outputs SVG+PNG) |
| `--from <SIZE>` | Original size (optional, auto-detected from SVG) |
| `--to <SIZE\|LIST>` | Target size, e.g. `128` or `16,32,48` |
| `--scale <FLOAT>` | Direct scale ratio (highest priority) |
| `-o, --output <FILE>` | Output file (single size) |
| `--out-dir <DIR>` | Output directory (for --vscode or batch mode) |
| `--fix-stroke` | Remove non-scaling-stroke |
| `--precision <N>` | Decimal precision [default: 4] |

## Requirements

- Rust
- `rsvg-convert` (from librsvg, for `--vscode` mode)

```bash
brew install librsvg
```

## package.json

```json
{
  "icon": "images/dist/icon.png"
}
```
