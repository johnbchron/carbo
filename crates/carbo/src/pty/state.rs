#[derive(Debug, Default, Clone)]
pub struct PtyState {
  /// placeholder
  output: String,
}

impl PtyState {
  pub fn push_char(&mut self, c: char) { self.output.push(c); }
}
