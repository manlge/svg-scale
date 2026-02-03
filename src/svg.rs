use crate::{path::scale_path, scale::ScaleCtx};
use roxmltree::Node;
use xmlwriter::XmlWriter;

/// Check if transform contains actual scale factor (not just translate or mirror/flip)
fn has_scale_in_transform(transform: &str) -> bool {
    // Check for scale() function - but need to check the actual value
    if let Some(captures) = regex::Regex::new(r"scale\((-?\d*\.?\d+)(?:\s*,\s*(-?\d*\.?\d+))?\)")
        .unwrap()
        .captures(transform)
    {
        let sx: f64 = captures.get(1).unwrap().as_str().parse().unwrap();
        let sy: f64 = captures
            .get(2)
            .map(|m| m.as_str().parse().unwrap())
            .unwrap_or(sx);
        // Only consider it scaling if |sx| != 1 or |sy| != 1
        // (sx=-1 or sy=-1 is just mirroring, not actual scaling)
        if (sx.abs() - 1.0).abs() > 1e-6 || (sy.abs() - 1.0).abs() > 1e-6 {
            return true;
        }
    }
    // Check for matrix - matrix(a,b,c,d,e,f)
    // a and d control scaling, but -1 values indicate mirroring not scaling
    if let Some(captures) = regex::Regex::new(
        r"matrix\((-?\d*\.?\d+)\s*,\s*(-?\d*\.?\d+)\s*,\s*(-?\d*\.?\d+)\s*,\s*(-?\d*\.?\d+)\s*,",
    )
    .unwrap()
    .captures(transform)
    {
        let a: f64 = captures.get(1).unwrap().as_str().parse().unwrap();
        let d: f64 = captures.get(4).unwrap().as_str().parse().unwrap();
        // Only if |a| != 1 or |d| != 1 is there actual scaling
        // (a=-1 or d=-1 is just mirroring/flipping, coordinates stay same magnitude)
        if (a.abs() - 1.0).abs() > 1e-6 || (d.abs() - 1.0).abs() > 1e-6 {
            return true;
        }
    }
    false
}

/// Scale translate values in transform attribute (legacy, kept for potential future use)
/// translate(a,b) -> translate(a*scale,b*scale)
#[allow(dead_code)]
fn scale_transform(v: &str, scale: f64) -> String {
    // Match translate(a,b) or translate(a) pattern
    if let Some(captures) = regex::Regex::new(r"translate\((-?\d*\.?\d+)(?:,\s*(-?\d*\.?\d+))?\)")
        .unwrap()
        .captures(v)
    {
        let tx: f64 = captures.get(1).unwrap().as_str().parse().unwrap();
        let ty: f64 = captures
            .get(2)
            .map(|m| m.as_str().parse().unwrap())
            .unwrap_or(0.0);
        let suffix = captures.get(0).unwrap().as_str();

        // Remove translate from transform
        let rest = v.replace(suffix, "");

        format!(
            "{}{}translate({},{})",
            rest,
            if rest.ends_with('(') || rest.ends_with(' ') {
                ""
            } else {
                " "
            },
            tx * scale,
            ty * scale
        )
    } else {
        v.to_string()
    }
}

