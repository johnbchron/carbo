mod itemize;

use self::itemize::ItemizerPersistentResources;
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
  pub fn draw_to_scene(
    self,
    scene: &mut vello::Scene,
    pr: &mut PersistedDrawingResources,
  ) {
    let FullFrameInput {
      input,
      physical_size,
      scale_factor,
      frame_count: _frame_count,
    } = self;

    let _text_runs = input.itemize_text_runs(&mut pr.itemizer_pr);

    let logical_width = physical_size.0 as f64 / scale_factor;
    let logical_height = physical_size.1 as f64 / scale_factor;

    let zoom = vello::kurbo::Affine::scale(scale_factor);
    let rect = vello::kurbo::Rect::new(0.0, 0.0, logical_width, logical_height);
    scene.fill(
      vello::peniko::Fill::NonZero,
      zoom,
      &vello::peniko::Brush::Solid(vello::peniko::color::palette::css::MAGENTA),
      None,
      &rect,
    );
  }
}

#[derive(Default)]
pub struct PersistedDrawingResources {
  itemizer_pr: ItemizerPersistentResources,
}
