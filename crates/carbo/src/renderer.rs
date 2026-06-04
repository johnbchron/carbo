use std::{
  sync::{Arc, mpsc},
  thread::JoinHandle,
};

use miette::{Context, IntoDiagnostic};
use tracing::{info_span, instrument};
use vello::peniko::color::palette;
use wgpu::CommandEncoderDescriptor;
use winit::{dpi::PhysicalSize, window::Window};

use crate::{
  draw::{FrameInput, FullFrameInput},
  event_sender::EventSender,
  gpu_context::GpuContext,
  surface_state::SurfaceState,
};

/// The [`Renderer`] lives in its own thread, and is responsible for:
/// - Holding the surface and [`vello::Renderer`].
/// - Receiving resizing events and reconfiguring the surface when needed.
/// - Receiving [`FrameInput`]s, turning them into [`FullFrameInput`]s, drawing
///   them to a [`vello::Scene`], and then rendering them to the surface.
///
/// Interactions with the [`Renderer`] happen through the [`RendererHandle`].
/// There is only one [`RendererHandle`] per [`Renderer`], and it sends
/// [`RendererCommand`]s and controls the lifecycle of the [`Renderer`]. When
/// the [`RendererHandle`] drops, the [`Renderer`]'s thread ends and it drops.
/// The [`RendererHandle`] can send new [`FrameInput`]s to be rendered and
/// resizing and scale notifications.
///
/// To turn a [`FrameInput`] into a [`FullFrameInput`], we need the physical
/// size of the surface we're drawing to, the scale factor we're drawing at, and
/// the current frame count. We can keep the physical size stored in the
/// surface, but we have to keep track of the scale factor and frame count as
/// mutable state in the [`Renderer`].
pub struct Renderer {
  gpu:                  Arc<GpuContext>,
  renderer:             vello::Renderer,
  surface_state:        SurfaceState,
  current_scale_factor: f64,
  current_frame_count:  u64,
  renderer_command_rx:  mpsc::Receiver<RendererCommand>,
  cached_scene:         vello::Scene,
  window:               Arc<Window>,
  _event_tx:            EventSender,
}

/// Sent from the [`RendererHandle`] to the [`Renderer`].
enum RendererCommand {
  FrameInput(FrameInput),
  ChangedScaleFactor(f64),
  Resized(u32, u32),
}

/// An error indicating to skip the frame.
pub struct SkipFrame;

impl Renderer {
  /// Builds the [`Renderer`], starts it in its own thread, and returns a
  /// [`RendererHandle`].
  pub fn launch(
    gpu: Arc<GpuContext>,
    window: Arc<Window>,
    event_tx: EventSender,
  ) -> miette::Result<RendererHandle> {
    let (renderer_command_tx, renderer_command_rx) = mpsc::channel();

    let current_scale_factor = window.scale_factor();
    let surface_state = SurfaceState::new(gpu.clone(), window.clone())
      .context("failed to create a surface")?;

    let renderer = vello::Renderer::new(gpu.device(), vello::RendererOptions {
      use_cpu:              false,
      antialiasing_support: vello::AaSupport::area_only(),
      num_init_threads:     None,
      pipeline_cache:       None,
    })
    .into_diagnostic()
    .context("failed to create vello renderer")?;

    let renderer = Renderer {
      gpu,
      renderer,
      surface_state,
      current_scale_factor,
      current_frame_count: 0,
      renderer_command_rx,
      cached_scene: vello::Scene::new(),
      window,
      _event_tx: event_tx.clone(),
    };

    let join_handle = std::thread::Builder::new()
      .name("renderer".into())
      .spawn(move || {
        let mut renderer = renderer;
        renderer.run().unwrap();
      })
      .into_diagnostic()
      .context("failed to launch renderer thread")?;

    let handle = RendererHandle {
      _join_handle: join_handle,
      renderer_command_tx,
      event_tx,
    };

    Ok(handle)
  }

