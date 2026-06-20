use std::sync::Arc;

use derive_debug::Dbg;
use miette::{Context, IntoDiagnostic};
use tracing::{info_span, instrument};
use wgpu::{
  CommandEncoder, Surface, SurfaceConfiguration, SurfaceTexture, Texture,
  TextureDescriptor, TextureDimension, TextureFormat, TextureUsages,
  TextureView, TextureViewDescriptor, util::TextureBlitter,
};
use winit::window::Window;

use crate::{gpu_context::GpuContext, renderer::SkipFrame};

/// Manages a [`wgpu::Surface`]. This is used to present frames to the window.
#[derive(Dbg)]
pub struct SurfaceState {
  gpu:            Arc<GpuContext>,
  surface:        Surface<'static>,
  surface_config: SurfaceConfiguration,
  target_texture: Texture,
  target_view:    TextureView,
  #[dbg(skip)]
  blitter:        TextureBlitter,
}

impl SurfaceState {
  /// Constructs a surface with its config, given the [`GpuContext`] and target
  /// [`Window`].
  #[instrument("new_surface_state", skip_all)]
  pub fn new(
    gpu: Arc<GpuContext>,
    window: Arc<Window>,
  ) -> miette::Result<Self> {
    let size = window.inner_size();
    let surface = gpu
      .instance()
      .create_surface(window)
      .into_diagnostic()
      .context("failed to create surface from GPU instance")?;

    let width = size.width.max(1);
    let height = size.height.max(1);

    let default_surface_config = surface
      .get_default_config(gpu.adapter(), width, height)
      .ok_or_else(|| {
        miette::miette!(
          "failed to get default configuration for surface because the \
           surface isn't compatible with the adapter"
        )
      })?;

    // don't allow the OS to select manually to avoid SRGB formats
    let surface_format = cfg_select! {
      target_os = "linux" => TextureFormat::Rgba8Unorm,
      target_os = "macos" => TextureFormat::Bgra8Unorm,
    };
    let surface_config = SurfaceConfiguration {
      usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::COPY_DST,
      format: surface_format,
      present_mode: wgpu::PresentMode::AutoVsync,
      ..default_surface_config
    };

    surface.configure(gpu.device(), &surface_config);

    let blitter = TextureBlitter::new(gpu.device(), surface_config.format);

    let target_texture = gpu.device().create_texture(&TextureDescriptor {
      label:           Some("vello target"),
      size:            wgpu::Extent3d {
        width:                 surface_config.width,
        height:                surface_config.height,
        depth_or_array_layers: 1,
      },
      mip_level_count: 1,
      sample_count:    1,
      dimension:       TextureDimension::D2,
      // necessitated by vello
      format:          TextureFormat::Rgba8Unorm,
      // needs STORAGE_BINDING for vello and TEXTURE_BINDING for blitting
      usage:           TextureUsages::STORAGE_BINDING
        | TextureUsages::TEXTURE_BINDING,
      view_formats:    &[],
    });
    let target_view =
      target_texture.create_view(&TextureViewDescriptor::default());

    Ok(Self {
      gpu,
      surface,
      surface_config,
      target_texture,
      target_view,
      blitter,
    })
  }

  /// Resizes and reconfigures the surface.
  pub fn resize_surface(
    &mut self,
    device: &wgpu::Device,
    width: u32,
    height: u32,
  ) {
    // bail early
    if self.surface_config.width == width
      && self.surface_config.height == height
    {
      tracing::debug!("skipping surface resize: already at requested size");
      return;
    }

    self.surface_config.width = width.max(1);
    self.surface_config.height = height.max(1);
    tracing::debug!("resizing surface to ({width}, {height})");
    self.reconfigure_surface(device);
  }

  /// Reconfigures the surface with the current config.
  #[instrument(skip_all)]
  pub fn reconfigure_surface(&mut self, device: &wgpu::Device) {
    info_span!("surface_configure").in_scope(|| {
      self.surface.configure(device, &self.surface_config);
    });

    let target_texture = info_span!("create_target_texture").in_scope(|| {
      self.gpu.device().create_texture(&TextureDescriptor {
        label:           Some("vello_target"),
        size:            wgpu::Extent3d {
          width:                 self.config_width(),
          height:                self.config_height(),
          depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count:    1,
        dimension:       TextureDimension::D2,
        format:          self.surface_config.format,
        usage:           TextureUsages::STORAGE_BINDING
          | TextureUsages::TEXTURE_BINDING,
        view_formats:    &[],
      })
    });

    self.target_view =
      info_span!("create_target_texture_view").in_scope(|| {
        target_texture.create_view(&TextureViewDescriptor {
          label: Some("vello_target_view"),
          ..Default::default()
        })
      });
    self.target_texture = target_texture;
  }

  /// Encodes a blit operation from the held target texture to the given texture
  /// view.
  pub fn enqueue_blit(&self, encoder: &mut CommandEncoder, to: &TextureView) {
    self
      .blitter
      .copy(self.gpu.device(), encoder, &self.target_view, to);
  }

  /// The width specified in the surface config.
  pub fn config_width(&self) -> u32 { self.surface_config.width }

  /// The height specified in the surface config.
  pub fn config_height(&self) -> u32 { self.surface_config.height }

  /// Returns the next texture to be presented by the swapchain.
  pub fn get_current_surface_texture(
    &self,
  ) -> Result<SurfaceTexture, SkipFrame> {
    self
      .surface
      .get_current_texture()
      .inspect_err(|e| {
        tracing::error!("failed to get surface texture: {e}");
      })
      .map_err(|_| SkipFrame)
  }

  /// Returns a [`TextureView`] into the current target [`Texture`].
  pub fn get_target_texture_view(&self) -> &TextureView { &self.target_view }
}
