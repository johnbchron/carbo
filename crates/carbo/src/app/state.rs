mod config;

use std::sync::Arc;

use miette::Context;
use tracing::instrument;

use self::config::AppConfig;
use crate::{
  gpu_context::GpuContext,
  pty::{PtyHandle, PtyStateView},
  window_handle::WindowHandle,
};

pub struct AppState {
  pub gpu:    Arc<GpuContext>,
  pub window: Option<WindowHandle>,
  pub pty:    PtyLifecyle,
  pub config: AppConfig,
}

impl AppState {
  #[instrument]
  pub fn build() -> miette::Result<Self> {
    Ok(AppState {
      gpu:    Arc::new(
        GpuContext::new().context("failed to build GPU context")?,
      ),
      window: None,
      pty:    PtyLifecyle::default(),
      config: AppConfig::default(),
    })
  }
}

#[derive(Default)]
pub enum PtyLifecyle {
  #[default]
  NotSpawned,
  Alive(PtyHandle, PtyStateView),
  Exited(PtyStateView),
}
