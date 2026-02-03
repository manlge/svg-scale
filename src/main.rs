use anyhow::*;
use clap::Parser;
use std::{fs, path::Path, process::Command};

mod path;
mod scale;
mod svg;

use scale::ScaleCtx;

#[derive(Parser)]
struct Cli {
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
    svg::walk(doc.root_element(), &mut writer, ctx);
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

fn normal_pipeline(cli: &Cli) -> Result<()> {
    // 计算比例
    let scale = if let Some(s) = cli.scale {
        s
    } else if let (Some(from), Some(to)) = (cli.from, &cli.to) {
        let to_values: Vec<f64> = to
            .split(',')
            .map(|s| s.trim().parse())
            .collect::<Result<_, _>>()?;
        to_values[0] / from
    } else {
        bail!("必须指定 --scale 或 --from 和 --to");
    };

    let ctx = ScaleCtx {
        scale,
        precision: cli.precision,
        fix_stroke: cli.fix_stroke,
    };

    let input_svg = fs::read_to_string(&cli.input)?;
    let doc = roxmltree::Document::parse(&input_svg)?;

    let scaled_svg = write_svg(&doc, &ctx)?;

    // 输出文件
    if let Some(output) = &cli.output {
        fs::write(output, &scaled_svg)?;
        println!("输出: {}", output);
    } else if let Some(out_dir) = &cli.out_dir {
        let to_values: Vec<f64> = cli
            .to
            .as_ref()
            .unwrap()
            .split(',')
            .map(|s| s.trim().parse())
            .collect::<Result<_, _>>()?;

        fs::create_dir_all(out_dir)?;
        for (_, &to_size) in to_values.iter().enumerate() {
            let scale_i = to_size / cli.from.unwrap_or(1.0);
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
    } else {
        // 默认输出到 stdout
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

    let out_dir = Path::new("images/dist");
    fs::create_dir_all(out_dir)?;

    let svg_out = out_dir.join("icon.svg");
    fs::write(&svg_out, &scaled_svg)?;

    let png_out = out_dir.join("icon.png");

    let status = Command::new("rsvg-convert")
        .arg(svg_out.to_str().unwrap())
        .arg("-w")
        .arg("128")
        .arg("-h")
        .arg("128")
        .arg("-o")
        .arg(png_out.to_str().unwrap())
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
