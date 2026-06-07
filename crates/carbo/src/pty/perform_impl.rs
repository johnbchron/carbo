use vte::Perform;

use super::PtyState;

impl Perform for PtyState {
  fn print(&mut self, c: char) { self.push_char(c); }
}
