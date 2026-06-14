use std::{
  fmt::Debug,
  num::{NonZeroU16, NonZeroUsize},
};

use vt100::{Parser, Screen};

pub struct PtyState {
  parser: Parser,
}

impl PtyState {
  pub fn new(
    rows: NonZeroU16,
    cols: NonZeroU16,
    scrollback: NonZeroUsize,
  ) -> Self {
    Self {
      parser: Parser::new(rows.get(), cols.get(), scrollback.get()),
    }
  }

  pub fn process(&mut self, input: &[u8]) { self.parser.process(input); }

  pub fn view(&self) -> PtyStateView {
    PtyStateView {
      screen: self.parser.screen().clone(),
    }
  }
}

#[derive(Clone)]
pub struct PtyStateView {
  screen: Screen,
}

impl Debug for PtyStateView {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.debug_struct("PtyStateView").finish()
  }
}

impl PtyStateView {
  pub fn screen(&self) -> &Screen { &self.screen }
}
