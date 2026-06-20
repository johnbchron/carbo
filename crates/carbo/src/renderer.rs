use std::{
  sync::{Arc, mpsc},
  thread::JoinHandle,
};

use miette::{Context, IntoDiagnostic};
use tracing::{info_span, instrument};
use vello::peniko::color::palette;
use wgpu::{CommandEncoderDescriptor, TextureViewDescriptor};
use winit::{dpi::PhysicalSize, window::Window};

use crate::{
  draw::{FrameInput, FullFrameInput, PersistedDrawingResources},
  event::Event,
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
  /// GPU context handle
  gpu:                  Arc<GpuContext>,
  /// vello renderer
  renderer:             vello::Renderer,
  /// state for the window's surface; used to present frames.
  surface_state:        SurfaceState,
  /// the last scale factor sent by the app. held here to sync to frame inputs
  /// correctly.
  current_scale_factor: f64,
  /// incremented on frame presentation.
  current_frame_count:  u64,
  /// receives communication from the [`RendererHandle`].
  renderer_command_rx:  mpsc::Receiver<RendererCommand>,
  /// retained vello scene to reuse allocations
  cached_scene:         vello::Scene,
  /// handle to the window for notifying that we're about to present a frame
  window:               Arc<Window>,
  /// EventSender just in case.
  _event_tx:            EventSender,
  persisted_resources:  PersistedDrawingResources,
}

/// Sent from the [`RendererHandle`] to the [`Renderer`].
enum RendererCommand {
  BlankFrame,
  FrameInput(FrameInput),
  ChangedScaleFactor(f64),
  Resized(u32, u32),
}

/// An error indicating to skip the frame.
pub struct SkipFrame;

impl Renderer {
  /// Builds the [`Renderer`], starts it in its own thread, and returns a
  /// [`RendererHandle`].
  #[instrument("launch_renderer", skip_all)]
  pub fn launch(
    gpu: Arc<GpuContext>,
    window: Arc<Window>,
    event_tx: EventSender,
  ) -> miette::Result<RendererHandle> {
    let (renderer_command_tx, renderer_command_rx) = mpsc::channel();

    let current_scale_factor = window.scale_factor();
    let surface_state = SurfaceState::new(gpu.clone(), window.clone())
      .context("failed to create a surface")?;

    let renderer = info_span!("create_vello_renderer")
      .in_scope(|| {
        vello::Renderer::new(gpu.device(), vello::RendererOptions {
          use_cpu:              false,
          antialiasing_support: vello::AaSupport::area_only(),
          num_init_threads:     None,
          pipeline_cache:       None,
        })
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
      persisted_resources: PersistedDrawingResources::default(),
    };

    let join_handle = std::thread::Builder::new()
      .name("renderer".into())
      .spawn({
        let event_tx = event_tx.clone();
        move || {
          let mut renderer = renderer;
          if let Err(e) = renderer.run() {
            event_tx.event(Event::CriticalFailure {
              message: "the renderer thread failed".into(),
              error:   e,
            });
          }
        }
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
      // a blank frame request
      let mut blank_frame_requested = false;
      // the latest resize seen during the drain; applied once after the loop
      let mut pending_resize = None;
      // the next command to execute
      let mut command = Some(first);

      // coalesce the commands that are waiting
      while let Some(cmd) = command {
        match cmd {
          // queue a blank frame
          RendererCommand::BlankFrame => {
            blank_frame_requested = true;
          }
          // don't render yet, just store the frame input
          RendererCommand::FrameInput(frame_input) => {
            if pending_frame.is_some() {
              tracing::debug!("coalesced frame input");
            }
            pending_frame = Some(frame_input);
          }
          RendererCommand::ChangedScaleFactor(new_scale_factor) => {
            self.current_scale_factor = new_scale_factor;
          }
          RendererCommand::Resized(physical_width, physical_height) => {
            if pending_resize.is_some() {
              tracing::debug!("coalesced resize event");
            }
            pending_resize = Some((physical_width, physical_height));
          }
        }

        // get the next command if there is one
        command = self.renderer_command_rx.try_recv().ok();
      }

      // apply the latest resize
      if let Some((physical_width, physical_height)) = pending_resize {
        self.surface_state.resize_surface(
          self.gpu.device(),
          physical_width,
          physical_height,
        );
      }

      // render a blank frame if requested
      if blank_frame_requested {
        let _ = self.render_blank_frame();
      }

      // render when there are no more commands waiting
      if let Some(frame_input) = pending_frame {
        let _ = self.render_frame_input(frame_input);
      }
    }

    Ok(())
  }

  /// Renders a frame input to a frame and presents it.
  fn render_frame_input(
    &mut self,
    frame_input: FrameInput,
  ) -> Result<(), SkipFrame> {
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
      full_frame_input.draw_to_scene(scene, &mut self.persisted_resources);
    });

    // render the scene
    self.render_current_scene()?;

    Ok(())
  }

  /// Renders a blank frame and presents it.
  fn render_blank_frame(&mut self) -> Result<(), SkipFrame> {
    // reset the scene
    self.cached_scene.reset();

    // render it
    self.render_current_scene()?;

    Ok(())
  }

  /// Renders the currently held scene to a frame and presents it.
  #[instrument(skip_all)]
  fn render_current_scene(&mut self) -> Result<(), SkipFrame> {
    let width = self.surface_state.config_width();
    let height = self.surface_state.config_height();

    // render the scene to the target texture
    info_span!("vello_render").in_scope(|| {
      self
        .renderer
        .render_to_texture(
          self.gpu.device(),
          self.gpu.queue(),
          &self.cached_scene,
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

    // get the current surface texture & a view into it
    let surface_tex = info_span!("get_surface_tex")
      .in_scope(|| self.surface_state.get_current_surface_texture())?;
    let surface_tex_view =
      surface_tex.texture.create_view(&TextureViewDescriptor {
        label: Some("surface_tex_view"),
        ..Default::default()
      });

    // queue the blit op from the vello target to the surface
    info_span!("encode_blit").in_scope(|| {
      self
        .surface_state
        .enqueue_blit(&mut encoder, &surface_tex_view);
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

  /// Sends a request to the renderer to produce a blank frame.
  pub fn send_blank_frame(&self) { self.command(RendererCommand::BlankFrame); }

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
      self.event_tx.event(Event::CriticalFailure {
        message: "failed to send renderer command to renderer because the \
                  renderer thread has exited"
          .into(),
        error:   e,
      });
    }
  }
}
