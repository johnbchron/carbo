use std::num::NonZeroU16;

#[derive(Debug, Clone)]
pub struct PtyState {}

impl PtyState {
  pub fn new(_rows: NonZeroU16, _cols: NonZeroU16) -> Self { Self {} }
}
