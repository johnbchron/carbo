use std::{
  fmt::Debug,
  num::{NonZeroU16, NonZeroUsize},
};

use tracing::instrument;
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

  #[instrument("process_pty_input", skip_all)]
  pub fn process_input(&mut self, input: &[u8]) { self.parser.process(input); }

  #[instrument("pty_state_snapshot", skip_all)]
  pub fn snapshot(&self) -> PtyStateView {
    PtyStateView {
      screen: self.parser.screen().clone(),
    }
  }

  #[instrument("pty_state_snapshot_recycled", skip_all)]
  pub fn snapshot_recycled(&self, recycled: &mut PtyStateView) {
    Clone::clone_from(&mut recycled.screen, self.parser.screen());
  }

  pub fn resize(&mut self, rows: NonZeroU16, cols: NonZeroU16) {
    tracing::debug!("attempting to set new pty size: {rows} rows, {cols} cols");
    self.parser.screen_mut().set_size(rows.get(), cols.get());
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
