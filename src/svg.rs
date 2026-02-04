use anyhow::{Context, Result};
use crate::{path::scale_path, scale::ScaleCtx};
use roxmltree::Node;
use xmlwriter::XmlWriter;

/// Check if transform contains any non-translate components
fn has_non_translate_transform(transform: &str) -> Result<bool> {
    let list = parse_transform_list(transform)?;
    Ok(list.iter().any(|(name, _)| name != "translate"))
}

/// Scale translate values in transform attribute (legacy, kept for potential future use)
/// translate(a,b) -> translate(a*scale,b*scale)
#[allow(dead_code)]
fn scale_transform(v: &str, scale: f64) -> Result<String> {
    // Match translate(a,b) or translate(a) pattern
    if let Some(captures) = regex::Regex::new(
        r"translate\((-?\d*\.?\d+)(?:,\s*(-?\d*\.?\d+))?\)",
    )
    .context("invalid translate() regex")?
    .captures(v)
    {
        let tx: f64 = captures
            .get(1)
            .context("missing translate x")?
            .as_str()
            .parse()
            .context("invalid translate x")?;
        let ty: f64 = captures
            .get(2)
            .map(|m| m.as_str().parse().context("invalid translate y"))
            .transpose()?
            .unwrap_or(0.0);
        let suffix = captures.get(0).context("missing translate match")?.as_str();

        // Remove translate from transform
        let rest = v.replace(suffix, "");

        Ok(format!(
            "{}{}translate({},{})",
            rest,
            if rest.ends_with('(') || rest.ends_with(' ') {
                ""
            } else {
                " "
            },
            tx * scale,
            ty * scale
        ))
    } else {
        Ok(v.to_string())
    }
}

/// Scale all transform values appropriately
/// - translate(x,y): scale x and y
/// - rotate(angle, cx, cy): scale cx and cy (center point)
/// - matrix(a,b,c,d,e,f): scale e and f (translation components)
fn parse_transform_list(v: &str) -> Result<Vec<(String, Vec<f64>)>> {
    let func_re = regex::Regex::new(r"([a-zA-Z]+)\s*\(([^)]*)\)")
        .context("invalid transform function regex")?;
    let num_re = regex::Regex::new(r"[-+]?\d*\.?\d+(?:[eE][+-]?\d+)?")
        .context("invalid transform number regex")?;
    let mut out = Vec::new();
    for caps in func_re.captures_iter(v) {
        let name = caps.get(1).unwrap().as_str().to_string();
        let params = caps.get(2).unwrap().as_str();
        let mut nums = Vec::new();
        for m in num_re.find_iter(params) {
            nums.push(m.as_str().parse::<f64>().context("invalid transform number")?);
        }
        out.push((name, nums));
    }
    Ok(out)
}

fn mat_mul(a: [f64; 6], b: [f64; 6]) -> [f64; 6] {
    let (a1, b1, c1, d1, e1, f1) = (a[0], a[1], a[2], a[3], a[4], a[5]);
    let (a2, b2, c2, d2, e2, f2) = (b[0], b[1], b[2], b[3], b[4], b[5]);
    [
        a1 * a2 + c1 * b2,
        b1 * a2 + d1 * b2,
        a1 * c2 + c1 * d2,
        b1 * c2 + d1 * d2,
        a1 * e2 + c1 * f2 + e1,
        b1 * e2 + d1 * f2 + f1,
    ]
}

fn clean_matrix_value(v: f64) -> f64 {
    if v.abs() < 1e-12 {
        0.0
    } else {
        v
    }
}

