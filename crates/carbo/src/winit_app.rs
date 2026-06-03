use std::{sync::Arc, time::Instant};

use miette::Context;
use tracing::debug;
use winit::{
  application::ApplicationHandler,
  dpi::LogicalSize,
  event::DeviceId,
  event_loop::ActiveEventLoop,
  window::{Window, WindowId},
};

use crate::{
  event::{Event, WindowingEvent, WinitEventLoopEvent},
  event_sender::EventSender,
  executor::EventLoopCommand,
  renderer::Renderer,
  window_handle::WindowHandle,
};

/// The app passed to the [`winit`] event loop.
///
/// [`WinitApp`] receives commands from the [`App`](crate::app::App), forwarded
/// by the [`Executor`](crate::executor::Executor) to the
/// [`EventLoopProxy`](winit::event_loop::EventLoopProxy).
///
/// It forwards all callbacks it receives to the [`App`](crate::app::App) as
/// events.
///
/// This must run on the main thread due to platform windowing restrictions.
pub struct WinitApp {
  event_tx: EventSender,
}

impl WinitApp {
  pub fn new(event_tx: EventSender) -> Self { Self { event_tx } }

  /// Processes a command (user event) sent by the
  /// [`EventLoopProxy`](winit::event_loop::EventLoopProxy).
  fn run_command(
    &mut self,
    event_loop: &ActiveEventLoop,
    command: EventLoopCommand,
  ) {
    match command {
      EventLoopCommand::BuildWindow => {
        let now = Instant::now();
        let attrs = Window::default_attributes()
          .with_title("carbo")
          .with_inner_size(LogicalSize::new(800, 600));
        let window = Arc::new(event_loop.create_window(attrs).unwrap());
        debug!("built window in {:.02}ms", now.elapsed().as_millis_f32());

        // app loop may already have exited. avoids a potential panic.
        let _ = self.event_tx.try_event(Event::Windowing(Box::new(
          WindowingEvent::WindowBuilt(window),
        )));
      }
      EventLoopCommand::ExitEventLoop => {
        event_loop.exit();
      }
      EventLoopCommand::SpawnRenderer(window, gpu) => {
        tracing::debug!(
          window.id = ?window.id(),
          "spawning renderer for window"
        );
        let now = Instant::now();
        let result =
          Renderer::launch(gpu, window.clone(), self.event_tx.clone())
            .context("failed to launch renderer thread");
        debug!(
          "launched renderer in {:.2}ms",
          now.elapsed().as_millis_f32()
        );

        match result {
          Ok(handle) => {
            self
              .event_tx
              .event(Event::RendererSpawned(WindowHandle::new(window, handle)));
          }
          Err(error) => {
            self.event_tx.event(Event::CriticalFailure {
              message: "failed to spawn a renderer".into(),
              error,
            });
          }
        }
      }
    }
  }
}

impl ApplicationHandler<EventLoopCommand> for WinitApp {
  fn resumed(&mut self, _event_loop: &ActiveEventLoop) {
    // app loop may already have exited. avoids a potential panic.
    let _ = self.event_tx.try_event(Event::Windowing(Box::new(
      WindowingEvent::EventLoop(WinitEventLoopEvent::Resumed),
    )));
  }

  fn window_event(
    &mut self,
    _event_loop: &ActiveEventLoop,
    window_id: WindowId,
    event: winit::event::WindowEvent,
  ) {
    // app loop may already have exited. avoids a potential panic.
    let _ = self.event_tx.try_event(Event::Windowing(Box::new(
      WindowingEvent::Window(window_id, event),
    )));
  }

  fn user_event(
    &mut self,
    event_loop: &ActiveEventLoop,
    command: EventLoopCommand,
  ) {
    self.run_command(event_loop, command);
  }

  fn device_event(
    &mut self,
    _event_loop: &ActiveEventLoop,
    device_id: DeviceId,
    event: winit::event::DeviceEvent,
  ) {
    // app loop may already have exited. avoids a potential panic.
    let _ = self.event_tx.try_event(Event::Windowing(Box::new(
      WindowingEvent::Device(device_id, event),
    )));
  }

  fn suspended(&mut self, _event_loop: &ActiveEventLoop) {
    // app loop may already have exited. avoids a potential panic.
    let _ = self.event_tx.try_event(Event::Windowing(Box::new(
      WindowingEvent::EventLoop(WinitEventLoopEvent::Suspended),
    )));
  }

  fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
    // app loop may already have exited. avoids a potential panic.
    let _ = self.event_tx.try_event(Event::Windowing(Box::new(
      WindowingEvent::EventLoop(WinitEventLoopEvent::Exiting),
    )));
  }
}