/// Scale all transform values appropriately
/// - translate(x,y): scale x and y
/// - rotate(angle, cx, cy): scale cx and cy (center point)
/// - matrix(a,b,c,d,e,f): scale e and f (translation components)
fn scale_transform_all(v: &str, scale: f64) -> String {
    let mut result = v.to_string();

    // Handle translate(x, y) or translate(x)
    if let Some(captures) = regex::Regex::new(
        r"translate\((-?\d*\.?\d+(?:e[+-]?\d+)?)(?:\s*,?\s*(-?\d*\.?\d+(?:e[+-]?\d+)?))?\)",
    )
    .unwrap()
    .captures(&result)
    {
        let tx: f64 = captures.get(1).unwrap().as_str().parse().unwrap();
        let ty: f64 = captures
            .get(2)
            .map(|m| m.as_str().parse().unwrap())
            .unwrap_or(0.0);
        let matched = captures.get(0).unwrap().as_str();
        let replacement = format!("translate({},{})", tx * scale, ty * scale);
        result = result.replace(matched, &replacement);
    }

    // Handle rotate(angle, cx, cy)
    if let Some(captures) = regex::Regex::new(r"rotate\((-?\d*\.?\d+(?:e[+-]?\d+)?)(?:\s*,?\s*(-?\d*\.?\d+(?:e[+-]?\d+)?)(?:\s*,?\s*(-?\d*\.?\d+(?:e[+-]?\d+)?))?)?\)")
        .unwrap()
        .captures(&result)
    {
        let angle: f64 = captures.get(1).unwrap().as_str().parse().unwrap();
        if let (Some(cx_match), Some(cy_match)) = (captures.get(2), captures.get(3)) {
            let cx: f64 = cx_match.as_str().parse().unwrap();
            let cy: f64 = cy_match.as_str().parse().unwrap();
            let matched = captures.get(0).unwrap().as_str();
            let replacement = format!("rotate({},{},{})", angle, cx * scale, cy * scale);
            result = result.replace(matched, &replacement);
        }
    }

    // Handle matrix(a, b, c, d, e, f) - scale translation components e and f
    if let Some(captures) = regex::Regex::new(r"matrix\((-?\d*\.?\d+(?:e[+-]?\d+)?)\s*,?\s*(-?\d*\.?\d+(?:e[+-]?\d+)?)\s*,?\s*(-?\d*\.?\d+(?:e[+-]?\d+)?)\s*,?\s*(-?\d*\.?\d+(?:e[+-]?\d+)?)\s*,?\s*(-?\d*\.?\d+(?:e[+-]?\d+)?)\s*,?\s*(-?\d*\.?\d+(?:e[+-]?\d+)?)\)")
        .unwrap()
        .captures(&result)
    {
        let a: f64 = captures.get(1).unwrap().as_str().parse().unwrap();
        let b: f64 = captures.get(2).unwrap().as_str().parse().unwrap();
        let c: f64 = captures.get(3).unwrap().as_str().parse().unwrap();
        let d: f64 = captures.get(4).unwrap().as_str().parse().unwrap();
        let e: f64 = captures.get(5).unwrap().as_str().parse().unwrap();
        let f: f64 = captures.get(6).unwrap().as_str().parse().unwrap();
        let matched = captures.get(0).unwrap().as_str();
        // Scale only the translation components (e, f)
        let replacement = format!("matrix({},{},{},{},{},{})", a, b, c, d, e * scale, f * scale);
        result = result.replace(matched, &replacement);
    }

    result
}

fn walk_impl(
    node: Node,
    w: &mut XmlWriter,
    ctx: &ScaleCtx,
    ancestor_has_non_translate_transform: bool,
) {
    match node.node_type() {
        roxmltree::NodeType::Element => {
            w.start_element(node.tag_name().name());

            // Check if this element has transform
            let transform_attr = node.attributes().find(|attr| attr.name() == "transform");
            let has_transform = transform_attr.is_some();
            let transform_value = transform_attr.map(|a| a.value()).unwrap_or("");

            // Check if this element has a non-translate transform
            let has_non_translate_transform =
                has_transform && has_scale_in_transform(transform_value);

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
                            v.to_string()
                        } else {
                            scale_path(v, ctx)
                        }
                    }

                    "stroke-width" | "width" | "height" | "x" | "y" | "cx" | "cy" | "r" | "rx"
                    | "ry" | "x1" | "y1" | "x2" | "y2" => {
                        ctx.fmt(v.parse::<f64>().unwrap() * ctx.scale)
                    }

                    "viewBox" => v
                        .split_whitespace()
                        .map(|n| ctx.fmt(n.parse::<f64>().unwrap() * ctx.scale))
                        .collect::<Vec<_>>()
                        .join(" "),

                    "transform" => scale_transform_all(v, ctx.scale),

                    _ => v.to_string(),
                };

                w.write_attribute(&k, &nv);
            }

            // Pass down whether there's a non-translate transform in the ancestry
            for c in node.children() {
                walk_impl(
                    c,
                    w,
                    ctx,
                    ancestor_has_non_translate_transform || has_non_translate_transform,
                );
            }

            w.end_element();
        }
        roxmltree::NodeType::Text => {
            w.write_text(node.text().unwrap_or(""));
        }
        _ => {}
    }
}

pub fn walk(node: Node, w: &mut XmlWriter, ctx: &ScaleCtx) {
    walk_impl(node, w, ctx, false)
}