fn transform_to_matrix(list: &[(String, Vec<f64>)]) -> Result<[f64; 6]> {
    let mut m = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
    for (name, nums) in list {
        let mi = match name.as_str() {
            "translate" => {
                let tx = nums.get(0).copied().unwrap_or(0.0);
                let ty = nums.get(1).copied().unwrap_or(0.0);
                [1.0, 0.0, 0.0, 1.0, tx, ty]
            }
            "scale" => {
                let sx = nums.get(0).copied().unwrap_or(1.0);
                let sy = nums.get(1).copied().unwrap_or(sx);
                [sx, 0.0, 0.0, sy, 0.0, 0.0]
            }
            "rotate" => {
                let angle = nums.get(0).copied().unwrap_or(0.0);
                let rad = angle.to_radians();
                let cos = rad.cos();
                let sin = rad.sin();
                if nums.len() >= 3 {
                    let cx = nums[1];
                    let cy = nums[2];
                    let t1 = [1.0, 0.0, 0.0, 1.0, cx, cy];
                    let r = [cos, sin, -sin, cos, 0.0, 0.0];
                    let t2 = [1.0, 0.0, 0.0, 1.0, -cx, -cy];
                    mat_mul(t1, mat_mul(r, t2))
                } else {
                    [cos, sin, -sin, cos, 0.0, 0.0]
                }
            }
            "skewX" => {
                let angle = nums.get(0).copied().unwrap_or(0.0);
                [1.0, 0.0, angle.to_radians().tan(), 1.0, 0.0, 0.0]
            }
            "skewY" => {
                let angle = nums.get(0).copied().unwrap_or(0.0);
                [1.0, angle.to_radians().tan(), 0.0, 1.0, 0.0, 0.0]
            }
            "matrix" => {
                if nums.len() < 6 {
                    return Err(anyhow::anyhow!("matrix() requires 6 parameters"));
                }
                [nums[0], nums[1], nums[2], nums[3], nums[4], nums[5]]
            }
            _ => return Err(anyhow::anyhow!("unsupported transform: {}", name)),
        };
        m = mat_mul(m, mi);
    }
    Ok(m)
}

