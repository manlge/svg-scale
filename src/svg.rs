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

#[derive(Debug, Clone)]
struct StyleRule {
    selector: StyleSelector,
    props: Vec<(String, String)>,
    specificity: u32,
    order: u32,
}

#[derive(Debug, Clone)]
struct SimpleSelector {
    element: Option<String>,
    id: Option<String>,
    classes: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SelectorRelation {
    Descendant,
    Child,
}

#[derive(Debug, Clone)]
struct StyleSelector {
    ancestor: Option<SimpleSelector>,
    relation: Option<SelectorRelation>,
    target: SimpleSelector,
}

fn scale_transform_all(v: &str, scale: f64, precision: usize) -> Result<String> {
    scale_transform_value(v, scale, precision)
}

fn parse_style(input: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for part in input.split(';') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let mut it = part.splitn(2, ':');
        let key = it.next().unwrap_or("").trim();
        let val = it.next().unwrap_or("").trim();
        if key.is_empty() || val.is_empty() {
            continue;
        }
        out.push((key.to_string(), val.to_string()));
    }
    out
}

fn is_num_char(c: char) -> bool {
    c.is_ascii_digit() || matches!(c, '-' | '+' | '.' | 'e' | 'E')
}

fn is_supported_unit(unit: &str) -> bool {
    matches!(unit, "" | "px" | "pt" | "pc" | "mm" | "cm" | "in")
}

fn split_num_and_unit(token: &str) -> (&str, &str) {
    let mut idx = 0;
    for (i, c) in token.char_indices() {
        if is_num_char(c) {
            idx = i + c.len_utf8();
        } else {
            break;
        }
    }
    let (num, unit) = token.split_at(idx);
    (num, unit)
}

fn scale_number_token(token: &str, ctx: &ScaleCtx) -> Option<String> {
    let t = token.trim();
    if t.is_empty() {
        return None;
    }
    let (num_part, unit) = split_num_and_unit(t);
    if num_part.is_empty() {
        return None;
    }
    let unit = unit.trim();
    if unit == "%" {
        return None;
    }
    if !is_supported_unit(unit) {
        return None;
    }
    let num: f64 = num_part.parse().ok()?;
    let mut out = ctx.fmt(num * ctx.scale);
    if !unit.is_empty() {
        out.push_str(unit);
    }
    Some(out)
}

fn scale_number_list(value: &str, ctx: &ScaleCtx) -> String {
    let mut out = String::with_capacity(value.len());
    let mut buf = String::new();

    let flush_buf = |out: &mut String, buf: &mut String| {
        if buf.is_empty() {
            return;
        }
        if let Some(scaled) = scale_number_token(buf, ctx) {
            out.push_str(&scaled);
        } else {
            out.push_str(buf);
        }
        buf.clear();
    };

    for c in value.chars() {
        if is_num_char(c) || c.is_ascii_alphabetic() {
            buf.push(c);
        } else {
            flush_buf(&mut out, &mut buf);
            out.push(c);
        }
    }
    flush_buf(&mut out, &mut buf);
    out
}

fn scale_number_list_inverse(value: &str, ctx: &ScaleCtx) -> String {
    if ctx.scale == 0.0 {
        return value.to_string();
    }
    let inv = 1.0 / ctx.scale;
    let mut out = String::with_capacity(value.len());
    let mut buf = String::new();

    let flush_buf = |out: &mut String, buf: &mut String| {
        if buf.is_empty() {
            return;
        }
        if let Some(scaled) = scale_number_token(buf, &ScaleCtx { scale: inv, precision: ctx.precision, fix_stroke: ctx.fix_stroke }) {
            out.push_str(&scaled);
        } else {
            out.push_str(buf);
        }
        buf.clear();
    };

    for c in value.chars() {
        if is_num_char(c) || c.is_ascii_alphabetic() {
            buf.push(c);
        } else {
            flush_buf(&mut out, &mut buf);
            out.push(c);
        }
    }
    flush_buf(&mut out, &mut buf);
    out
}

fn scale_length_value(val: &str, ctx: &ScaleCtx) -> Result<String> {
    let t = val.trim();
    if t.is_empty() {
        return Ok(val.to_string());
    }
    let (num_part, unit) = split_num_and_unit(t);
    if num_part.is_empty() {
        return Ok(val.to_string());
    }
    let unit = unit.trim();
    if unit == "%" {
        return Ok(val.to_string());
    }
    if !is_supported_unit(unit) {
        return Ok(val.to_string());
    }
    let num: f64 = num_part
        .parse()
        .with_context(|| format!("invalid length: {}", val))?;
    let mut out = ctx.fmt(num * ctx.scale);
    if !unit.is_empty() {
        out.push_str(unit);
    }
    Ok(out)
}

fn strip_css_comments(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut i = 0;
    let bytes = input.as_bytes();
    while i < bytes.len() {
        if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'*' {
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            if i + 1 < bytes.len() {
                i += 2;
            }
        } else {
            out.push(bytes[i] as char);
            i += 1;
        }
    }
    out
}

