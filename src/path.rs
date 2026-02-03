use regex::Regex;
use crate::scale::ScaleCtx;

pub fn scale_path(d: &str, ctx: &ScaleCtx) -> String {
    let re = Regex::new(r"-?\d*\.?\d+").unwrap();
    re.replace_all(d, |caps: &regex::Captures| {
        let m = caps.get(0).unwrap();
        let v: f64 = m.as_str().parse().unwrap();
        ctx.fmt(v * ctx.scale)
    }).to_string()
}