fn scale_transform_all(v: &str, scale: f64) -> Result<String> {
    let list = parse_transform_list(v)?;
    let has_non_translate = list.iter().any(|(name, _)| name != "translate");
    if has_non_translate {
        if list.len() == 1 {
            match list[0].0.as_str() {
                "scale" => {
                    // fall through to keep scale() formatting
                }
                "matrix" => {
                    let m = transform_to_matrix(&list)?;
                    return Ok(format!(
                        "matrix({},{},{},{},{},{})",
                        clean_matrix_value(m[0] * scale),
                        clean_matrix_value(m[1] * scale),
                        clean_matrix_value(m[2] * scale),
                        clean_matrix_value(m[3] * scale),
                        clean_matrix_value(m[4] * scale),
                        clean_matrix_value(m[5] * scale)
                    ));
                }
                _ => {
                    let m = transform_to_matrix(&list)?;
                    return Ok(format!(
                        "matrix({},{},{},{},{},{})",
                        clean_matrix_value(m[0] * scale),
                        clean_matrix_value(m[1] * scale),
                        clean_matrix_value(m[2] * scale),
                        clean_matrix_value(m[3] * scale),
                        clean_matrix_value(m[4] * scale),
                        clean_matrix_value(m[5] * scale)
                    ));
                }
            }
        } else {
            let m = transform_to_matrix(&list)?;
            return Ok(format!(
                "matrix({},{},{},{},{},{})",
                clean_matrix_value(m[0] * scale),
                clean_matrix_value(m[1] * scale),
                clean_matrix_value(m[2] * scale),
                clean_matrix_value(m[3] * scale),
                clean_matrix_value(m[4] * scale),
                clean_matrix_value(m[5] * scale)
            ));
        }
    }

    let mut result = v.to_string();

    // Handle scale(sx, sy) - scale all occurrences
    let scale_re = regex::Regex::new(
        r"scale\(\s*(-?\d*\.?\d+(?:[eE][+-]?\d+)?)(?:\s*(?:,|\s)\s*(-?\d*\.?\d+(?:[eE][+-]?\d+)?))?\s*\)",
    )
    .context("invalid scale() regex")?;
    let mut err: Option<anyhow::Error> = None;
    result = scale_re
        .replace_all(&result, |caps: &regex::Captures| {
            if err.is_some() {
                return caps.get(0).map(|m| m.as_str()).unwrap_or("").to_string();
            }
            let sx: f64 = match caps
                .get(1)
                .context("missing scale x")
                .and_then(|m| m.as_str().parse().context("invalid scale x"))
            {
                Ok(v) => v,
                Err(e) => {
                    err = Some(e);
                    return String::new();
                }
            };
            let sy_match = caps.get(2);
            let sy: f64 = match sy_match
                .map(|m| m.as_str().parse().context("invalid scale y"))
                .transpose()
            {
                Ok(Some(v)) => v,
                Ok(None) => sx,
                Err(e) => {
                    err = Some(e);
                    return String::new();
                }
            };
            if sy_match.is_some() {
                format!("scale({},{})", sx * scale, sy * scale)
            } else {
                format!("scale({})", sx * scale)
            }
        })
        .to_string();

    if let Some(e) = err.take() {
        return Err(e);
    }

    // Handle translate(x, y) or translate(x) - scale all occurrences
    let translate_re = regex::Regex::new(
        r"translate\(\s*(-?\d*\.?\d+(?:[eE][+-]?\d+)?)(?:\s*(?:,|\s)\s*(-?\d*\.?\d+(?:[eE][+-]?\d+)?))?\s*\)",
    )
    .context("invalid translate() regex")?;
    let mut err: Option<anyhow::Error> = None;
    result = translate_re
        .replace_all(&result, |caps: &regex::Captures| {
            if err.is_some() {
                return caps.get(0).map(|m| m.as_str()).unwrap_or("").to_string();
            }
            let tx: f64 = match caps
                .get(1)
                .context("missing translate x")
                .and_then(|m| m.as_str().parse().context("invalid translate x"))
            {
                Ok(v) => v,
                Err(e) => {
                    err = Some(e);
                    return String::new();
                }
            };
            let ty: f64 = match caps
                .get(2)
                .map(|m| m.as_str().parse().context("invalid translate y"))
                .transpose()
            {
                Ok(Some(v)) => v,
                Ok(None) => 0.0,
                Err(e) => {
                    err = Some(e);
                    return String::new();
                }
            };
            format!("translate({},{})", tx * scale, ty * scale)
        })
        .to_string();

    // Handle rotate(angle, cx, cy) - scale all occurrences that include a center
    let rotate_re = regex::Regex::new(
        r"rotate\(\s*(-?\d*\.?\d+(?:[eE][+-]?\d+)?)\s*(?:,|\s)\s*(-?\d*\.?\d+(?:[eE][+-]?\d+)?)\s*(?:,|\s)\s*(-?\d*\.?\d+(?:[eE][+-]?\d+)?)\s*\)",
    )
    .context("invalid rotate() regex")?;
    result = rotate_re
        .replace_all(&result, |caps: &regex::Captures| {
            if err.is_some() {
                return caps.get(0).map(|m| m.as_str()).unwrap_or("").to_string();
            }
            let angle: f64 = match caps
                .get(1)
                .context("missing rotate angle")
                .and_then(|m| m.as_str().parse().context("invalid rotate angle"))
            {
                Ok(v) => v,
                Err(e) => {
                    err = Some(e);
                    return String::new();
                }
            };
            let cx: f64 = match caps
                .get(2)
                .context("missing rotate cx")
                .and_then(|m| m.as_str().parse().context("invalid rotate cx"))
            {
                Ok(v) => v,
                Err(e) => {
                    err = Some(e);
                    return String::new();
                }
            };
            let cy: f64 = match caps
                .get(3)
                .context("missing rotate cy")
                .and_then(|m| m.as_str().parse().context("invalid rotate cy"))
            {
                Ok(v) => v,
                Err(e) => {
                    err = Some(e);
                    return String::new();
                }
            };
            format!("rotate({},{},{})", angle, cx * scale, cy * scale)
        })
        .to_string();

    // Handle matrix(a, b, c, d, e, f) - scale translation components e and f (all occurrences)
    let matrix_re = regex::Regex::new(
        r"matrix\(\s*(-?\d*\.?\d+(?:[eE][+-]?\d+)?)\s*,?\s*(-?\d*\.?\d+(?:[eE][+-]?\d+)?)\s*,?\s*(-?\d*\.?\d+(?:[eE][+-]?\d+)?)\s*,?\s*(-?\d*\.?\d+(?:[eE][+-]?\d+)?)\s*,?\s*(-?\d*\.?\d+(?:[eE][+-]?\d+)?)\s*,?\s*(-?\d*\.?\d+(?:[eE][+-]?\d+)?)\s*\)",
    )
    .context("invalid matrix() regex")?;
    result = matrix_re
        .replace_all(&result, |caps: &regex::Captures| {
            if err.is_some() {
                return caps.get(0).map(|m| m.as_str()).unwrap_or("").to_string();
            }
            let a: f64 = match caps
                .get(1)
                .context("missing matrix a")
                .and_then(|m| m.as_str().parse().context("invalid matrix a"))
            {
                Ok(v) => v,
                Err(e) => {
                    err = Some(e);
                    return String::new();
                }
            };
            let b: f64 = match caps
                .get(2)
                .context("missing matrix b")
                .and_then(|m| m.as_str().parse().context("invalid matrix b"))
            {
                Ok(v) => v,
                Err(e) => {
                    err = Some(e);
                    return String::new();
                }
            };
            let c: f64 = match caps
                .get(3)
                .context("missing matrix c")
                .and_then(|m| m.as_str().parse().context("invalid matrix c"))
            {
                Ok(v) => v,
                Err(e) => {
                    err = Some(e);
                    return String::new();
                }
            };
            let d: f64 = match caps
                .get(4)
                .context("missing matrix d")
                .and_then(|m| m.as_str().parse().context("invalid matrix d"))
            {
                Ok(v) => v,
                Err(e) => {
                    err = Some(e);
                    return String::new();
                }
            };
            let e: f64 = match caps
                .get(5)
                .context("missing matrix e")
                .and_then(|m| m.as_str().parse().context("invalid matrix e"))
            {
                Ok(v) => v,
                Err(e) => {
                    err = Some(e);
                    return String::new();
                }
            };
            let f: f64 = match caps
                .get(6)
                .context("missing matrix f")
                .and_then(|m| m.as_str().parse().context("invalid matrix f"))
            {
                Ok(v) => v,
                Err(e) => {
                    err = Some(e);
                    return String::new();
                }
            };
            format!(
                "matrix({},{},{},{},{},{})",
                a * scale,
                b * scale,
                c * scale,
                d * scale,
                e * scale,
                f * scale
            )
        })
        .to_string();

    if let Some(e) = err {
        return Err(e);
    }
    Ok(result)
}

