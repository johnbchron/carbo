use std::sync::mpsc;

use miette::{Context, IntoDiagnostic};
use winit::event_loop::{ControlFlow, EventLoop};

use super::App;
use crate::{
  event_sender::EventSender,
  executor::{EventLoopCommand, Executor},
  winit_app::WinitApp,
};

impl App {
  /// Builds the [`App`] and all the things it's connected to, and sets it all
  /// in motion.
  pub fn launch(state: crate::app::AppState) -> miette::Result<()> {
    // build the channels
    let (event_tx, event_rx) = mpsc::channel();
    let (command_tx, command_rx) = mpsc::channel();
    let event_tx = EventSender::new(event_tx);

    // build the winit app
    let mut winit_app = WinitApp::new(event_tx.clone());
    let window_event_loop = EventLoop::<EventLoopCommand>::with_user_event()
      .build()
      .into_diagnostic()
      .context("failed to build winit event loop")?;
    window_event_loop.set_control_flow(ControlFlow::Wait);
    // the event loop proxy goes into the executor
    let winit_tx = window_event_loop.create_proxy();

    // build the main app
    let app = App {
      event_loopback: event_tx.clone(),
      event_rx,
      state,
      command_tx,
    };

    // build the executor
    let executor =
      Executor::new(command_rx, event_tx, app.state.font_ctx.clone(), winit_tx);

    // launch the app thread
    std::thread::Builder::new()
      .name("app".into())
      .spawn(move || {
        let mut app = app;
        app.run().unwrap();
      })
      .into_diagnostic()
      .context("failed to launch app thread")?;

    // launch the executor thread
    std::thread::Builder::new()
      .name("executor".into())
      .spawn(move || {
        let mut executor = executor;
        executor.run().unwrap();
      })
      .into_diagnostic()
      .context("failed to launch executor thread")?;

    // run the winit event loop on the main thread for program lifecycle
    window_event_loop
      .run_app(&mut winit_app)
      .into_diagnostic()
      .context("failed to run winit event loop")?;

    // program exits
    Ok(())
  }
}
