use vello::{
  kurbo::{Affine, Circle, RoundedRect},
  peniko::{Brush, Fill, color::palette},
};

use crate::pty::PtyStateView;

/// A snapshot of [`AppState`](crate::app::AppState) containing all the domain
/// information needed to draw a frame.
pub struct FrameInput {
  pub pty: Option<PtyStateView>,
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
  pub fn draw_to_scene(&self, scene: &mut vello::Scene) {
    let w = self.physical_size.0 as f64 / self.scale_factor;
    let h = self.physical_size.1 as f64 / self.scale_factor;
    let zoom = Affine::scale(self.scale_factor);

    let rect = RoundedRect::new(20.0, 20.0, w - 20.0, h - 20.0, 16.0);
    scene.fill(
      Fill::NonZero,
      zoom,
      &Brush::Solid(palette::css::DARK_SLATE_GRAY),
      None,
      &rect,
    );

    let t = self.frame_count as f64 * 0.02;
    let cx = w / 2.0 + t.cos() * 120.0;
    let cy = h / 2.0 + t.sin() * 120.0;
    scene.fill(
      Fill::NonZero,
      zoom,
      &Brush::Solid(palette::css::CORAL),
      None,
      &Circle::new((cx, cy), 40.0),
    );
  }
}
