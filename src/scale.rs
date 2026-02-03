pub struct ScaleCtx {
    pub scale: f64,
    pub precision: usize,
    pub fix_stroke: bool,
}

impl ScaleCtx {
    pub fn fmt(&self, v: f64) -> String {
        let s = format!("{:.*}", self.precision, v);
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}
