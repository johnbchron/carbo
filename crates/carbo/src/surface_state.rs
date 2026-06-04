use std::sync::Arc;

use miette::{Context, IntoDiagnostic};
use tracing::{info_span, instrument};
use wgpu::{
  Surface, SurfaceConfiguration, SurfaceTexture, Texture, TextureDescriptor,
  TextureDimension, TextureFormat, TextureUsages, TextureView,
  TextureViewDescriptor,
};
use winit::window::Window;

use crate::{gpu_context::GpuContext, renderer::SkipFrame};

/// Manages a [`wgpu::Surface`]. This is held by the
/// [`Renderer`](crate::renderer::Renderer) and is used to present frames to the
/// window.
#[derive(Debug)]
pub struct SurfaceState {
  gpu:            Arc<GpuContext>,
  surface:        Surface<'static>,
  surface_config: SurfaceConfiguration,
  target_texture: Texture,
  target_view:    TextureView,
}

impl SurfaceState {
  /// Constructs a surface with its config, given the [`GpuContext`] and target
  /// [`Window`].
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

    let surface_config = SurfaceConfiguration {
      usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::COPY_DST,
      // required for STORAGE_ATTACHMENT on render target texture
      format: TextureFormat::Rgba8Unorm,
      width: size.width.max(1),
      height: size.height.max(1),
      present_mode: wgpu::PresentMode::AutoVsync,
      desired_maximum_frame_latency: 2,
      alpha_mode: wgpu::CompositeAlphaMode::Auto,
      view_formats: vec![],
    };

    surface.configure(gpu.device(), &surface_config);

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
      format:          surface_config.format,
      usage:           TextureUsages::STORAGE_BINDING | TextureUsages::COPY_SRC,
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
    })
  }

  /// Resizes and reconfigures the surface.
  pub fn resize_surface(
    &mut self,
    device: &wgpu::Device,
    width: u32,
    height: u32,
  ) {
    self.surface_config.width = width.max(1);
    self.surface_config.height = height.max(1);
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
        label:           Some("vello target"),
        size:            wgpu::Extent3d {
          width:                 self.config_width(),
          height:                self.config_height(),
          depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count:    1,
        dimension:       TextureDimension::D2,
        format:          self.config_format(),
        usage:           TextureUsages::STORAGE_BINDING
          | TextureUsages::COPY_SRC,
        view_formats:    &[],
      })
    });

    self.target_view =
      info_span!("create_target_texture_view").in_scope(|| {
        target_texture.create_view(&TextureViewDescriptor::default())
      });
    self.target_texture = target_texture;
  }

  /// The width specified in the surface config.
  pub fn config_width(&self) -> u32 { self.surface_config.width }

  /// The height specified in the surface config.
  pub fn config_height(&self) -> u32 { self.surface_config.height }

  /// The format specified in the surface config.
  pub fn config_format(&self) -> TextureFormat { self.surface_config.format }

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

  /// Returns the current target [`Texture`].
  pub fn get_target_texture(&self) -> &Texture { &self.target_texture }
}
