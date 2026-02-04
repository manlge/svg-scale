use crate::scale::ScaleCtx;
use anyhow::Result;
use nom::{
    branch::alt, bytes::complete::take_while1, character::complete::one_of, combinator::recognize,
    multi::many0, number::complete::double, IResult,
};
pub fn scale_path(d: &str, ctx: &ScaleCtx) -> Result<String> {
    let (rest, parts) = match parse_parts(d) {
        Ok(v) => v,
        Err(nom::Err::Error(e)) | Err(nom::Err::Failure(e)) => {
            let pos = d.len().saturating_sub(e.input.len());
            return Err(anyhow::anyhow!(format_path_error(d, pos)));
        }
        Err(_) => return Err(anyhow::anyhow!("invalid path data")),
    };
    if !rest.is_empty() {
        let pos = d.len().saturating_sub(rest.len());
        return Err(anyhow::anyhow!(format_path_error(d, pos)));
    }

    let mut cmd: Option<char> = None;
    let mut param_index: usize = 0;
    let mut out = String::with_capacity(d.len());

    for part in parts {
        match part {
            Part::Sep(s) => out.push_str(s),
            Part::Cmd(c) => {
                cmd = Some(c);
                param_index = 0;
                out.push(c);
            }
            Part::Num { raw, val } => {
                let should_scale = match cmd {
                    Some('A') | Some('a') => {
                        let idx = param_index % 7;
                        matches!(idx, 0 | 1 | 5 | 6)
                    }
                    _ => true,
                };
                if should_scale {
                    out.push_str(&ctx.fmt(val * ctx.scale));
                } else {
                    out.push_str(raw);
                }
                param_index = param_index.saturating_add(1);
            }
        }
    }

    Ok(out)
}

fn format_path_error(input: &str, pos: usize) -> String {
    let start = pos.saturating_sub(10);
    let end = (pos + 10).min(input.len());
    let snippet = &input[start..end];
    let char_index = input[..pos].chars().count();
    let reason = classify_path_error(input, pos);
    format!(
        "invalid path data at char {} (byte {}) near '{}': {}",
        char_index, pos, snippet, reason
    )
}

fn classify_path_error(input: &str, pos: usize) -> &'static str {
    if pos >= input.len() {
        return "unexpected end of input";
    }
    let rest = &input[pos..];
    let mut chars = rest.chars();
    let c = match chars.next() {
        Some(c) => c,
        None => return "unexpected end of input",
    };
    let is_cmd = matches!(
        c,
        'M' | 'm'
            | 'Z'
            | 'z'
            | 'L'
            | 'l'
            | 'H'
            | 'h'
            | 'V'
            | 'v'
            | 'C'
            | 'c'
            | 'S'
            | 's'
            | 'Q'
            | 'q'
            | 'T'
            | 't'
            | 'A'
            | 'a'
    );
    if c.is_ascii_alphabetic() && !is_cmd {
        return "invalid command";
    }
    if c.is_ascii_digit() || c == '-' || c == '+' || c == '.' || c == 'e' || c == 'E' {
        return "invalid number";
    }
    if c.is_whitespace() || c == ',' {
        return "invalid separator";
    }
    "unexpected token"
}

#[derive(Debug)]
enum Part<'a> {
    Sep(&'a str),
    Cmd(char),
    Num { raw: &'a str, val: f64 },
}

fn is_sep_char(c: char) -> bool {
    !c.is_ascii_alphabetic() && c != '-' && c != '+' && c != '.' && !c.is_ascii_digit()
}

fn parse_sep(input: &str) -> IResult<&str, Part<'_>> {
    let (rest, s) = take_while1(is_sep_char)(input)?;
    Ok((rest, Part::Sep(s)))
}

fn parse_cmd(input: &str) -> IResult<&str, Part<'_>> {
    let (rest, c) = one_of("MmZzLlHhVvCcSsQqTtAa")(input)?;
    Ok((rest, Part::Cmd(c)))
}

fn parse_num(input: &str) -> IResult<&str, Part<'_>> {
    let (rest, raw) = recognize(double)(input)?;
    let val: f64 = raw.parse().unwrap_or(0.0);
    Ok((rest, Part::Num { raw, val }))
}

fn parse_parts(input: &str) -> IResult<&str, Vec<Part<'_>>> {
    many0(alt((parse_cmd, parse_num, parse_sep)))(input)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arc_flags_should_not_be_scaled() -> Result<()> {
        let ctx = ScaleCtx {
            scale: 2.0,
            precision: 4,
            fix_stroke: false,
        };
        let input = "M10 10 A 5 5 0 0 1 20 20";
        let out = scale_path(input, &ctx)?;
        assert_eq!(out, "M20 20 A 10 10 0 0 1 40 40");
        Ok(())
    }

    #[test]
    fn large_path_scales_without_panic() -> Result<()> {
        let ctx = ScaleCtx {
            scale: 1.25,
            precision: 4,
            fix_stroke: false,
        };
        let mut d = String::from("M0 0");
        for i in 1..1000 {
            d.push_str(&format!(" L{} {}", i, i + 1));
        }
        let out = scale_path(&d, &ctx)?;
        assert!(out.starts_with("M0 0 L1.25 2.5"));
        Ok(())
    }

    #[test]
    fn path_numbers_with_scientific_notation_and_signs() -> Result<()> {
        let ctx = ScaleCtx {
            scale: 2.0,
            precision: 6,
            fix_stroke: false,
        };
        let input = "M-0.5e-2 1E2 L+.25 -3.5e1";
        let out = scale_path(input, &ctx)?;
        assert_eq!(out, "M-0.01 200 L0.5 -70");
        Ok(())
    }

    #[test]
    fn path_numbers_with_tight_packing() -> Result<()> {
        let ctx = ScaleCtx {
            scale: 2.0,
            precision: 4,
            fix_stroke: false,
        };
        let input = "M10-20L.5-.25";
        let out = scale_path(input, &ctx)?;
        assert_eq!(out, "M20-40L1-0.5");
        Ok(())
    }

    #[test]
    fn arc_flags_remain_unscaled_in_mixed_numbers() -> Result<()> {
        let ctx = ScaleCtx {
            scale: 3.0,
            precision: 4,
            fix_stroke: false,
        };
        let input = "M0 0 A1.5e1 2.5 0 1 0 10 -20";
        let out = scale_path(input, &ctx)?;
        assert_eq!(out, "M0 0 A45 7.5 0 1 0 30 -60");
        Ok(())
    }

    #[test]
    fn path_invalid_trailing_garbage_fails() {
        let ctx = ScaleCtx {
            scale: 1.0,
            precision: 4,
            fix_stroke: false,
        };
        let err = scale_path("M10e", &ctx).unwrap_err();
        assert!(err.to_string().contains("invalid path data at char"));
        assert!(err.to_string().contains("invalid number"));
    }

    #[test]
    fn path_invalid_command_fails() {
        let ctx = ScaleCtx {
            scale: 1.0,
            precision: 4,
            fix_stroke: false,
        };
        let err = scale_path("X10 20", &ctx).unwrap_err();
        assert!(err.to_string().contains("invalid path data at char"));
        assert!(err.to_string().contains("invalid command"));
    }
}