fn is_simple_ident(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    s.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn parse_simple_selector(sel: &str) -> Option<SimpleSelector> {
    if sel.is_empty() {
        return None;
    }
    if sel.contains(['>', '+', '~', '[', ']', ':']) {
        return None;
    }

    let mut element: Option<String> = None;
    let mut id: Option<String> = None;
    let mut classes: Vec<String> = Vec::new();
    let mut i = 0;
    let bytes = sel.as_bytes();

    while i < bytes.len() {
        let c = bytes[i] as char;
        if c == '.' || c == '#' {
            let kind = c;
            i += 1;
            let start = i;
            while i < bytes.len() {
                let ch = bytes[i] as char;
                if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                    i += 1;
                } else {
                    break;
                }
            }
            if start == i {
                return None;
            }
            let ident = &sel[start..i];
            if !is_simple_ident(ident) {
                return None;
            }
            if kind == '.' {
                classes.push(ident.to_string());
            } else {
                if id.is_some() {
                    return None;
                }
                id = Some(ident.to_string());
            }
        } else {
            let start = i;
            while i < bytes.len() {
                let ch = bytes[i] as char;
                if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                    i += 1;
                } else {
                    break;
                }
            }
            if start == i {
                return None;
            }
            let ident = &sel[start..i];
            if !is_simple_ident(ident) {
                return None;
            }
            if element.is_some() {
                return None;
            }
            element = Some(ident.to_string());
        }
    }

    if element.is_none() && id.is_none() && classes.is_empty() {
        return None;
    }

    Some(SimpleSelector { element, id, classes })
}

fn parse_selector(s: &str) -> Option<StyleSelector> {
    let sel = s.trim();
    if sel.is_empty() {
        return None;
    }
    if sel.contains('>') {
        let mut parts: Vec<&str> = sel.split('>').map(|p| p.trim()).collect();
        parts.retain(|p| !p.is_empty());
        if parts.len() != 2 {
            return None;
        }
        let ancestor = parse_simple_selector(parts[0])?;
        let target = parse_simple_selector(parts[1])?;
        return Some(StyleSelector {
            ancestor: Some(ancestor),
            relation: Some(SelectorRelation::Child),
            target,
        });
    }

    let parts: Vec<&str> = sel.split_whitespace().collect();
    if parts.len() > 2 || parts.is_empty() {
        return None;
    }
    let target = parse_simple_selector(parts[parts.len() - 1])?;
    let ancestor = if parts.len() == 2 {
        Some(parse_simple_selector(parts[0])?)
    } else {
        None
    };
    let relation = if ancestor.is_some() {
        Some(SelectorRelation::Descendant)
    } else {
        None
    };
    Some(StyleSelector {
        ancestor,
        relation,
        target,
    })
}

fn selector_specificity_simple(sel: &SimpleSelector) -> u32 {
    let mut score = 0;
    if sel.id.is_some() {
        score += 100;
    }
    if !sel.classes.is_empty() {
        score += 10 * sel.classes.len() as u32;
    }
    if sel.element.is_some() {
        score += 1;
    }
    score
}

fn selector_specificity(sel: &StyleSelector) -> u32 {
    let mut score = selector_specificity_simple(&sel.target);
    if let Some(anc) = &sel.ancestor {
        score += selector_specificity_simple(anc);
    }
    score
}

fn parse_css_rules(input: &str) -> Vec<StyleRule> {
    let cleaned = strip_css_comments(input);
    let mut rules = Vec::new();
    let mut i = 0;
    let mut order: u32 = 0;
    while let Some(open) = cleaned[i..].find('{') {
        let open_idx = i + open;
        let selector_text = cleaned[i..open_idx].trim();
        let rest = &cleaned[open_idx + 1..];
        let Some(close) = rest.find('}') else {
            break;
        };
        let body = rest[..close].trim();
        let props = parse_style(body);
        if !selector_text.is_empty() && !props.is_empty() {
            for sel in selector_text.split(',') {
                if let Some(selector) = parse_selector(sel) {
                    let specificity = selector_specificity(&selector);
                    rules.push(StyleRule {
                        selector,
                        props: props.clone(),
                        specificity,
                        order,
                    });
                }
            }
        }
        i = open_idx + 1 + close + 1;
        order = order.saturating_add(1);
    }
    rules
}

fn collect_style_rules(root: Node) -> Vec<StyleRule> {
    let mut rules = Vec::new();
    for n in root.descendants() {
        if n.is_element() && n.tag_name().name() == "style" {
            let text = n.text().unwrap_or("");
            if !text.trim().is_empty() {
                rules.extend(parse_css_rules(text));
            }
        }
    }
    rules
}

fn serialize_style(props: &[(String, String)]) -> String {
    let mut s = String::new();
    for (i, (k, v)) in props.iter().enumerate() {
        if i > 0 {
            s.push_str("; ");
        }
        s.push_str(k);
        s.push(':');
        s.push_str(v);
    }
    s
}

fn merge_style_props(base: &mut Vec<(String, String)>, other: &[(String, String)]) {
    for (k, v) in other {
        if let Some(pos) = base.iter().position(|(bk, _)| bk == k) {
            base[pos] = (k.clone(), v.clone());
        } else {
            base.push((k.clone(), v.clone()));
        }
    }
}

fn node_class_list<'a>(node: Node<'a, 'a>) -> Vec<&'a str> {
    node.attribute("class")
        .map(|s| s.split_whitespace().collect())
        .unwrap_or_else(Vec::new)
}

fn node_id<'a>(node: Node<'a, 'a>) -> &'a str {
    node.attribute("id").unwrap_or("")
}

fn node_tag<'a>(node: Node<'a, 'a>) -> &'a str {
    node.tag_name().name()
}

fn matches_simple_selector(sel: &SimpleSelector, node: Node) -> bool {
    if let Some(el) = &sel.element {
        if el != node_tag(node) {
            return false;
        }
    }
    if let Some(id) = &sel.id {
        if id != node_id(node) {
            return false;
        }
    }
    if !sel.classes.is_empty() {
        let class_list = node_class_list(node);
        for cls in &sel.classes {
            if !class_list.iter().any(|c| c == cls) {
                return false;
            }
        }
    }
    true
}