  /// Runs the [`Renderer`] event loop.
  fn run(&mut self) -> miette::Result<()> {
    // receive the first available message
    while let Ok(first) = self.renderer_command_rx.recv() {
      let _span = info_span!("command_dispatch").entered();
      // the frame we'll draw
      let mut pending_frame = None;
      // the latest resize seen during the drain; applied once after the loop
      let mut pending_resize = None;
      // the next command to execute
      let mut command = Some(first);

      // execute the command we have queued
      while let Some(cmd) = command {
        match cmd {
          // don't render yet, just store the frame input
          RendererCommand::FrameInput(frame_input) => {
            if pending_frame.is_some() {
              tracing::info!("coalesced frame input");
            }
            pending_frame = Some(frame_input);
          }
          RendererCommand::ChangedScaleFactor(new_scale_factor) => {
            self.current_scale_factor = new_scale_factor;
          }
          RendererCommand::Resized(physical_width, physical_height) => {
            if pending_resize.is_some() {
              tracing::info!("coalesced resize event");
            }
            pending_resize = Some((physical_width, physical_height));
          }
        }

        // get the next command if there is one
        command = self.renderer_command_rx.try_recv().ok();
      }

      // apply the latest resize once, after the drain
      if let Some((physical_width, physical_height)) = pending_resize {
        self.surface_state.resize_surface(
          self.gpu.device(),
          physical_width,
          physical_height,
        );
      }

      // render when there are no more commands waiting
      if let Some(frame_input) = pending_frame {
        let _ = self.render_frame(frame_input);
      }
    }

    Ok(())
  }

  /// Renders and presents a frame.
  #[instrument(skip_all)]
  fn render_frame(&mut self, frame_input: FrameInput) -> Result<(), SkipFrame> {
    let width = self.surface_state.config_width();
    let height = self.surface_state.config_height();

    // build the full frame input
    let full_frame_input = FullFrameInput::new(
      frame_input,
      (width, height),
      self.current_scale_factor,
      self.current_frame_count,
    );

    // draw into the scene
    let scene = &mut self.cached_scene;
    info_span!("draw_scene").in_scope(|| {
      scene.reset();
      full_frame_input.draw_to_scene(scene);
    });

    // render the scene to the target texture
    info_span!("vello_render").in_scope(|| {
      self
        .renderer
        .render_to_texture(
          self.gpu.device(),
          self.gpu.queue(),
          scene,
          self.surface_state.get_target_texture_view(),
          &vello::RenderParams {
            base_color: palette::css::BLACK,
            width,
            height,
            antialiasing_method: vello::AaConfig::Area,
          },
        )
        .expect("vello render failed");
    });

    // prepare to blit from the target view to the surface
    let mut encoder = self
      .gpu
      .device()
      .create_command_encoder(&CommandEncoderDescriptor::default());

    let surface_tex = info_span!("get_surface_tex")
      .in_scope(|| self.surface_state.get_current_surface_texture())?;

    // queue the blit op
    info_span!("encode").in_scope(|| {
      encoder.copy_texture_to_texture(
        self.surface_state.get_target_texture().as_image_copy(),
        surface_tex.texture.as_image_copy(),
        wgpu::Extent3d {
          width,
          height,
          depth_or_array_layers: 1,
        },
      );
    });

    // hint the window and submit all the work to the GPU
    info_span!("pre_present_notify").in_scope(|| {
      self.window.pre_present_notify();
    });
    info_span!("submit_to_queue").in_scope(|| {
      self.gpu.queue().submit([encoder.finish()]);
    });

    // present the frame
    surface_tex.present();

    self.current_frame_count += 1;

    Ok(())
  }
}

/// The handle returned by [`Renderer::launch`]. This is the only way to
/// interact with the [`Renderer`], and dropping it will stop the [`Renderer`]
/// after it finishes the work at hand.
#[derive(Debug)]
pub struct RendererHandle {
  _join_handle:        JoinHandle<()>,
  renderer_command_tx: mpsc::Sender<RendererCommand>,
  event_tx:            EventSender,
}

impl RendererHandle {
  /// Sends a [`FrameInput`] to the renderer, to be drawn and rendered to the
  /// [`Renderer`]'s surface.
  pub fn send_frame_input(&self, input: FrameInput) {
    self.command(RendererCommand::FrameInput(input));
  }

  /// Notifies the [`Renderer`] of a resize event, and prompts it to reconfigure
  /// its surface.
  pub fn send_resize(&self, new_size: PhysicalSize<u32>) {
    self.command(RendererCommand::Resized(new_size.width, new_size.height));
  }

  /// Notifies the [`Renderer`] of a scale factor change, prompting it to render
  /// later frames at this scale factor.
  pub fn send_scale_factor_change(&self, new_scale_factor: f64) {
    self.command(RendererCommand::ChangedScaleFactor(new_scale_factor));
  }

  fn command(&self, command: RendererCommand) {
    if let Err(e) = self
      .renderer_command_tx
      .send(command)
      .into_diagnostic()
      .context("failed to send render command to renderer")
    {
      self.event_tx.event(crate::event::Event::CriticalFailure {
        message: "failed to send renderer command to renderer because the \
                  renderer thread has exited"
          .into(),
        error:   e,
      });
    }
  }
}