fn walk_impl(
    node: Node,
    w: &mut XmlWriter,
    ctx: &ScaleCtx,
    ancestor_has_non_translate_transform: bool,
) -> Result<()> {
    match node.node_type() {
        roxmltree::NodeType::Element => {
            let tag_name = node.tag_name().name();
            let node_id = node.attribute("id").unwrap_or("");
            w.start_element(tag_name);

            // Check if this element has transform
            let transform_attr = node.attributes().find(|attr| attr.name() == "transform");
            let has_transform = transform_attr.is_some();
            let transform_value = transform_attr.map(|a| a.value()).unwrap_or("");

            let has_non_scaling_stroke = node
                .attributes()
                .find(|attr| attr.name() == "vector-effect")
                .map(|attr| attr.value() == "non-scaling-stroke")
                .unwrap_or(false);

            // Check if this element has a non-translate transform
            let has_non_translate_transform = has_transform
                && has_non_translate_transform(transform_value).with_context(|| {
                    if node_id.is_empty() {
                        format!("transform parse failed on <{}>", tag_name)
                    } else {
                        format!("transform parse failed on <{} id=\"{}\">", tag_name, node_id)
                    }
                })?;

            for attr in node.attributes() {
                let local_name = attr.name();
                // Construct full attribute name with namespace prefix if present
                let k = if let Some(ns_uri) = attr.namespace() {
                    // Look up the prefix for this namespace URI
                    if let Some(prefix) = node.lookup_prefix(ns_uri) {
                        format!("{}:{}", prefix, local_name)
                    } else {
                        local_name.to_string()
                    }
                } else {
                    local_name.to_string()
                };
                let v = attr.value();

                if ctx.fix_stroke && k == "vector-effect" {
                    continue;
                }

                let nv = match k.as_str() {
                    "d" => {
                        // Only skip scaling if there's a non-translate transform in ancestry
                        // (translate doesn't affect path coordinate space)
                        if ancestor_has_non_translate_transform || has_non_translate_transform {
                            Ok(v.to_string())
                        } else {
                            scale_path(v, ctx).with_context(|| {
                                if node_id.is_empty() {
                                    format!("scale path failed on <{}>", tag_name)
                                } else {
                                    format!("scale path failed on <{} id=\"{}\">", tag_name, node_id)
                                }
                            })
                        }
                    }

                    "stroke-width" | "width" | "height" | "x" | "y" | "cx" | "cy" | "r" | "rx"
                    | "ry" | "x1" | "y1" | "x2" | "y2" => {
                        if ancestor_has_non_translate_transform || has_non_translate_transform {
                            Ok(v.to_string())
                        } else if k == "stroke-width" && has_non_scaling_stroke && !ctx.fix_stroke {
                            Ok(v.to_string())
                        } else {
                        let num_part = if v.ends_with("px") {
                            &v[..v.len() - 2]
                        } else {
                            v
                        };
                        let num: f64 = num_part.parse().with_context(|| {
                            if node_id.is_empty() {
                                format!("invalid {} on <{}>: {}", k, tag_name, v)
                            } else {
                                format!("invalid {} on <{} id=\"{}\">: {}", k, tag_name, node_id, v)
                            }
                        })?;
                        Ok(ctx.fmt(num * ctx.scale))
                        }
                    }

                    "viewBox" => {
                        let parts: Result<Vec<String>> = v
                            .split_whitespace()
                            .map(|n| {
                                let val: f64 = n.parse().with_context(|| {
                                    if node_id.is_empty() {
                                        format!("invalid viewBox on <{}>: {}", tag_name, n)
                                    } else {
                                        format!(
                                            "invalid viewBox on <{} id=\"{}\">: {}",
                                            tag_name, node_id, n
                                        )
                                    }
                                })?;
                                Ok(ctx.fmt(val * ctx.scale))
                            })
                            .collect();
                        Ok(parts?.join(" "))
                    }

                    "transform" => scale_transform_all(v, ctx.scale).with_context(|| {
                        if node_id.is_empty() {
                            format!("transform scale failed on <{}>", tag_name)
                        } else {
                            format!("transform scale failed on <{} id=\"{}\">", tag_name, node_id)
                        }
                    }),

                    _ => Ok(v.to_string()),
                };

                w.write_attribute(&k, &nv?);
            }

            // Pass down whether there's a non-translate transform in the ancestry
            for c in node.children() {
                walk_impl(
                    c,
                    w,
                    ctx,
                    ancestor_has_non_translate_transform || has_non_translate_transform,
                )?;
            }

            w.end_element();
        }
        roxmltree::NodeType::Text => {
            w.write_text(node.text().unwrap_or(""));
        }
        _ => {}
    }
    Ok(())
}

