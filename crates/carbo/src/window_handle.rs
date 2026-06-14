use std::sync::Arc;

use winit::{dpi::PhysicalSize, window::Window};

use crate::{draw::FrameInput, renderer::RendererHandle};

/// Holds all window state and rendering responsibility.
#[derive(Debug)]
pub struct WindowHandle {
  window:   Arc<Window>,
  renderer: RendererHandle,
}

impl WindowHandle {
  pub fn new(window: Arc<Window>, renderer: RendererHandle) -> Self {
    Self { window, renderer }
  }

  /// Sends a redraw request for the window to [`winit`].
  pub fn request_redraw(&self) { self.window.request_redraw(); }

  /// Kicks off a frame. The [`Renderer`](crate::renderer::Renderer) will
  /// present it to the window when it's ready.
  pub fn initiate_frame(&self, frame_input: FrameInput) {
    self.renderer.send_frame_input(frame_input);
  }

  /// Sends a blank frame to the [`Renderer`](crate::renderer::Renderer).
  pub fn initiate_blank_frame(&self) { self.renderer.send_blank_frame(); }

  /// Handles a resize event.
  pub fn handle_resize(&self, new_size: PhysicalSize<u32>) {
    self.renderer.send_resize(new_size);
    self.request_redraw();
  }

  /// Handles a scale factor change event.
  pub fn handle_scale_factor_change(&self, new_scale_factor: f64) {
    self.renderer.send_scale_factor_change(new_scale_factor);
    self.request_redraw();
  }
}
