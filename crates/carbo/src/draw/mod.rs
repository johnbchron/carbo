mod itemize;

use vello::{
  kurbo::{Affine, Circle, RoundedRect},
  peniko::{Brush, Fill, color::palette},
};

use crate::pty::PtyStateView;

/// A snapshot of [`AppState`](crate::app::AppState) containing all the domain
/// information needed to draw a frame.
pub struct FrameInput {
  pub pty: PtyStateView,
}

/// A [`FullFrameInput`] plus the rendering info needed to fully draw a frame.
pub struct FullFrameInput {
  input:         FrameInput,
  physical_size: (u32, u32),
  scale_factor:  f64,
  frame_count:   u64,
}

impl FullFrameInput {
  pub fn new(
    input: FrameInput,
    physical_size: (u32, u32),
    scale_factor: f64,
    frame_count: u64,
  ) -> Self {
    Self {
      input,
      physical_size,
      scale_factor,
      frame_count,
    }
  }
}

impl FullFrameInput {
  /// Draws into a [`vello::Scene`].
  pub fn draw_to_scene(&self, _scene: &mut vello::Scene) {}
}