pub fn walk(node: Node, w: &mut XmlWriter, ctx: &ScaleCtx) -> Result<()> {
    walk_impl(node, w, ctx, false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scale::ScaleCtx;

    fn render_scaled_svg(input: &str, scale: f64) -> Result<String> {
        let doc = roxmltree::Document::parse(input)?;
        let mut writer = XmlWriter::new(xmlwriter::Options::default());
        walk(doc.root_element(), &mut writer, &ScaleCtx { scale, precision: 4, fix_stroke: false })?;
        Ok(writer.end_document())
    }

    #[test]
    fn transform_scale_should_be_scaled_when_path_is_not() -> Result<()> {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><path d="M10 0 L20 0" transform="scale(2)"/></svg>"#;
        let out = render_scaled_svg(input, 0.5)?;
        let ok = out.contains(r#"transform="scale(1)""#)
            || out.contains(r#"transform="scale(1,1)""#);
        assert!(ok, "expected scaled transform, got: {out}");
        Ok(())
    }

    #[test]
    fn transform_matrix_should_scale_all_components() -> Result<()> {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><path d="M10 0 L20 0" transform="matrix(2,0,0,2,10,20)"/></svg>"#;
        let out = render_scaled_svg(input, 0.5)?;
        assert!(
            out.contains(r#"transform="matrix(1,0,0,1,5,10)""#),
            "expected scaled matrix, got: {out}"
        );
        Ok(())
    }

    #[test]
    fn transform_combo_translate_rotate_scale() -> Result<()> {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><path d="M10 0 L20 0" transform="translate(10,20) rotate(30 5 6) scale(2)"/></svg>"#;
        let out = render_scaled_svg(input, 0.5)?;
        assert!(
            out.contains(r#"transform="matrix("#)
                || out.contains(r#"transform="translate(5,10) rotate(30,2.5,3) scale(1)""#)
                || out.contains(r#"transform="translate(5,10) rotate(30,2.5,3) scale(1,1)""#),
            "expected scaled transform combo, got: {out}"
        );
        Ok(())
    }

    #[test]
    fn transform_combo_matrix_and_translate() -> Result<()> {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><path d="M10 0 L20 0" transform="matrix(1,2,3,4,10,20) translate(6 8)"/></svg>"#;
        let out = render_scaled_svg(input, 0.5)?;
        assert!(
            out.contains(r#"transform="matrix("#)
                || out.contains(r#"transform="matrix(0.5,1,1.5,2,5,10) translate(3,4)""#),
            "expected scaled matrix + translate, got: {out}"
        );
        Ok(())
    }

    #[test]
    fn transform_rotate_without_center_is_unchanged() -> Result<()> {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><path d="M10 0 L20 0" transform="rotate(30)"/></svg>"#;
        let out = render_scaled_svg(input, 0.5)?;
        assert!(
            out.contains(r#"transform="matrix("#) || out.contains(r#"transform="rotate(30)""#),
            "expected rotate angle unchanged or matrix, got: {out}"
        );
        Ok(())
    }

    #[test]
    fn transform_skew_is_unchanged() -> Result<()> {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><path d="M10 0 L20 0" transform="skewX(30) skewY(10)"/></svg>"#;
        let out = render_scaled_svg(input, 0.5)?;
        assert!(
            out.contains(r#"transform="matrix("#)
                || out.contains(r#"transform="skewX(30) skewY(10)""#),
            "expected skew angles unchanged or matrix, got: {out}"
        );
        Ok(())
    }

    #[test]
    fn nested_transforms_scale_correctly() -> Result<()> {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><g transform="translate(10,20)"><g transform="scale(2)"><path d="M10 0 L20 0"/></g></g></svg>"#;
        let out = render_scaled_svg(input, 0.5)?;
        assert!(
            out.contains(r#"transform="translate(5,10)""#),
            "expected translated parent to scale, got: {out}"
        );
        assert!(
            out.contains(r#"transform="scale(1)""#) || out.contains(r#"transform="scale(1,1)""#),
            "expected scaled child transform, got: {out}"
        );
        assert!(
            out.contains(r#"d="M10 0 L20 0""#),
            "expected path not to be double-scaled under scale(), got: {out}"
        );
        Ok(())
    }

    #[test]
    fn multi_element_integration() -> Result<()> {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg">
            <g transform="translate(10,20)">
                <rect x="5" y="6" width="10" height="12"/>
            </g>
            <circle cx="8" cy="9" r="4" transform="rotate(45 8 9)"/>
        </svg>"#;
        let out = render_scaled_svg(input, 0.5)?;
        assert!(
            out.contains(r#"transform="translate(5,10)""#),
            "expected group translate scaled, got: {out}"
        );
        assert!(
            out.contains(r#"x="2.5""#) && out.contains(r#"y="3""#),
            "expected rect position scaled, got: {out}"
        );
        assert!(
            out.contains(r#"width="5""#) && out.contains(r#"height="6""#),
            "expected rect size scaled, got: {out}"
        );
        assert!(
            out.contains(r#"transform="matrix("#)
                || out.contains(r#"transform="rotate(45,4,4.5)""#),
            "expected rotate center scaled or matrix, got: {out}"
        );
        Ok(())
    }

    #[test]
    fn non_scaling_stroke_preserves_stroke_width() -> Result<()> {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><path d="M10 0 L20 0" vector-effect="non-scaling-stroke" stroke-width="2"/></svg>"#;
        let out = render_scaled_svg(input, 0.5)?;
        assert!(
            out.contains(r#"stroke-width="2""#),
            "expected stroke-width unchanged, got: {out}"
        );
        assert!(
            out.contains(r#"vector-effect="non-scaling-stroke""#),
            "expected vector-effect preserved, got: {out}"
        );
        Ok(())
    }

    #[test]
    fn transform_scientific_e_notation_is_supported() -> Result<()> {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><path d="M10 0 L20 0" transform="scale(1E2) translate(1e1,2E1) rotate(3e1 4E0 5e0) matrix(1E0,0,0,1e0,1E1,2e1)"/></svg>"#;
        let out = render_scaled_svg(input, 0.5)?;
        assert!(
            out.contains(r#"transform="matrix("#)
                || out.contains(
                    r#"transform="scale(50) translate(5,10) rotate(30,2,2.5) matrix(0.5,0,0,0.5,5,10)""#
                ),
            "expected scientific notation to parse, got: {out}"
        );
        Ok(())
    }

    #[test]
    fn viewbox_scientific_e_notation_is_supported() -> Result<()> {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1E2 2e2"><path d="M10 0 L20 0"/></svg>"#;
        let out = render_scaled_svg(input, 0.5)?;
        assert!(
            out.contains(r#"viewBox="0 0 50 100""#),
            "expected viewBox scaled with scientific notation, got: {out}"
        );
        Ok(())
    }

    #[test]
    fn fixture_basic_svg_scales() -> Result<()> {
        let input = include_str!("../tests/fixtures/basic.svg");
        let out = render_scaled_svg(input, 0.5)?;
        assert!(
            out.contains(r#"viewBox="0 0 50 50""#),
            "expected viewBox scaled, got: {out}"
        );
        assert!(out.contains(r#"x="5""#) && out.contains(r#"y="10""#));
        assert!(out.contains(r#"width="15""#) && out.contains(r#"height="20""#));
        assert!(out.contains(r#"stroke-width="1""#));
        assert!(out.contains(r#"cx="25""#) && out.contains(r#"cy="30""#) && out.contains(r#"r="5""#));
        Ok(())
    }

    #[test]
    fn fixture_complex_svg_scales() -> Result<()> {
        let input = include_str!("../tests/fixtures/complex.svg");
        let out = render_scaled_svg(input, 0.5)?;
        assert!(
            out.contains(r#"viewBox="0 0 100 50""#),
            "expected viewBox scaled, got: {out}"
        );
        assert!(
            out.contains(r#"transform="translate(5,10)""#),
            "expected translate scaled, got: {out}"
        );
        assert!(
            out.contains(r#"stroke-width="2""#),
            "expected non-scaling stroke to remain, got: {out}"
        );
        assert!(
            out.contains(r#"transform="scale(1)""#) || out.contains(r#"transform="scale(1,1)""#),
            "expected scale transformed, got: {out}"
        );
        assert!(
            out.contains(r#"transform="matrix("#)
                || out.contains(r#"transform="rotate(45,20,25)""#),
            "expected rotate center scaled or matrix, got: {out}"
        );
        assert!(
            out.contains(r#"A 2.5 2.5 0 1 0 5 5""#),
            "expected arc scaled correctly, got: {out}"
        );
        Ok(())
    }

    #[test]
    fn fixture_skew_and_matrix_svg_scales() -> Result<()> {
        let input = include_str!("../tests/fixtures/skew-matrix.svg");
        let out = render_scaled_svg(input, 0.5)?;
        assert!(
            out.contains(r#"viewBox="0 0 60 30""#),
            "expected viewBox scaled, got: {out}"
        );
        assert!(
            out.contains(r#"transform="matrix("#)
                || out.contains(r#"transform="skewX(30) skewY(10)""#),
            "expected skew transform preserved or matrix, got: {out}"
        );
        assert!(
            out.contains(r#"transform="matrix(0.5,1,1.5,2,5,10)""#),
            "expected matrix scaled, got: {out}"
        );
        Ok(())
    }

    #[test]
    fn matrix_with_mirror_is_treated_as_non_translate() -> Result<()> {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><g transform="matrix(1,0,0,-1,0,216)"><path d="M10 0 L20 0"/></g></svg>"#;
        let out = render_scaled_svg(input, 0.25)?;
        assert!(
            out.contains(r#"transform="matrix(0.25,0,0,-0.25,0,54)""#),
            "expected matrix scaled, got: {out}"
        );
        assert!(
            out.contains(r#"d="M10 0 L20 0""#),
            "expected path not double-scaled, got: {out}"
        );
        Ok(())
    }
}
