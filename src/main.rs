use anyhow::*;
use clap::Parser;
use std::result::Result::Ok;
use std::{fs, path::Path, process::Command};

mod path;
mod scale;
mod svg;
mod transform;

use scale::ScaleCtx;

#[derive(Parser)]
struct Cli {
    /// 输入 SVG 文件
    #[arg(short, long)]
    input: String,

    #[arg(long)]
    vscode: bool,

    #[arg(long, default_value = "4")]
    precision: usize,

    /// 原始尺寸（可选）
    #[arg(long)]
    from: Option<f64>,

    /// 目标尺寸，如 128 或 16,32,48
    #[arg(long)]
    to: Option<String>,

    /// 直接指定比例（优先级最高）
    #[arg(long)]
    scale: Option<f64>,

    /// 输出文件（单尺寸）
    #[arg(short, long)]
    output: Option<String>,

    /// 批量输出目录
    #[arg(long)]
    out_dir: Option<String>,

    /// 移除 non-scaling-stroke
    #[arg(long)]
    fix_stroke: bool,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    if cli.vscode {
        vscode_pipeline(&cli)?;
    } else {
        normal_pipeline(&cli)?;
    }

    Ok(())
}

fn write_svg(doc: &roxmltree::Document, ctx: &ScaleCtx) -> Result<String> {
    let mut writer = xmlwriter::XmlWriter::new(xmlwriter::Options::default());
    svg::walk(doc.root_element(), &mut writer, ctx)?;
    let mut svg = writer.end_document();

    // Prepend XML declaration
    svg.insert_str(
        0,
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"no\"?>\n",
    );

    // Preserve namespace declarations from root element
    let mut ns_decls: Vec<String> = Vec::new();
    for ns in doc.root_element().namespaces() {
        if let Some(name) = ns.name() {
            ns_decls.push(format!(" xmlns:{}=\"{}\"", name, ns.uri()));
        } else {
            ns_decls.push(format!(" xmlns=\"{}\"", ns.uri()));
        }
    }

    // Insert namespace declarations after the opening <svg tag
    if let Some(pos) = svg.find("<svg") {
        if let Some(end_pos) = svg[pos..].find('>') {
            let insert_pos = pos + end_pos;
            let ns_str = ns_decls.join("");
            svg.insert_str(insert_pos, &ns_str);
        }
    }

    Ok(svg)
}

fn get_svg_size(doc: &roxmltree::Document) -> Option<f64> {
    let root = doc.root_element();
    // Try width attribute first
    if let Some(w) = root.attribute("width") {
        // Remove "px" if present and parse
        let w_str = w.trim_end_matches("px");
        if let Ok(val) = w_str.parse::<f64>() {
            return Some(val);
        }
    }
    // Try viewBox
    if let Some(view_box) = root.attribute("viewBox") {
        let parts: Vec<&str> = view_box.split_whitespace().collect();
        if parts.len() == 4 {
            if let Ok(w) = parts[2].parse::<f64>() {
                return Some(w);
            }
        }
    }
    None
}

fn normal_pipeline(cli: &Cli) -> Result<()> {
    // 1. Parse SVG first
    let input_svg = fs::read_to_string(&cli.input)?;
    let doc = roxmltree::Document::parse(&input_svg)?;

    // 2. Determine 'from' size
    let from_size = if let Some(f) = cli.from {
        f
    } else {
        match get_svg_size(&doc) {
            Some(s) => {
                println!("自动检测到原始尺寸: {}", s);
                s
            }
            None => bail!("未能从SVG检测到尺寸，请使用 --from 指定原始尺寸"),
        }
    };

    // 3. Calculate scale or output modes
    // Check if we are in single output mode or multi-output directory mode
    if let Some(out_dir) = &cli.out_dir {
        // Multi-file output mode (requires --to)
        let to_str = cli
            .to
            .as_ref()
            .context("批量输出模式需要指定 --to (例如: --to 16,32,48)")?;
        let to_values: Vec<f64> = to_str
            .split(',')
            .map(|s| s.trim().parse())
            .collect::<Result<_, _>>()?;

        fs::create_dir_all(out_dir)?;
        for &to_size in to_values.iter() {
            let scale_i = to_size / from_size;
            let ctx_i = ScaleCtx {
                scale: scale_i,
                precision: cli.precision,
                fix_stroke: cli.fix_stroke,
            };

            let svg_i = write_svg(&doc, &ctx_i)?;

            let name = if to_values.len() == 1 {
                "icon.svg".to_string()
            } else {
                format!("icon-{}.svg", to_size as u32)
            };
            let out_path = Path::new(out_dir).join(&name);
            fs::write(&out_path, &svg_i)?;
            println!("输出: {}", out_path.display());
        }
        return Ok(());
    }

    // Single file output or stdout mode
    let scale = if let Some(s) = cli.scale {
        s
    } else if let Some(to_str) = &cli.to {
        // Only verify first value if multiple provided, though single output usually implies single 'to'
        let to_values: Vec<f64> = to_str
            .split(',')
            .map(|s| s.trim().parse())
            .collect::<Result<_, _>>()?;
        // Use the first target size for single file output
        to_values[0] / from_size
    } else {
        bail!("必须指定 --scale 或 --to");
    };

    let ctx = ScaleCtx {
        scale,
        precision: cli.precision,
        fix_stroke: cli.fix_stroke,
    };

    let scaled_svg = write_svg(&doc, &ctx)?;

    // Output file
    if let Some(output) = &cli.output {
        fs::write(output, &scaled_svg)?;
        println!("输出: {}", output);
    } else {
        // Default to stdout
        println!("{}", scaled_svg);
    }

    Ok(())
}

fn vscode_pipeline(cli: &Cli) -> Result<()> {
    let scale = 128.0 / 512.0;

    let ctx = ScaleCtx {
        scale,
        precision: cli.precision,
        fix_stroke: true,
    };

    let input_svg = fs::read_to_string(&cli.input)?;
    let doc = roxmltree::Document::parse(&input_svg)?;

    let scaled_svg = write_svg(&doc, &ctx)?;

    // Use --out-dir if provided, otherwise default to images/dist
    let out_dir: &Path = if let Some(dir) = &cli.out_dir {
        Path::new(dir)
    } else {
        Path::new("images/dist")
    };
    fs::create_dir_all(out_dir)?;

    let svg_out = out_dir.join("icon.svg");
    fs::write(&svg_out, &scaled_svg)?;

    let png_out = out_dir.join("icon.png");

    let status = Command::new("rsvg-convert")
        .arg(svg_out.to_str().context("non-utf8 svg path")?)
        .arg("-w")
        .arg("128")
        .arg("-h")
        .arg("128")
        .arg("-o")
        .arg(png_out.to_str().context("non-utf8 png path")?)
        .status()
        .context("failed to execute rsvg-convert")?;

    if !status.success() {
        bail!("rsvg-convert failed");
    }

    println!("VSCode icon generated:");
    println!("  {}", svg_out.display());
    println!("  {}", png_out.display());

    Ok(())
}
