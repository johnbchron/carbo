mod launch;
pub mod state;

use std::{
  num::{NonZeroU16, NonZeroUsize},
  sync::{Arc, mpsc},
  time::{Duration, Instant},
};

use miette::IntoDiagnostic;
use tracing::{debug, info, info_span, instrument};
use winit::{self, dpi::PhysicalSize, event::WindowEvent};

pub use self::state::AppState;
use self::state::{PtyLifecyle, WindowState};
use crate::{
  draw::FrameInput,
  event::{Event, RendererEvent, WindowingEvent, WinitEventLoopEvent},
  event_sender::EventSender,
  executor::{Command, EventLoopCommand},
  pty::PtySpawnArguments,
  window_handle::WindowHandle,
};

const MAX_FRAME_DISPATCH_PROXIMITY: Duration = Duration::from_millis(4);

/// The fundamental decision-maker and state-holder.
///
/// All events come into [`App`]. In response to an event, the app may:
/// - Mutate something in [`AppState`]
/// - Send a command to the [`Executor`]
/// - Kick a frame off to a [`WindowHandle`]
///
/// ## Guidelines
/// The [`App`] is meant to run a really tight loop because all input and events
/// flow through it. If the thing you want to do takes more than just a quick
/// state mutation, turn it into a command. Package whatever state you need and
/// send it to the [`Executor`], and then fire an event when it's done. If you
/// need, you can receive that event and kick off another command or event.
/// Events and commands can be chained easily. You can package a state machine
/// that you pass back and forth if you wish.
///
/// Maintain logical flow in event handling. There are generally two types of
/// events: stimulus events (X thing happened) and intent events (it's time to
/// do X). If an action is not semantically tied to a stimulus event, create an
/// intent event and fire it from the stimulus event, e.g. don't fire the exit
/// machinery directly from a Ctrl+Q key window event, but rather fire an
/// ExitRequested event, and then fire the exit machinery from there.
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
  event_loopback: EventSender,
  event_rx:       mpsc::Receiver<Event>,
  state:          state::AppState,
  command_tx:     mpsc::Sender<Command>,
}

impl App {
  /// Syntax sugar for sending a command
  fn command(&self, command: Command) {
    tracing::debug!(?command, "sending command");
    self.command_tx.send(command).unwrap();
  }

  /// Run the app event loop.
  pub fn run(&mut self) -> miette::Result<()> {
    self.event_loopback.event(Event::ApplicationStarted);

    while let Ok(event) = self.event_rx.recv() {
      let _span = info_span!("event_dispatch", event = ?event).entered();

      match event {
        Event::ApplicationStarted => {
          self.command(Command::SpawnPty(PtySpawnArguments {
            rows:       NonZeroU16::new(24).unwrap(),
            cols:       NonZeroU16::new(80).unwrap(),
            scrollback: NonZeroUsize::new(3000).unwrap(),
          }));
          self.command(Command::LoadSystemFonts);
        }

        // mainline event loop control flow
        Event::Windowing(box WindowingEvent::EventLoop(
          WinitEventLoopEvent::Resumed,
        )) => {
          debug!("received winit resumed event => building window");
          self
            .command(Command::EventLoopCommand(EventLoopCommand::BuildWindow));
        }
        Event::Windowing(box WindowingEvent::EventLoop(
          WinitEventLoopEvent::Suspended,
        )) => {
          debug!("received winit suspended event => destroying window");
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
          info!("window closed, requesting app exit");
          self.event_loopback.event(Event::ExitRequested);
        }

        Event::Windowing(box WindowingEvent::Window(_, _window_event)) => {
          // tracing::debug!(window.id = ?w_id, "ignoring unimplemented window
          // event");
          self.request_frame();
        }
        Event::Windowing(box WindowingEvent::Device(_, _device_event)) => {
          // tracing::debug!(device.id = ?d_id, "ignoring unimplemented device
          // event");
          // if let Some(wh) = self.get_window_handle() {
          //   wh.request_redraw();
          // }
        }

        Event::Windowing(box WindowingEvent::WindowBuilt(window)) => {
          self.command(Command::EventLoopCommand(
            EventLoopCommand::SpawnRenderer(window, self.state.gpu.clone()),
          ));
        }
        Event::RendererSpawned {
          handle,
          logical_size,
        } => {
          self.accept_window_handle(handle, logical_size);
        }
        Event::RendererEvent(RendererEvent::LogicalResize {
          logical_width,
          logical_height,
        }) => {
          let Some(window_state) = self.state.window.as_mut() else {
            unreachable!("got a renderer event without a renderer")
          };
          window_state.last_logical_size = (logical_width, logical_height);
          debug!("got logical resize event from renderer, resizing");
          self.attempt_pty_resize();
        }
        Event::PtySpawned(new_pty_handle, new_pty_state) => {
          match &mut self.state.pty {
            // accept new pty
            state @ (PtyLifecyle::NotSpawned | PtyLifecyle::Exited(_)) => {
              tracing::info!("new pty spawned");
              *state = PtyLifecyle::Alive(new_pty_handle, new_pty_state);
              self.attempt_pty_resize();
            }
            // handle lifecycles shouldn't collide
            PtyLifecyle::Alive(..) => {
              self.event_loopback.event(Event::CriticalFailure {
                message: "new PTY was spawned while still holding the \
                          existing handle"
                  .into(),
                error:   miette::miette!("cannot swap in newly spawned PTY"),
              });
            }
          }
        }
        Event::PtySnapshot(new_pty_state) => {
          match &mut self.state.pty {
            PtyLifecyle::NotSpawned => unreachable!(
              "no pty state snapshots should be received before the pty is \
               spawned"
            ),
            PtyLifecyle::Alive(_, pty_state) => {
              tracing::debug!("received new pty state snapshot");
              *pty_state = new_pty_state;
            }
            PtyLifecyle::Exited(pty_state) => {
              tracing::debug!("received pty state snapshot after pty exited");
              *pty_state = new_pty_state;
            }
          }
          self.request_frame();
        }
        Event::PtyExited => {
          self.state.pty = match std::mem::replace(
            &mut self.state.pty,
            PtyLifecyle::NotSpawned,
          ) {
            PtyLifecyle::NotSpawned => {
              tracing::warn!("got pty child exited event without live pty");
              PtyLifecyle::NotSpawned
            }
            PtyLifecyle::Alive(_, pty_state) => {
              tracing::debug!("pty child exited; dropping pty handle");
              PtyLifecyle::Exited(pty_state)
            }
            exited @ PtyLifecyle::Exited(..) => {
              tracing::warn!("got duplicated pty child exited event");
              exited
            }
          };
          self.request_frame();
        }
        Event::SystemFontsLoaded => {
          tracing::info!("system fonts have been loaded");
          self.command(Command::ResolveTerminalFonts(
            self.state.config.font_config.clone(),
          ));
        }
        Event::TerminalFontsResolved(terminal_fonts) => {
          self.state.fonts = Some(Arc::new(terminal_fonts));
          self.attempt_pty_resize();
          tracing::info!("got fonts");
        }
        Event::ExitRequested => {
          self.shut_down_app();
          return Ok(());
        }
        Event::CriticalFailure {
          message,
          error: report,
        } => {
          tracing::error!(
            message,
            "catastrophic error occurred; shutting down app"
          );
          self.shut_down_app();
          return Err(
            report
              .context(message)
              .context("catastrophic error occured; shutting down app"),
          );
        }
      }
    }

    Ok(())
  }

