use std::sync::Arc;

use miette::Context;
use tracing::instrument;

use crate::{gpu_context::GpuContext, window_handle::WindowHandle};

pub struct AppState {
  pub(crate) gpu:    Arc<GpuContext>,
  pub(crate) window: Option<WindowHandle>,
}

impl AppState {
  #[instrument]
  pub fn build() -> miette::Result<Self> {
    Ok(AppState {
      gpu:    Arc::new(
        GpuContext::new().context("failed to build GPU context")?,
      ),
      window: None,
    })
  }
}
