#![feature(box_patterns)]
#![feature(duration_millis_float)]

mod app;
mod draw;
mod event;
mod event_sender;
mod executor;
mod gpu_context;
mod renderer;
mod surface_state;
mod window_handle;
mod winit_app;

use miette::{Context, IntoDiagnostic};
use tracing::level_filters::LevelFilter;
use tracing_subscriber::{
  EnvFilter, layer::SubscriberExt, util::SubscriberInitExt,
};

fn main() -> miette::Result<()> {
  let perfetto = tracing_perfetto::PerfettoLayer::new(std::sync::Mutex::new(
    std::fs::File::create("/tmp/carbo.pftrace").unwrap(),
  ))
  .with_debug_annotations(true);
  tracing_subscriber::registry()
    .with(tracing_subscriber::fmt::layer())
    .with(
      EnvFilter::builder()
        .with_default_directive(LevelFilter::INFO.into())
        .from_env_lossy(),
    )
    .with(perfetto)
    .try_init()
    .into_diagnostic()?;

  let app_state =
    crate::app::AppState::build().context("failed to build app state")?;
  crate::app::App::launch(app_state)
}
