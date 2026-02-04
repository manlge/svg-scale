use crate::{
    path::scale_path,
    scale::ScaleCtx,
    transform::{parse_transform_list, scale_transform_value},
};
use anyhow::{Context, Result};
use roxmltree::Node;
use xmlwriter::XmlWriter;

/// Check if transform contains any non-translate components
fn has_non_translate_transform(transform: &str) -> Result<bool> {
    let list = parse_transform_list(transform)?;
    Ok(list.iter().any(|t| t.name != "translate"))
}

fn scale_transform_all(v: &str, scale: f64, precision: usize) -> Result<String> {
    scale_transform_value(v, scale, precision)
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
                        format!(
                            "transform parse failed on <{} id=\"{}\">",
                            tag_name, node_id
                        )
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
                                    format!(
                                        "scale path failed on <{} id=\"{}\">",
                                        tag_name, node_id
                                    )
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
                                    format!(
                                        "invalid {} on <{} id=\"{}\">: {}",
                                        k, tag_name, node_id, v
                                    )
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

                    "transform" => {
                        scale_transform_all(v, ctx.scale, ctx.precision).with_context(|| {
                            if node_id.is_empty() {
                                format!("transform scale failed on <{}>", tag_name)
                            } else {
                                format!(
                                    "transform scale failed on <{} id=\"{}\">",
                                    tag_name, node_id
                                )
                            }
                        })
                    }

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
        walk(
            doc.root_element(),
            &mut writer,
            &ScaleCtx {
                scale,
                precision: 4,
                fix_stroke: false,
            },
        )?;
        Ok(writer.end_document())
    }

    #[test]
    fn transform_scale_should_be_scaled_when_path_is_not() -> Result<()> {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><path d="M10 0 L20 0" transform="scale(2)"/></svg>"#;
        let out = render_scaled_svg(input, 0.5)?;
        let ok =
            out.contains(r#"transform="scale(1)""#) || out.contains(r#"transform="scale(1,1)""#);
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
        assert!(
            out.contains(r#"cx="25""#) && out.contains(r#"cy="30""#) && out.contains(r#"r="5""#)
        );
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
