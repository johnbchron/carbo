use std::{
  fmt::Debug,
  num::{NonZeroU16, NonZeroUsize},
  sync::Arc,
};

use sharded_slab::{Clear, Pool, pool::OwnedRef};
use tracing::instrument;
use vt100::{Parser, Screen};

/// Wrapper to allow use with [`sharded_slab::Pool`].
struct PooledScreen(Screen);

impl Default for PooledScreen {
  fn default() -> Self { Self(Parser::new(25, 80, 0).screen().clone()) }
}

// no work needs to be done because we'll [`Clone::clone_into()`] it anyways
impl Clear for PooledScreen {
  fn clear(&mut self) {}
}

pub struct PtyState {
  /// Holds the vt100 state.
  parser: Parser,
  /// A pool of screen objects for state snapshots.
  pool:   Arc<Pool<PooledScreen>>,
}

impl PtyState {
  pub fn new(
    rows: NonZeroU16,
    cols: NonZeroU16,
    scrollback: NonZeroUsize,
  ) -> Self {
    Self {
      parser: Parser::new(rows.get(), cols.get(), scrollback.get()),
      pool:   Arc::new(Pool::new()),
    }
  }

  #[instrument("process_pty_input", skip_all)]
  pub fn process_input(&mut self, input: &[u8]) { self.parser.process(input); }

  #[instrument("pty_state_snapshot", skip_all)]
  pub fn snapshot(&self) -> PtyStateView {
    let mut slot = self
      .pool
      .clone()
      .create_owned()
      .expect("pty screen pool overflowed");
    slot.0.clone_from(self.parser.screen());
    PtyStateView {
      inner: Arc::new(slot.downgrade()),
    }
  }

  pub fn resize(&mut self, rows: NonZeroU16, cols: NonZeroU16) {
    tracing::debug!("attempting to set new pty size: {rows} rows, {cols} cols");
    self.parser.screen_mut().set_size(rows.get(), cols.get());
  }
}

#[derive(Clone)]
pub struct PtyStateView {
  inner: Arc<OwnedRef<PooledScreen>>,
}

impl Debug for PtyStateView {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.debug_struct("PtyStateView").finish()
  }
}

impl PtyStateView {
  pub fn screen(&self) -> &Screen { &self.inner.0 }
}
