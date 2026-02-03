# svg-scale

A geometry-true SVG scaling tool with a built-in **VSCode Extension icon pipeline**.

## Why

- No Inkscape
- No viewBox fake scaling
- Real geometry scaling (path / stroke / shapes)
- CI friendly

## Install

```bash
cargo install --path .
```

## Usage

### VSCode Icon Mode (recommended)

Source of truth: **512x512 SVG**

```bash
svg-scale icon-512.svg --vscode
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
svg-scale input.svg --scale 0.5 -o output.svg

# Calculate scale from source/target size
svg-scale input.svg --from 512 --to 128 -o output.svg

# Batch output multiple sizes
svg-scale input.svg --from 512 --to 16,32,48,128 --out-dir ./dist
```

### Options

| Option | Description |
|--------|-------------|
| `--from <SIZE>` | Original size (optional) |
| `--to <SIZE\|LIST>` | Target size, e.g. `128` or `16,32,48` |
| `--scale <FLOAT>` | Direct scale ratio (highest priority) |
| `-o, --output <FILE>` | Output file (single size) |
| `--out-dir <DIR>` | Batch output directory |
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