fn matches_selector(sel: &StyleSelector, node: Node) -> bool {
    if !matches_simple_selector(&sel.target, node) {
        return false;
    }
    if let Some(anc) = &sel.ancestor {
        match sel.relation {
            Some(SelectorRelation::Child) => {
                if let Some(parent) = node.parent() {
                    return parent.is_element() && matches_simple_selector(anc, parent);
                }
                return false;
            }
            _ => {
                for a in node.ancestors().skip(1) {
                    if a.is_element() && matches_simple_selector(anc, a) {
                        return true;
                    }
                }
                return false;
            }
        }
    }
    true
}

fn collect_matching_style_props(rules: &[StyleRule], node: Node) -> Vec<(String, String)> {
    let mut matched: Vec<&StyleRule> = Vec::new();
    for rule in rules {
        if matches_selector(&rule.selector, node) {
            matched.push(rule);
        }
    }
    matched.sort_by_key(|r| (r.specificity, r.order));
    let mut props = Vec::new();
    for rule in matched {
        merge_style_props(&mut props, &rule.props);
    }
    props
}

fn scale_style_value(
    key: &str,
    val: &str,
    ctx: &ScaleCtx,
    skip_scale: bool,
    has_non_scaling_stroke: bool,
) -> Result<String> {
    match key {
        "transform" => scale_transform_all(val, ctx.scale, ctx.precision)
            .with_context(|| format!("transform scale failed in style: {}", val)),
        "stroke-width" | "width" | "height" | "x" | "y" | "z" | "cx" | "cy" | "r" | "rx"
        | "ry" | "x1" | "y1" | "x2" | "y2" | "font-size" | "letter-spacing"
        | "stroke-dashoffset" | "dx" | "dy" | "markerWidth" | "markerHeight" | "refX"
        | "refY" | "surfaceScale" | "pointsAtX" | "pointsAtY" | "pointsAtZ" => {
            if skip_scale {
                return Ok(val.to_string());
            }
            if key == "stroke-width" && has_non_scaling_stroke && !ctx.fix_stroke {
                return Ok(val.to_string());
            }
            scale_length_value(val, ctx).with_context(|| {
                format!("invalid {} in style: {}", key, val)
            })
        }
        "stroke-dasharray" => {
            if skip_scale {
                return Ok(val.to_string());
            }
            if val.trim().eq_ignore_ascii_case("none") {
                return Ok(val.to_string());
            }
            Ok(scale_number_list(val, ctx))
        }
        "stdDeviation" | "radius" | "kernelUnitLength" => {
            if skip_scale {
                return Ok(val.to_string());
            }
            Ok(scale_number_list(val, ctx))
        }
        "baseFrequency" => {
            if skip_scale {
                return Ok(val.to_string());
            }
            Ok(scale_number_list_inverse(val, ctx))
        }
        _ => Ok(val.to_string()),
    }
}

