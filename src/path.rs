use anyhow::{Context, Result};
use crate::scale::ScaleCtx;
pub fn scale_path(d: &str, ctx: &ScaleCtx) -> Result<String> {
    let bytes = d.as_bytes();
    let mut i = 0usize;
    let mut cmd: Option<char> = None;
    let mut param_index: usize = 0;
    let mut replacements: Vec<(usize, usize, String)> = Vec::new();

    while i < bytes.len() {
        let c = bytes[i] as char;
        if c.is_ascii_alphabetic() {
            if matches!(
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
            ) {
                cmd = Some(c);
                param_index = 0;
            }
            i += 1;
            continue;
        }

        if is_number_start(c) {
            let start = i;
            let end = parse_number(bytes, i);
            let s = &d[start..end];
            let v: f64 = s
                .parse()
                .with_context(|| "failed to parse path number")?;

            let should_scale = match cmd {
                Some('A') | Some('a') => {
                    let idx = param_index % 7;
                    matches!(idx, 0 | 1 | 5 | 6)
                }
                _ => true,
            };

            let out = if should_scale {
                ctx.fmt(v * ctx.scale)
            } else {
                s.to_string()
            };

            replacements.push((start, end, out));
            param_index = param_index.saturating_add(1);
            i = end;
            continue;
        }

        i += 1;
    }

    if replacements.is_empty() {
        return Ok(d.to_string());
    }

    let mut out = String::with_capacity(d.len());
    let mut last = 0usize;
    for (start, end, rep) in replacements {
        out.push_str(&d[last..start]);
        out.push_str(&rep);
        last = end;
    }
    out.push_str(&d[last..]);
    Ok(out)
}

fn is_number_start(c: char) -> bool {
    c == '-' || c == '+' || c == '.' || c.is_ascii_digit()
}

fn parse_number(bytes: &[u8], start: usize) -> usize {
    let mut i = start;
    let mut has_dot = false;
    let mut has_exp = false;

    if i < bytes.len() {
        let c = bytes[i] as char;
        if c == '+' || c == '-' {
            i += 1;
        }
    }

    while i < bytes.len() {
        let c = bytes[i] as char;
        if c.is_ascii_digit() {
            i += 1;
            continue;
        }
        if c == '.' && !has_dot && !has_exp {
            has_dot = true;
            i += 1;
            continue;
        }
        if (c == 'e' || c == 'E') && !has_exp {
            has_exp = true;
            i += 1;
            if i < bytes.len() {
                let sign = bytes[i] as char;
                if sign == '+' || sign == '-' {
                    i += 1;
                }
            }
            continue;
        }
        break;
    }

    i
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
}
