use anyhow::{Context, Result};
use crate::scale::ScaleCtx;
use regex::Regex;

pub fn scale_path(d: &str, ctx: &ScaleCtx) -> Result<String> {
    let re = Regex::new(r"(?i)-?\d*\.?\d+(?:e[+-]?\d+)?")
        .context("invalid number regex")?;
    let mut err: Option<anyhow::Error> = None;
    let out = re
        .replace_all(d, |caps: &regex::Captures| {
            if err.is_some() {
                return caps.get(0).map(|m| m.as_str()).unwrap_or("").to_string();
            }
            let m = match caps.get(0) {
                Some(m) => m,
                None => {
                    err = Some(anyhow::anyhow!("missing regex match in path data"));
                    return String::new();
                }
            };
            match m.as_str().parse::<f64>() {
                Ok(v) => ctx.fmt(v * ctx.scale),
                Err(e) => {
                    err = Some(anyhow::Error::new(e).context("failed to parse path number"));
                    String::new()
                }
            }
        })
        .to_string();
    if let Some(e) = err {
        return Err(e);
    }
    Ok(out)
}