fn walk_impl(
    node: Node,
    w: &mut XmlWriter,
    ctx: &ScaleCtx,
    ancestor_has_non_translate_transform: bool,
    ancestor_skip_scale: bool,
    style_rules: &[StyleRule],
) -> Result<()> {
    match node.node_type() {
        roxmltree::NodeType::Element => {
            let tag_name = node.tag_name().name();
            let node_id = node.attribute("id").unwrap_or("");
            w.start_element(tag_name);

            let units_attr = if tag_name == "clipPath" {
                node.attribute("clipPathUnits")
            } else if tag_name == "mask" {
                node.attribute("maskUnits")
            } else if tag_name == "linearGradient" || tag_name == "radialGradient" {
                node.attribute("gradientUnits")
            } else if tag_name == "pattern" {
                node.attribute("patternUnits")
            } else if tag_name == "filter" {
                node.attribute("filterUnits")
            } else if tag_name == "marker" {
                node.attribute("markerUnits")
            } else {
                None
            };
            let skip_scale_due_to_units = matches!(units_attr, Some("objectBoundingBox"))
                || (tag_name == "marker"
                    && (matches!(units_attr, Some("strokeWidth")) || units_attr.is_none()));
            let skip_children_due_to_content_units = if tag_name == "pattern" {
                matches!(node.attribute("patternContentUnits"), Some("objectBoundingBox"))
            } else if tag_name == "filter" {
                matches!(node.attribute("primitiveUnits"), Some("objectBoundingBox"))
            } else if tag_name == "marker" {
                matches!(node.attribute("markerUnits"), Some("strokeWidth"))
            } else {
                false
            };

            let mut rule_style_props = collect_matching_style_props(style_rules, node);

            let style_attr = node.attributes().find(|attr| attr.name() == "style");
            let style_value = style_attr.map(|a| a.value()).unwrap_or("");
            let inline_style_props = parse_style(style_value);
            merge_style_props(&mut rule_style_props, &inline_style_props);

            // Check if this element has transform
            let transform_attr = node.attributes().find(|attr| attr.name() == "transform");
            let has_style_transform = rule_style_props.iter().any(|(k, _)| k == "transform");
            let has_transform = transform_attr.is_some() || has_style_transform;
            let transform_value = transform_attr.map(|a| a.value()).unwrap_or("");
            let style_transform_value = rule_style_props
                .iter()
                .find(|(k, _)| k == "transform")
                .map(|(_, v)| v.as_str())
                .unwrap_or("");

            let has_non_scaling_stroke = node
                .attributes()
                .find(|attr| attr.name() == "vector-effect")
                .map(|attr| attr.value() == "non-scaling-stroke")
                .unwrap_or(false)
                || rule_style_props
                    .iter()
                    .any(|(k, v)| k == "vector-effect" && v == "non-scaling-stroke");

            // Check if this element has a non-translate transform
            let has_non_translate_transform = if has_transform {
                let mut any_non_translate = false;
                if !transform_value.is_empty() {
                    any_non_translate =
                        has_non_translate_transform(transform_value).with_context(|| {
                            if node_id.is_empty() {
                                format!("transform parse failed on <{}>", tag_name)
                            } else {
                                format!(
                                    "transform parse failed on <{} id=\"{}\">",
                                    tag_name, node_id
                                )
                            }
                        })?;
                }
                if !style_transform_value.is_empty() {
                    any_non_translate |= has_non_translate_transform(style_transform_value)
                        .with_context(|| {
                            if node_id.is_empty() {
                                format!("transform parse failed in style on <{}>", tag_name)
                            } else {
                                format!(
                                    "transform parse failed in style on <{} id=\"{}\">",
                                    tag_name, node_id
                                )
                            }
                        })?;
                }
                any_non_translate
            } else {
                false
            };

            let skip_scale_self = ancestor_skip_scale || skip_scale_due_to_units;
            let child_skip_scale = if tag_name == "filter" {
                ancestor_skip_scale || skip_children_due_to_content_units
            } else {
                skip_scale_self || skip_children_due_to_content_units
            };

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

                if k == "style" {
                    continue;
                }

                if ctx.fix_stroke && k == "vector-effect" {
                    continue;
                }

                let nv = match k.as_str() {
                    "d" => {
                        // Only skip scaling if there's a non-translate transform in ancestry
                        // (translate doesn't affect path coordinate space)
                        if ancestor_has_non_translate_transform
                            || has_non_translate_transform
                            || skip_scale_self
                        {
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

                    "stroke-width" | "width" | "height" | "x" | "y" | "z" | "cx" | "cy" | "r"
                    | "rx" | "ry" | "x1" | "y1" | "x2" | "y2" | "font-size"
                    | "letter-spacing" | "stroke-dashoffset" | "fx" | "fy" | "dx" | "dy"
                    | "markerWidth" | "markerHeight" | "refX" | "refY" | "surfaceScale"
                    | "pointsAtX" | "pointsAtY" | "pointsAtZ" => {
                        if ancestor_has_non_translate_transform
                            || has_non_translate_transform
                            || skip_scale_self
                        {
                            Ok(v.to_string())
                        } else if k == "stroke-width" && has_non_scaling_stroke && !ctx.fix_stroke {
                            Ok(v.to_string())
                        } else {
                            scale_length_value(v, ctx).with_context(|| {
                                if node_id.is_empty() {
                                    format!("invalid {} on <{}>: {}", k, tag_name, v)
                                } else {
                                    format!(
                                        "invalid {} on <{} id=\"{}\">: {}",
                                        k, tag_name, node_id, v
                                    )
                                }
                            })
                        }
                    }
                    "stroke-dasharray" | "stdDeviation" | "radius" | "scale" | "kernelUnitLength" => {
                        if ancestor_has_non_translate_transform
                            || has_non_translate_transform
                            || skip_scale_self
                        {
                            Ok(v.to_string())
                        } else if v.trim().eq_ignore_ascii_case("none") {
                            Ok(v.to_string())
                        } else {
                            Ok(scale_number_list(v, ctx))
                        }
                    }
                    "baseFrequency" => {
                        if ancestor_has_non_translate_transform
                            || has_non_translate_transform
                            || skip_scale_self
                        {
                            Ok(v.to_string())
                        } else {
                            Ok(scale_number_list_inverse(v, ctx))
                        }
                    }
                    "gradientTransform" | "patternTransform" => {
                        if skip_scale_self {
                            Ok(v.to_string())
                        } else {
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

            if !rule_style_props.is_empty() {
                let mut new_props = Vec::with_capacity(rule_style_props.len());
                for (sk, sv) in rule_style_props {
                    if ctx.fix_stroke && sk == "vector-effect" {
                        continue;
                    }
                    let scaled = scale_style_value(
                        &sk,
                        &sv,
                        ctx,
                        skip_scale_self
                            || ancestor_has_non_translate_transform
                            || has_non_translate_transform,
                        has_non_scaling_stroke,
                    )?;
                    new_props.push((sk, scaled));
                }
                if !new_props.is_empty() {
                    let serialized = serialize_style(&new_props);
                    w.write_attribute("style", &serialized);
                }
            }

            // Pass down whether there's a non-translate transform in the ancestry
            for c in node.children() {
                walk_impl(
                    c,
                    w,
                    ctx,
                    ancestor_has_non_translate_transform || has_non_translate_transform,
                    child_skip_scale,
                    style_rules,
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
    let style_rules = collect_style_rules(node);
    walk_impl(node, w, ctx, false, false, &style_rules)
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
    fn style_attributes_are_scaled() -> Result<()> {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><rect style="x:10; y:20; width:30; height:40; stroke-width:2"/></svg>"#;
        let out = render_scaled_svg(input, 0.5)?;
        assert!(
            out.contains(r#"style="x:5; y:10; width:15; height:20; stroke-width:1""#),
            "expected style numeric values scaled, got: {out}"
        );
        Ok(())
    }

    #[test]
    fn style_non_scaling_stroke_preserves_width() -> Result<()> {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><path d="M10 0 L20 0" style="vector-effect:non-scaling-stroke; stroke-width:2"/></svg>"#;
        let out = render_scaled_svg(input, 0.5)?;
        assert!(
            out.contains(r#"style="vector-effect:non-scaling-stroke; stroke-width:2""#),
            "expected style stroke-width unchanged, got: {out}"
        );
        Ok(())
    }

    #[test]
    fn style_transform_is_scaled_and_prevents_path_double_scale() -> Result<()> {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><path d="M10 0 L20 0" style="transform:scale(2)"/></svg>"#;
        let out = render_scaled_svg(input, 0.5)?;
        assert!(
            out.contains(r#"style="transform:scale(1)""#)
                || out.contains(r#"style="transform:scale(1,1)""#)
                || out.contains(r#"style="transform:matrix("#),
            "expected style transform scaled, got: {out}"
        );
        assert!(
            out.contains(r#"d="M10 0 L20 0""#),
            "expected path not double-scaled under style transform, got: {out}"
        );
        Ok(())
    }

    #[test]
    fn stylesheet_rules_are_applied_and_scaled() -> Result<()> {
        let input = r#"
        <svg xmlns="http://www.w3.org/2000/svg">
            <style>
                rect { width: 30; height: 40; }
                .big { x: 10; y: 20; }
                #solo { stroke-width: 2; }
            </style>
            <rect id="solo" class="big"/>
        </svg>"#;
        let out = render_scaled_svg(input, 0.5)?;
        assert!(out.contains(r#"style="#), "expected style attribute, got: {out}");
        assert!(out.contains(r#"width:15"#), "expected width scaled, got: {out}");
        assert!(out.contains(r#"height:20"#), "expected height scaled, got: {out}");
        assert!(out.contains(r#"x:5"#), "expected x scaled, got: {out}");
        assert!(out.contains(r#"y:10"#), "expected y scaled, got: {out}");
        assert!(
            out.contains(r#"stroke-width:1"#),
            "expected stroke-width scaled, got: {out}"
        );
        Ok(())
    }

    #[test]
    fn stroke_dasharray_and_offset_scale() -> Result<()> {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><path d="M0 0 L10 0" stroke-dasharray="4, 2 1" stroke-dashoffset="3"/></svg>"#;
        let out = render_scaled_svg(input, 0.5)?;
        assert!(
            out.contains(r#"stroke-dasharray="2, 1 0.5""#),
            "expected dasharray scaled, got: {out}"
        );
        assert!(
            out.contains(r#"stroke-dashoffset="1.5""#),
            "expected dashoffset scaled, got: {out}"
        );
        Ok(())
    }

    #[test]
    fn font_size_and_letter_spacing_scale_in_style() -> Result<()> {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><text style="font-size:16; letter-spacing:2">Hi</text></svg>"#;
        let out = render_scaled_svg(input, 0.5)?;
        assert!(
            out.contains(r#"style="font-size:8; letter-spacing:1"#),
            "expected font properties scaled, got: {out}"
        );
        Ok(())
    }

    #[test]
    fn gradient_and_pattern_attributes_scale() -> Result<()> {
        let input = r#"
        <svg xmlns="http://www.w3.org/2000/svg">
            <defs>
                <linearGradient id="g1" x1="0" y1="0" x2="100" y2="200" gradientTransform="translate(10,20) scale(2)"/>
                <radialGradient id="g2" cx="50" cy="60" r="40" fx="10" fy="20"/>
                <pattern id="p1" x="5" y="6" width="70" height="80" patternTransform="translate(4 8)"/>
            </defs>
            <rect width="100" height="100" fill="url(#g1)"/>
        </svg>"#;
        let out = render_scaled_svg(input, 0.5)?;
        assert!(
            out.contains(r#"x1="0""#) && out.contains(r#"y1="0""#),
            "expected gradient x1/y1 scaled, got: {out}"
        );
        assert!(
            out.contains(r#"x2="50""#) && out.contains(r#"y2="100""#),
            "expected gradient x2/y2 scaled, got: {out}"
        );
        assert!(
            out.contains(r#"cx="25""#) && out.contains(r#"cy="30""#) && out.contains(r#"r="20""#),
            "expected radial gradient scaled, got: {out}"
        );
        assert!(
            out.contains(r#"fx="5""#) && out.contains(r#"fy="10""#),
            "expected focal point scaled, got: {out}"
        );
        assert!(
            out.contains(r#"width="35""#) && out.contains(r#"height="40""#),
            "expected pattern size scaled, got: {out}"
        );
        assert!(
            out.contains(r#"patternTransform="matrix("#)
                || out.contains(r#"patternTransform="translate(2,4)""#),
            "expected pattern transform scaled, got: {out}"
        );
        assert!(
            out.contains(r#"gradientTransform="matrix("#)
                || out.contains(r#"gradientTransform="translate(5,10) scale(1)"#),
            "expected gradient transform scaled, got: {out}"
        );
        Ok(())
    }

    #[test]
    fn mask_attributes_scale_in_user_space() -> Result<()> {
        let input = r#"
        <svg xmlns="http://www.w3.org/2000/svg">
            <mask id="m1" maskUnits="userSpaceOnUse" x="10" y="20" width="100" height="120">
                <rect x="10" y="20" width="30" height="40"/>
            </mask>
            <rect width="200" height="200" mask="url(#m1)"/>
        </svg>"#;
        let out = render_scaled_svg(input, 0.5)?;
        assert!(
            out.contains(r#"x="5""#) && out.contains(r#"y="10""#),
            "expected mask x/y scaled, got: {out}"
        );
        assert!(
            out.contains(r#"width="50""#) && out.contains(r#"height="60""#),
            "expected mask width/height scaled, got: {out}"
        );
        Ok(())
    }

    #[test]
    fn clip_path_object_bounding_box_is_not_scaled() -> Result<()> {
        let input = r#"
        <svg xmlns="http://www.w3.org/2000/svg">
            <clipPath id="c1" clipPathUnits="objectBoundingBox">
                <rect x="0.1" y="0.2" width="0.5" height="0.6"/>
            </clipPath>
            <rect width="200" height="200" clip-path="url(#c1)"/>
        </svg>"#;
        let out = render_scaled_svg(input, 0.5)?;
        assert!(
            out.contains(r#"x="0.1""#) && out.contains(r#"y="0.2""#),
            "expected clipPath rect coords unchanged, got: {out}"
        );
        assert!(
            out.contains(r#"width="0.5""#) && out.contains(r#"height="0.6""#),
            "expected clipPath rect size unchanged, got: {out}"
        );
        Ok(())
    }

    #[test]
    fn gradient_percent_values_are_preserved() -> Result<()> {
        let input = r#"
        <svg xmlns="http://www.w3.org/2000/svg">
            <defs>
                <linearGradient id="g1" x1="0%" y1="0%" x2="100%" y2="100%"/>
                <radialGradient id="g2" cx="50%" cy="60%" r="40%"/>
            </defs>
            <rect width="100" height="100" fill="url(#g1)"/>
        </svg>"#;
        let out = render_scaled_svg(input, 0.5)?;
        assert!(
            out.contains(r#"x1="0%""#) && out.contains(r#"y1="0%""#),
            "expected linear gradient percents preserved, got: {out}"
        );
        assert!(
            out.contains(r#"x2="100%""#) && out.contains(r#"y2="100%""#),
            "expected linear gradient percents preserved, got: {out}"
        );
        assert!(
            out.contains(r#"cx="50%""#) && out.contains(r#"cy="60%""#) && out.contains(r#"r="40%""#),
            "expected radial gradient percents preserved, got: {out}"
        );
        Ok(())
    }

    #[test]
    fn gradient_object_bounding_box_is_not_scaled() -> Result<()> {
        let input = r#"
        <svg xmlns="http://www.w3.org/2000/svg">
            <defs>
                <linearGradient id="g1" gradientUnits="objectBoundingBox" x1="0.1" y1="0.2" x2="0.9" y2="1"/>
                <radialGradient id="g2" gradientUnits="objectBoundingBox" cx="0.5" cy="0.6" r="0.4" fx="0.2" fy="0.3"/>
            </defs>
            <rect width="100" height="100" fill="url(#g1)"/>
        </svg>"#;
        let out = render_scaled_svg(input, 0.5)?;
        assert!(
            out.contains(r#"x1="0.1""#) && out.contains(r#"y1="0.2""#),
            "expected linear gradient values unchanged, got: {out}"
        );
        assert!(
            out.contains(r#"x2="0.9""#) && out.contains(r#"y2="1""#),
            "expected linear gradient values unchanged, got: {out}"
        );
        assert!(
            out.contains(r#"cx="0.5""#) && out.contains(r#"cy="0.6""#) && out.contains(r#"r="0.4""#),
            "expected radial gradient values unchanged, got: {out}"
        );
        assert!(
            out.contains(r#"fx="0.2""#) && out.contains(r#"fy="0.3""#),
            "expected focal point unchanged, got: {out}"
        );
        Ok(())
    }

    #[test]
    fn pattern_content_units_object_bounding_box_skips_child_scaling() -> Result<()> {
        let input = r#"
        <svg xmlns="http://www.w3.org/2000/svg">
            <defs>
                <pattern id="p1" patternUnits="userSpaceOnUse" patternContentUnits="objectBoundingBox" x="10" y="20" width="40" height="50">
                    <rect x="0.1" y="0.2" width="0.5" height="0.6"/>
                </pattern>
            </defs>
            <rect width="100" height="100" fill="url(#p1)"/>
        </svg>"#;
        let out = render_scaled_svg(input, 0.5)?;
        assert!(
            out.contains(r#"x="5""#) && out.contains(r#"y="10""#),
            "expected pattern x/y scaled, got: {out}"
        );
        assert!(
            out.contains(r#"width="20""#) && out.contains(r#"height="25""#),
            "expected pattern size scaled, got: {out}"
        );
        assert!(
            out.contains(r#"x="0.1""#) && out.contains(r#"y="0.2""#),
            "expected child rect coords unchanged, got: {out}"
        );
        assert!(
            out.contains(r#"width="0.5""#) && out.contains(r#"height="0.6""#),
            "expected child rect size unchanged, got: {out}"
        );
        Ok(())
    }

    #[test]
    fn length_units_are_scaled_and_preserved() -> Result<()> {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><rect width="10mm" height="8pt" x="1cm" y="2in"/></svg>"#;
        let out = render_scaled_svg(input, 0.5)?;
        assert!(
            out.contains(r#"width="5mm""#) && out.contains(r#"height="4pt""#),
            "expected mm/pt scaled, got: {out}"
        );
        assert!(
            out.contains(r#"x="0.5cm""#) && out.contains(r#"y="1in""#),
            "expected cm/in scaled, got: {out}"
        );
        Ok(())
    }

    #[test]
    fn dasharray_units_are_scaled_and_preserved() -> Result<()> {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><path d="M0 0 L10 0" stroke-dasharray="2pt 4pt,1mm"/></svg>"#;
        let out = render_scaled_svg(input, 0.5)?;
        assert!(
            out.contains(r#"stroke-dasharray="1pt 2pt,0.5mm""#),
            "expected dasharray units scaled, got: {out}"
        );
        Ok(())
    }

    #[test]
    fn filter_object_bounding_box_is_not_scaled() -> Result<()> {
        let input = r#"
        <svg xmlns="http://www.w3.org/2000/svg">
            <defs>
                <filter id="f1" filterUnits="objectBoundingBox" x="0.1" y="0.2" width="0.5" height="0.6">
                    <feOffset dx="10" dy="20"/>
                </filter>
            </defs>
            <rect width="100" height="100" filter="url(#f1)"/>
        </svg>"#;
        let out = render_scaled_svg(input, 0.5)?;
        assert!(
            out.contains(r#"x="0.1""#) && out.contains(r#"y="0.2""#),
            "expected filter region unchanged, got: {out}"
        );
        assert!(
            out.contains(r#"width="0.5""#) && out.contains(r#"height="0.6""#),
            "expected filter region unchanged, got: {out}"
        );
        assert!(
            out.contains(r#"dx="5""#) && out.contains(r#"dy="10""#),
            "expected feOffset scaled in user space, got: {out}"
        );
        Ok(())
    }

    #[test]
    fn filter_primitives_scale_in_user_space() -> Result<()> {
        let input = r#"
        <svg xmlns="http://www.w3.org/2000/svg">
            <defs>
                <filter id="f2" x="10" y="20" width="100" height="120">
                    <feGaussianBlur stdDeviation="4 2"/>
                    <feOffset dx="10" dy="20"/>
                    <feMorphology radius="6"/>
                </filter>
            </defs>
            <rect width="100" height="100" filter="url(#f2)"/>
        </svg>"#;
        let out = render_scaled_svg(input, 0.5)?;
        assert!(
            out.contains(r#"x="5""#) && out.contains(r#"y="10""#),
            "expected filter region scaled, got: {out}"
        );
        assert!(
            out.contains(r#"width="50""#) && out.contains(r#"height="60""#),
            "expected filter size scaled, got: {out}"
        );
        assert!(
            out.contains(r#"stdDeviation="2 1""#),
            "expected stdDeviation scaled, got: {out}"
        );
        assert!(
            out.contains(r#"dx="5""#) && out.contains(r#"dy="10""#),
            "expected feOffset scaled, got: {out}"
        );
        assert!(
            out.contains(r#"radius="3""#),
            "expected feMorphology radius scaled, got: {out}"
        );
        Ok(())
    }

    #[test]
    fn marker_units_stroke_width_skips_scaling() -> Result<()> {
        let input = r#"
        <svg xmlns="http://www.w3.org/2000/svg">
            <defs>
                <marker id="m1" markerUnits="strokeWidth" markerWidth="10" markerHeight="8" refX="2" refY="3">
                    <rect x="1" y="2" width="3" height="4"/>
                </marker>
            </defs>
            <path d="M0 0 L10 0" marker-end="url(#m1)"/>
        </svg>"#;
        let out = render_scaled_svg(input, 0.5)?;
        assert!(
            out.contains(r#"markerWidth="10""#)
                && out.contains(r#"markerHeight="8""#)
                && out.contains(r#"refX="2""#)
                && out.contains(r#"refY="3""#),
            "expected marker attributes unchanged, got: {out}"
        );
        assert!(
            out.contains(r#"x="1""#) && out.contains(r#"y="2""#),
            "expected marker child coords unchanged, got: {out}"
        );
        assert!(
            out.contains(r#"width="3""#) && out.contains(r#"height="4""#),
            "expected marker child size unchanged, got: {out}"
        );
        Ok(())
    }

    #[test]
    fn marker_user_space_scales() -> Result<()> {
        let input = r#"
        <svg xmlns="http://www.w3.org/2000/svg">
            <defs>
                <marker id="m2" markerUnits="userSpaceOnUse" markerWidth="10" markerHeight="8" refX="2" refY="3">
                    <rect x="4" y="6" width="10" height="12"/>
                </marker>
            </defs>
            <path d="M0 0 L10 0" marker-end="url(#m2)"/>
        </svg>"#;
        let out = render_scaled_svg(input, 0.5)?;
        assert!(
            out.contains(r#"markerWidth="5""#)
                && out.contains(r#"markerHeight="4""#)
                && out.contains(r#"refX="1""#)
                && out.contains(r#"refY="1.5""#),
            "expected marker attributes scaled, got: {out}"
        );
        assert!(
            out.contains(r#"x="2""#) && out.contains(r#"y="3""#),
            "expected marker child coords scaled, got: {out}"
        );
        assert!(
            out.contains(r#"width="5""#) && out.contains(r#"height="6""#),
            "expected marker child size scaled, got: {out}"
        );
        Ok(())
    }

    #[test]
    fn filter_drop_shadow_and_displacement_scale() -> Result<()> {
        let input = r#"
        <svg xmlns="http://www.w3.org/2000/svg">
            <defs>
                <filter id="f3" x="10" y="20" width="100" height="120">
                    <feDropShadow dx="4" dy="6" stdDeviation="5"/>
                    <feDisplacementMap scale="8"/>
                </filter>
            </defs>
            <rect width="100" height="100" filter="url(#f3)"/>
        </svg>"#;
        let out = render_scaled_svg(input, 0.5)?;
        assert!(
            out.contains(r#"dx="2""#) && out.contains(r#"dy="3""#),
            "expected drop shadow offset scaled, got: {out}"
        );
        assert!(
            out.contains(r#"stdDeviation="2.5""#),
            "expected drop shadow blur scaled, got: {out}"
        );
        assert!(
            out.contains(r#"scale="4""#),
            "expected displacement scale scaled, got: {out}"
        );
        Ok(())
    }

    #[test]
    fn filter_kernel_unit_length_and_surface_scale() -> Result<()> {
        let input = r#"
        <svg xmlns="http://www.w3.org/2000/svg">
            <defs>
                <filter id="f4" x="10" y="20" width="100" height="120">
                    <feDiffuseLighting surfaceScale="5" kernelUnitLength="2 4"/>
                </filter>
            </defs>
            <rect width="100" height="100" filter="url(#f4)"/>
        </svg>"#;
        let out = render_scaled_svg(input, 0.5)?;
        assert!(
            out.contains(r#"surfaceScale="2.5""#),
            "expected surfaceScale scaled, got: {out}"
        );
        assert!(
            out.contains(r#"kernelUnitLength="1 2""#),
            "expected kernelUnitLength scaled, got: {out}"
        );
        Ok(())
    }

    #[test]
    fn stylesheet_specificity_overrides() -> Result<()> {
        let input = r#"
        <svg xmlns="http://www.w3.org/2000/svg">
            <style>
                rect { width: 20; }
                .big { width: 30; }
                #solo { width: 40; }
                rect.big { height: 20; }
                rect#solo { height: 40; }
            </style>
            <rect id="solo" class="big"/>
        </svg>"#;
        let out = render_scaled_svg(input, 0.5)?;
        assert!(
            out.contains(r#"width:20"#),
            "expected id width scaled to 20, got: {out}"
        );
        assert!(
            out.contains(r#"height:20"#),
            "expected id+element height scaled to 20, got: {out}"
        );
        Ok(())
    }

    #[test]
    fn stylesheet_descendant_selector_applies() -> Result<()> {
        let input = r#"
        <svg xmlns="http://www.w3.org/2000/svg">
            <style>
                g .inner { width: 30; }
            </style>
            <g>
                <rect class="inner"/>
            </g>
        </svg>"#;
        let out = render_scaled_svg(input, 0.5)?;
        assert!(
            out.contains(r#"width:15"#),
            "expected descendant rule to apply, got: {out}"
        );
        Ok(())
    }

    #[test]
    fn stylesheet_child_selector_applies() -> Result<()> {
        let input = r#"
        <svg xmlns="http://www.w3.org/2000/svg">
            <style>
                g > .inner { width: 30; }
            </style>
            <g>
                <rect class="inner"/>
            </g>
        </svg>"#;
        let out = render_scaled_svg(input, 0.5)?;
        assert!(
            out.contains(r#"width:15"#),
            "expected child rule to apply, got: {out}"
        );
        Ok(())
    }

    #[test]
    fn filter_light_positions_scale() -> Result<()> {
        let input = r#"
        <svg xmlns="http://www.w3.org/2000/svg">
            <defs>
                <filter id="f5" x="10" y="20" width="100" height="120">
                    <fePointLight x="10" y="20" z="30"/>
                    <feSpotLight x="5" y="6" z="7" pointsAtX="8" pointsAtY="9" pointsAtZ="10"/>
                </filter>
            </defs>
            <rect width="100" height="100" filter="url(#f5)"/>
        </svg>"#;
        let out = render_scaled_svg(input, 0.5)?;
        assert!(
            out.contains(r#"x="5""#) && out.contains(r#"y="10""#) && out.contains(r#"z="15""#),
            "expected point light scaled, got: {out}"
        );
        assert!(
            out.contains(r#"pointsAtX="4""#)
                && out.contains(r#"pointsAtY="4.5""#)
                && out.contains(r#"pointsAtZ="5""#),
            "expected spot light points scaled, got: {out}"
        );
        Ok(())
    }

    #[test]
    fn marker_orient_is_preserved() -> Result<()> {
        let input = r#"
        <svg xmlns="http://www.w3.org/2000/svg">
            <defs>
                <marker id="m4" orient="auto" markerWidth="10" markerHeight="8" refX="2" refY="3">
                    <rect x="1" y="2" width="3" height="4"/>
                </marker>
            </defs>
            <path d="M0 0 L10 0" marker-end="url(#m4)"/>
        </svg>"#;
        let out = render_scaled_svg(input, 0.5)?;
        assert!(
            out.contains(r#"orient="auto""#),
            "expected orient preserved, got: {out}"
        );
        Ok(())
    }

    #[test]
    fn marker_orient_angle_is_preserved() -> Result<()> {
        let input = r#"
        <svg xmlns="http://www.w3.org/2000/svg">
            <defs>
                <marker id="m5" orient="45" markerWidth="10" markerHeight="8" refX="2" refY="3">
                    <rect x="1" y="2" width="3" height="4"/>
                </marker>
            </defs>
            <path d="M0 0 L10 0" marker-end="url(#m5)"/>
        </svg>"#;
        let out = render_scaled_svg(input, 0.5)?;
        assert!(
            out.contains(r#"orient="45""#),
            "expected orient angle preserved, got: {out}"
        );
        Ok(())
    }

    #[test]
    fn turbulence_base_frequency_scales_inverse() -> Result<()> {
        let input = r#"
        <svg xmlns="http://www.w3.org/2000/svg">
            <defs>
                <filter id="f6">
                    <feTurbulence baseFrequency="0.05 0.1"/>
                </filter>
            </defs>
            <rect width="100" height="100" filter="url(#f6)"/>
        </svg>"#;
        let out = render_scaled_svg(input, 0.5)?;
        assert!(
            out.contains(r#"baseFrequency="0.1 0.2""#),
            "expected baseFrequency inverse scaled, got: {out}"
        );
        Ok(())
    }

    #[test]
    fn marker_default_units_stroke_width_skips_scaling() -> Result<()> {
        let input = r#"
        <svg xmlns="http://www.w3.org/2000/svg">
            <defs>
                <marker id="m3" markerWidth="10" markerHeight="8" refX="2" refY="3">
                    <rect x="1" y="2" width="3" height="4"/>
                </marker>
            </defs>
            <path d="M0 0 L10 0" marker-end="url(#m3)"/>
        </svg>"#;
        let out = render_scaled_svg(input, 0.5)?;
        assert!(
            out.contains(r#"markerWidth="10""#)
                && out.contains(r#"markerHeight="8""#)
                && out.contains(r#"refX="2""#)
                && out.contains(r#"refY="3""#),
            "expected default marker units to skip scaling, got: {out}"
        );
        assert!(
            out.contains(r#"x="1""#) && out.contains(r#"y="2""#),
            "expected marker child coords unchanged, got: {out}"
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
