mod launch;
mod state;

use std::sync::mpsc;

use tracing::{debug, info, info_span};
use winit::{self, dpi::PhysicalSize, event::WindowEvent};

pub use self::state::AppState;
use crate::{
  draw::FrameInput,
  event::{Event, WindowingEvent, WinitEventLoopEvent},
  executor::{Command, EventLoopCommand},
  window_handle::WindowHandle,
};

/// The fundamental decision-maker and state-holder.
///
/// All events come into [`App`]. In response to an event, the app may:
/// - Mutate something in [`AppState`]
/// - Send a command to the [`Executor`]
/// - Kick a frame off to a [`WindowHandle`]
///
/// The [`App`] is meant to run a really tight loop because all input and events
/// flow through it. If processing the event takes too long and the watchdog
/// timer is tripped, a warning log will fire.
///
/// If the thing you want to do takes more than just a quick state mutation,
/// turn it into a command. Package whatever state you need and send it to the
/// [`Executor`], and then fire an event when it's done. If you need, you can
/// receive that event and kick off another command or event. Events and
/// commands can be chained easily. You can package a state machine that you
/// pass back and forth if you wish.
///
/// ## Flows
/// ### Starting the main window
/// - [`WinitEventLoopEvent::Resumed`] is received, meaning the [`WinitApp`] got
///   it's `resumed` method called.
/// - The [`App`] sends the [`EventLoopCommand::BuildWindow`] command, which the
///   [`Executor`] forwards to the [`WinitApp`].
/// - The [`WinitApp`] builds the window, and sends it back as the
///   [`WindowingEvent::WindowBuilt`].
pub struct App {
  event_rx:   mpsc::Receiver<Event>,
  state:      state::AppState,
  command_tx: mpsc::Sender<Command>,
}

impl App {
  /// Syntax sugar for sending a command
  fn command(&self, command: Command) {
    tracing::debug!(?command, "sending command");
    self.command_tx.send(command).unwrap();
  }

  /// Run the app event loop.
  pub fn run(&mut self) -> miette::Result<()> {
    while let Ok(event) = self.event_rx.recv() {
      let _span = info_span!("event_dispatch", event = ?event).entered();

      match event {
        // mainline event loop control flow
        Event::Windowing(box WindowingEvent::EventLoop(
          WinitEventLoopEvent::Resumed,
        )) => {
          debug!("received resumed event => building window");
          self
            .command(Command::EventLoopCommand(EventLoopCommand::BuildWindow));
        }
        Event::Windowing(box WindowingEvent::EventLoop(
          WinitEventLoopEvent::Suspended,
        )) => {
          debug!("received suspended event => destroying window");
          self.drop_window();
        }
        Event::Windowing(box WindowingEvent::EventLoop(
          WinitEventLoopEvent::Exiting,
        )) => {
          info!("winit event loop is exiting => ending app loop");
          break;
        }

        // resized
        Event::Windowing(box WindowingEvent::Window(
          _,
          WindowEvent::Resized(new_size),
        )) => {
          self.affect_resize(new_size);
        }
        // scale factor changed
        Event::Windowing(box WindowingEvent::Window(
          _,
          WindowEvent::ScaleFactorChanged { scale_factor, .. },
        )) => {
          self.affect_scale_factor_change(scale_factor);
        }
        // redraw requested
        Event::Windowing(box WindowingEvent::Window(
          _,
          WindowEvent::RedrawRequested,
        )) => {
          self.initiate_frame();
        }
        // close requested
        Event::Windowing(box WindowingEvent::Window(
          _,
          WindowEvent::CloseRequested,
        )) => {
          self.shut_down_app();
          return Ok(());
        }

        Event::Windowing(box WindowingEvent::Window(_, _window_event)) => {
          // tracing::debug!(window.id = ?w_id, "ignoring unimplemented window
          // event");
          if let Some(wh) = self.get_window_handle() {
            wh.request_redraw();
          }
        }
        Event::Windowing(box WindowingEvent::Device(_, _device_event)) => {
          // tracing::debug!(device.id = ?d_id, "ignoring unimplemented device
          // event");
          // if let Some(wh) = self.get_window_handle() {
          //   wh.request_redraw();
          // }
        }

        Event::Windowing(box WindowingEvent::WindowBuilt(window)) => {
          self.command(Command::SpawnRenderer(window, self.state.gpu.clone()));
        }
        Event::RendererSpawned(window_handle) => {
          self.accept_window_handle(window_handle);
        }
        Event::ExitRequested => {
          self.shut_down_app();
          return Ok(());
        }
        Event::CriticalFailure(report) => {
          self.shut_down_app();
          return Err(report);
        }
      }
    }

    Ok(())
  }

  fn accept_window_handle(&mut self, window_handle: WindowHandle) {
    window_handle.request_redraw();
    self.state.window = Some(window_handle);
  }

  fn drop_window(&mut self) { self.state.window = None; }

  fn shut_down_app(&mut self) {
    tracing::info!("shutting down app");
    self.drop_window();
    self.command(Command::EventLoopCommand(EventLoopCommand::ExitEventLoop));
  }

  fn initiate_frame(&self) {
    let frame_input = FrameInput {};

    let Some(window_handle) = self.get_window_handle() else {
      tracing::warn!("attempted to initiate a frame without a window present");
      return;
    };

    window_handle.initiate_frame(frame_input);
  }

  fn affect_resize(&self, new_size: PhysicalSize<u32>) {
    let Some(window_handle) = self.get_window_handle() else {
      tracing::warn!("attempted to affect a resize without a window present");
      return;
    };

    window_handle.handle_resize(new_size);
  }

  fn affect_scale_factor_change(&self, new_scale_factor: f64) {
    let Some(window_handle) = self.get_window_handle() else {
      tracing::warn!(
        "attempted to affect a scale factor change without a window present"
      );
      return;
    };

    window_handle.handle_scale_factor_change(new_scale_factor);
  }

  fn get_window_handle(&self) -> Option<&WindowHandle> {
    self.state.window.as_ref()
  }
}
