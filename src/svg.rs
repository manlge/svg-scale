use anyhow::{Context, Result};
use crate::{path::scale_path, scale::ScaleCtx};
use roxmltree::Node;
use xmlwriter::XmlWriter;

/// Check if transform contains actual scale factor (not just translate or mirror/flip)
fn has_scale_in_transform(transform: &str) -> Result<bool> {
    // Check for scale() function - but need to check the actual value
    if let Some(captures) = regex::Regex::new(
        r"scale\((-?\d*\.?\d+)(?:\s*,\s*(-?\d*\.?\d+))?\)",
    )
    .context("invalid scale() regex")?
    .captures(transform)
    {
        let sx: f64 = captures
            .get(1)
            .context("missing scale x")?
            .as_str()
            .parse()
            .context("invalid scale x")?;
        let sy: f64 = captures
            .get(2)
            .map(|m| m.as_str().parse().context("invalid scale y"))
            .transpose()?
            .unwrap_or(sx);
        // Only consider it scaling if |sx| != 1 or |sy| != 1
        // (sx=-1 or sy=-1 is just mirroring, not actual scaling)
        if (sx.abs() - 1.0).abs() > 1e-6 || (sy.abs() - 1.0).abs() > 1e-6 {
            return Ok(true);
        }
    }
    // Check for matrix - matrix(a,b,c,d,e,f)
    // a and d control scaling, but -1 values indicate mirroring not scaling
    if let Some(captures) = regex::Regex::new(
        r"matrix\((-?\d*\.?\d+)\s*,\s*(-?\d*\.?\d+)\s*,\s*(-?\d*\.?\d+)\s*,\s*(-?\d*\.?\d+)\s*,",
    )
    .context("invalid matrix() regex")?
    .captures(transform)
    {
        let a: f64 = captures
            .get(1)
            .context("missing matrix a")?
            .as_str()
            .parse()
            .context("invalid matrix a")?;
        let d: f64 = captures
            .get(4)
            .context("missing matrix d")?
            .as_str()
            .parse()
            .context("invalid matrix d")?;
        // Only if |a| != 1 or |d| != 1 is there actual scaling
        // (a=-1 or d=-1 is just mirroring/flipping, coordinates stay same magnitude)
        if (a.abs() - 1.0).abs() > 1e-6 || (d.abs() - 1.0).abs() > 1e-6 {
            return Ok(true);
        }
    }
    Ok(false)
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
fn scale_transform_all(v: &str, scale: f64) -> Result<String> {
    let mut result = v.to_string();

    // Handle translate(x, y) or translate(x) - scale all occurrences
    let translate_re = regex::Regex::new(
        r"translate\(\s*(-?\d*\.?\d+(?:e[+-]?\d+)?)(?:\s*(?:,|\s)\s*(-?\d*\.?\d+(?:e[+-]?\d+)?))?\s*\)",
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
        r"rotate\(\s*(-?\d*\.?\d+(?:e[+-]?\d+)?)\s*(?:,|\s)\s*(-?\d*\.?\d+(?:e[+-]?\d+)?)\s*(?:,|\s)\s*(-?\d*\.?\d+(?:e[+-]?\d+)?)\s*\)",
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
        r"matrix\(\s*(-?\d*\.?\d+(?:e[+-]?\d+)?)\s*,?\s*(-?\d*\.?\d+(?:e[+-]?\d+)?)\s*,?\s*(-?\d*\.?\d+(?:e[+-]?\d+)?)\s*,?\s*(-?\d*\.?\d+(?:e[+-]?\d+)?)\s*,?\s*(-?\d*\.?\d+(?:e[+-]?\d+)?)\s*,?\s*(-?\d*\.?\d+(?:e[+-]?\d+)?)\s*\)",
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
            format!("matrix({},{},{},{},{},{})", a, b, c, d, e * scale, f * scale)
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

            // Check if this element has a non-translate transform
            let has_non_translate_transform = has_transform
                && has_scale_in_transform(transform_value).with_context(|| {
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
