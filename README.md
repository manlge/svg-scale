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
svg-scale -i input.svg --to 128 -o output.svg

# Batch output multiple sizes
svg-scale -i input.svg --to 16,32,48,128 --out-dir ./dist
```
Source size is auto-detected from the SVG when not specified.

### Options

| Option | Description |
|--------|-------------|
| `-i, --input <FILE>` | Input SVG file |
| `--vscode` | VSCode icon pipeline mode (512→128, outputs SVG+PNG) |
| `--to <SIZE\|LIST>` | Target size, e.g. `128` or `16,32,48` |
| `--scale <FLOAT>` | Direct scale ratio (highest priority) |
| `-o, --output <FILE>` | Output file (single size) |
| `--out-dir <DIR>` | Output directory (for --vscode or batch mode) |
| `--fix-stroke` | Remove non-scaling-stroke |
| `--precision <N>` | Decimal precision [default: 4] |

## What Is Scaled

This tool performs geometry-true scaling of path data, common shape attributes, and transform values.

Supported (tested) areas include:
- `path` data (including arc flags handling)
- `viewBox`
- Shape attributes: `x/y/cx/cy/r/rx/ry/x1/y1/x2/y2/width/height/stroke-width`
- Additional geometry attributes: `dx/dy`, `font-size`, `letter-spacing`, `stroke-dasharray`, `stroke-dashoffset`
- `style=""` inline properties for the above attributes (including `transform`)
- `<style>` rules with simple selectors: element, `.class`, `#id`, combined (e.g. `rect.big`, `rect#id`), one-level descendant (`A B`) and child (`A > B`)
- Transforms: `translate`, `rotate` (with center), `scale`, `matrix`
- Gradients: `linearGradient`/`radialGradient` geometry (`x1/y1/x2/y2/cx/cy/r/fx/fy`) and `gradientTransform`
- Patterns: `pattern` geometry (`x/y/width/height`) and `patternTransform`
- Masks and clip paths: `mask`/`clipPath` geometry; respects `maskUnits` / `clipPathUnits` `objectBoundingBox`
- Filters: `filter` regions and common primitive attributes (`dx/dy`, `stdDeviation`, `radius`, `scale`, `surfaceScale`, `kernelUnitLength`, light positions)
- Markers: `markerWidth/markerHeight/refX/refY` and marker content; respects `markerUnits`
- Non-scaling strokes (`vector-effect="non-scaling-stroke"`) preserve `stroke-width` unless `--fix-stroke` is used
- Scientific notation in transforms and `viewBox` (e.g. `1e2`, `1E2`)
- Length units: supports `px`, `pt`, `pc`, `mm`, `cm`, `in` (numbers are scaled, units preserved)
- Percent lengths are preserved (e.g. `50%` stays `50%`)

Fixtures and tests also cover transform combinations, nested groups, and skew transforms.

## Scope / Limitations

- CSS support is intentionally limited to simple selectors and one-level relationships; pseudo-classes, attribute selectors, and complex selector chains are not parsed.
- Only a subset of filter primitives and attributes are scaled; less common filter parameters may remain unchanged.
- Unit conversion is not performed (values are scaled, but units are preserved).

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