  fn attempt_pty_resize(&mut self) {
    let Some(fonts) = self.state.fonts.as_ref() else {
      return;
    };
    let metrics = fonts.cell_metrics;

    let Some((logical_width, logical_height)) =
      self.state.window.as_ref().map(|ws| ws.last_logical_size)
    else {
      return;
    };

    let cols = NonZeroU16::new(
      (logical_width / metrics.cell_width() as f64).floor() as u16,
    )
    .unwrap_or(1.try_into().unwrap());
    let rows = NonZeroU16::new(
      (logical_height / metrics.cell_height() as f64).floor() as u16,
    )
    .unwrap_or(1.try_into().unwrap());

    match &self.state.pty {
      PtyLifecyle::NotSpawned | PtyLifecyle::Exited(_) => (),
      PtyLifecyle::Alive(pty_handle, _) => {
        pty_handle.resize(rows, cols);
      }
    }
  }

  fn accept_window_handle(
    &mut self,
    handle: WindowHandle,
    logical_size: (f64, f64),
  ) {
    handle.request_redraw();
    self.state.window = Some(WindowState {
      handle,
      last_logical_size: logical_size,
    });
  }

  fn drop_window(&mut self) { self.state.window = None; }

  fn shut_down_app(&mut self) {
    tracing::info!("shutting down app");

    // kill pty child
    if let PtyLifecyle::Alive(pty_handle, ..) = &mut self.state.pty
      && let Err(e) = pty_handle.kill_child().into_diagnostic()
    {
      self.event_loopback.event(Event::CriticalFailure {
        message: "failed to kill pty child while shutting down app".into(),
        error:   e,
      });
    };

    // shut down winit system
    self.drop_window();
    self.command(Command::EventLoopCommand(EventLoopCommand::ExitEventLoop));
  }

  fn request_frame(&self) {
    if let Some(wh) = self.get_window_handle() {
      wh.request_redraw();
    }
  }

  #[instrument(skip_all)]
  fn initiate_frame(&self) {
    let Some(window_handle) = self.get_window_handle() else {
      tracing::warn!("attempted to initiate a frame without a window present");
      return;
    };

    // skip frame if dispatched too recently
    if let Some(last_frame_dispatch) = window_handle.last_frame_dispatch() {
      let diff = Instant::now() - last_frame_dispatch;
      if diff < MAX_FRAME_DISPATCH_PROXIMITY {
        tracing::debug!(
          "skipping frame dispatch; last frame too recent ({:.02}ms ago)",
          diff.as_secs_f64() * 1000.0
        );
        return;
      }
    }

    // get the pty state
    let entered = info_span!("duplicate_pty_state").entered();
    let pty_state_view = match &self.state.pty {
      PtyLifecyle::NotSpawned => {
        tracing::warn!(
          "attempted to initiate a frame without a pty; sending blank frame"
        );
        window_handle.initiate_blank_frame();
        return;
      }
      PtyLifecyle::Alive(_, pty_state) | PtyLifecyle::Exited(pty_state) => {
        pty_state.clone()
      }
    };
    drop(entered);

    // get the terminal fonts
    let Some(terminal_fonts) = self.state.fonts.clone() else {
      tracing::warn!(
        "attempted to initiate a frame without fonts; sending blank frame"
      );
      window_handle.initiate_blank_frame();
      return;
    };

    let frame_input = FrameInput {
      pty: pty_state_view,
      terminal_fonts,
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
    self.state.window.as_ref().map(|ws| &ws.handle)
  }
}
