use std::sync::{Arc, mpsc};

use winit::{event_loop::EventLoopProxy, window::Window};

use crate::{
  event::Event,
  event_sender::EventSender,
  fonts::FontContext,
  gpu_context::GpuContext,
  pty::{Pty, PtySpawnArguments},
};

#[derive(Debug)]
/// Represents an action to be taken by the [`Executor`]. Commands can only be
/// sent by the [`App`](crate::app::App).
pub enum Command {
  /// Commands that must be executed by the
  /// [`WinitApp`](crate::winit_app::WinitApp).
  EventLoopCommand(EventLoopCommand),
  /// Spawns a PTY handling thread.
  SpawnPty(PtySpawnArguments),
}

/// Commands forwarded to the [`WinitApp`](crate::winit_app::WinitApp) to be
/// executed with access to the
/// [`ActiveEventLoop`](winit::event_loop::ActiveEventLoop) or on the main
/// thread.
#[derive(Debug)]
pub enum EventLoopCommand {
  /// Builds a window using the winit event loop.
  BuildWindow,
  /// Indicates to the winit event loop that it's time to exit.
  ExitEventLoop,
  /// Spawns a renderer into its own thread and returns a [`WindowHandle`] that
  /// holds the corresponding
  /// [`RendererHandle`](crate::renderer::RendererHandle). Sends back a
  /// [`Event::RendererSpawned`] event.
  SpawnRenderer(Arc<Window>, Arc<GpuContext>),
}

/// The executor receives [`Command`]s from the [`App`](crate::app::App) and
/// executes them.
///
/// Computation or interactions with external systems is meant to happen in the
/// executor. If the completion of a command requires a mutation of
/// [`AppState`](crate::app::AppState), the command should fire an event that
/// performs the mutation.
pub struct Executor {
  /// Receives the commands from [`App`](crate::app::App).
  command_rx: mpsc::Receiver<Command>,
  /// Sends events to [`App`](crate::app::App).
  event_tx:   EventSender,
  /// Font context for querying fonts and drawing text.
  font_ctx:   FontContext,
  /// Sends commands to [`WinitApp`](crate::winit_app::WinitApp).
  winit_tx:   EventLoopProxy<EventLoopCommand>,
}

impl Executor {
  pub fn new(
    command_rx: mpsc::Receiver<Command>,
    event_tx: EventSender,
    font_ctx: FontContext,
    winit_tx: EventLoopProxy<EventLoopCommand>,
  ) -> Self {
    Self {
      command_rx,
      event_tx,
      font_ctx,
      winit_tx,
    }
  }

  /// Runs the command loop of the executor. It will not end until the
  /// [`App`](crate::app::App) is dropped and with it its `command_tx:
  /// mpsc::Sender<Command>` field.
  pub fn run(&mut self) -> miette::Result<()> {
    while let Ok(command) = self.command_rx.recv() {
      match command {
        Command::EventLoopCommand(event_loop_command) => {
          tracing::debug!(
            ?event_loop_command,
            "forwarding command to winit event loop"
          );
          let _ = self.winit_tx.send_event(event_loop_command);
        }
        Command::SpawnPty(args) => {
          match Pty::launch(args, self.event_tx.clone()) {
            Ok((handle, pty_state_view)) => {
              self
                .event_tx
                .event(Event::PtySpawned(handle, pty_state_view));
            }
            Err(e) => {
              self.event_tx.event(Event::CriticalFailure {
                message: "failed to spawn a PTY".into(),
                error:   e,
              });
            }
          }
        }
      }
    }

    Ok(())
  }
}
