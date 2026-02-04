use anyhow::{Context, Result};
use nom::{
    branch::alt,
    bytes::complete::take_while1,
    character::complete::{char, space0, space1},
    combinator::{all_consuming, map},
    multi::{many0, separated_list0},
    number::complete::double,
    sequence::{delimited, preceded, terminated, tuple},
    IResult,
};

#[derive(Debug, Clone)]
pub struct Transform {
    pub name: String,
    pub params: Vec<f64>,
}

fn is_name_char(c: char) -> bool {
    c.is_ascii_alphabetic()
}

fn sep(input: &str) -> IResult<&str, ()> {
    let comma = map(tuple((space0, char(','), space0)), |_| ());
    let spaces = map(space1, |_| ());
    alt((comma, spaces))(input)
}

fn params_list(input: &str) -> IResult<&str, Vec<f64>> {
    separated_list0(sep, double)(input)
}

fn transform_fn(input: &str) -> IResult<&str, Transform> {
    let (input, name) = map(take_while1(is_name_char), |s: &str| s.to_string())(input)?;
    let (input, params) = delimited(
        tuple((space0, char('('), space0)),
        params_list,
        tuple((space0, char(')'))),
    )(input)?;
    Ok((input, Transform { name, params }))
}

fn transform_list(input: &str) -> IResult<&str, Vec<Transform>> {
    many0(preceded(space0, transform_fn))(input)
}

pub fn parse_transform_list(input: &str) -> Result<Vec<Transform>> {
    match all_consuming(terminated(preceded(space0, transform_list), space0))(input) {
        Ok((_, list)) => Ok(list),
        Err(_) => Err(anyhow::anyhow!("invalid transform: {}", input)),
    }
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

pub fn transform_to_matrix(list: &[Transform]) -> Result<[f64; 6]> {
    let mut m = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
    for t in list {
        let mi = match t.name.as_str() {
            "translate" => {
                let tx = t.params.get(0).copied().unwrap_or(0.0);
                let ty = t.params.get(1).copied().unwrap_or(0.0);
                [1.0, 0.0, 0.0, 1.0, tx, ty]
            }
            "scale" => {
                let sx = t.params.get(0).copied().unwrap_or(1.0);
                let sy = t.params.get(1).copied().unwrap_or(sx);
                [sx, 0.0, 0.0, sy, 0.0, 0.0]
            }
            "rotate" => {
                let angle = t.params.get(0).copied().unwrap_or(0.0);
                let rad = angle.to_radians();
                let cos = rad.cos();
                let sin = rad.sin();
                if t.params.len() >= 3 {
                    let cx = t.params[1];
                    let cy = t.params[2];
                    let t1 = [1.0, 0.0, 0.0, 1.0, cx, cy];
                    let r = [cos, sin, -sin, cos, 0.0, 0.0];
                    let t2 = [1.0, 0.0, 0.0, 1.0, -cx, -cy];
                    mat_mul(t1, mat_mul(r, t2))
                } else {
                    [cos, sin, -sin, cos, 0.0, 0.0]
                }
            }
            "skewX" => {
                let angle = t.params.get(0).copied().unwrap_or(0.0);
                [1.0, 0.0, angle.to_radians().tan(), 1.0, 0.0, 0.0]
            }
            "skewY" => {
                let angle = t.params.get(0).copied().unwrap_or(0.0);
                [1.0, angle.to_radians().tan(), 0.0, 1.0, 0.0, 0.0]
            }
            "matrix" => {
                if t.params.len() < 6 {
                    return Err(anyhow::anyhow!(
                        "matrix() requires 6 parameters, got {}",
                        t.params.len()
                    ));
                }
                [
                    t.params[0],
                    t.params[1],
                    t.params[2],
                    t.params[3],
                    t.params[4],
                    t.params[5],
                ]
            }
            _ => return Err(anyhow::anyhow!("unsupported transform: {}", t.name)),
        };
        m = mat_mul(m, mi);
    }
    Ok(m)
}

fn clean_matrix_value(v: f64) -> f64 {
    if v.abs() < 1e-12 {
        0.0
    } else {
        v
    }
}

fn fmt_num(v: f64, precision: usize) -> String {
    let s = format!("{:.*}", precision, v);
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

pub fn scale_transform_value(input: &str, scale: f64, precision: usize) -> Result<String> {
    let list = parse_transform_list(input).context("parse transform")?;
    if list.is_empty() {
        return Ok(input.to_string());
    }

    let has_non_translate = list.iter().any(|t| t.name != "translate");
    if has_non_translate {
        if list.len() == 1 {
            if list[0].name == "scale" {
                let sx = list[0].params.get(0).copied().unwrap_or(1.0);
                let sy = list[0].params.get(1).copied().unwrap_or(sx);
                if list[0].params.len() >= 2 {
                    return Ok(format!(
                        "scale({},{})",
                        fmt_num(sx * scale, precision),
                        fmt_num(sy * scale, precision)
                    ));
                }
                return Ok(format!("scale({})", fmt_num(sx * scale, precision)));
            }
        }

        let m = transform_to_matrix(&list)?;
        return Ok(format!(
            "matrix({},{},{},{},{},{})",
            fmt_num(clean_matrix_value(m[0] * scale), precision),
            fmt_num(clean_matrix_value(m[1] * scale), precision),
            fmt_num(clean_matrix_value(m[2] * scale), precision),
            fmt_num(clean_matrix_value(m[3] * scale), precision),
            fmt_num(clean_matrix_value(m[4] * scale), precision),
            fmt_num(clean_matrix_value(m[5] * scale), precision)
        ));
    }

    let mut parts = Vec::new();
    for t in &list {
        if t.name != "translate" {
            continue;
        }
        let tx = t.params.get(0).copied().unwrap_or(0.0);
        let ty = t.params.get(1).copied().unwrap_or(0.0);
        if t.params.len() >= 2 {
            parts.push(format!(
                "translate({},{})",
                fmt_num(tx * scale, precision),
                fmt_num(ty * scale, precision)
            ));
        } else {
            parts.push(format!("translate({})", fmt_num(tx * scale, precision)));
        }
    }
    Ok(parts.join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_transform_examples() {
        let s = "translate(10,20) rotate(30 5 6) scale(2)";
        let list = parse_transform_list(s).unwrap();
        assert_eq!(list.len(), 3);
    }
}
