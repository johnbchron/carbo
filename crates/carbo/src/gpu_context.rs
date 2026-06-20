use pollster::FutureExt;
use tracing::{info_span, instrument};
use wgpu::{
  Adapter, Backends, Device, DeviceDescriptor, Instance, Queue,
  RequestAdapterOptions,
};

/// Holds long-lived [`wgpu`] GPU resources used in all rendering operations. It
/// can be constructed once and shared everywhere.
#[derive(Debug)]
pub struct GpuContext {
  adapter:  Adapter,
  queue:    Queue,
  device:   Device,
  instance: Instance,
}

impl GpuContext {
  /// Constructs and provisions all the resources needed in [`GpuContext`].
  #[instrument("new_gpu_context")]
  pub fn new() -> miette::Result<Self> {
    // no support for GL
    let instance_descriptor = wgpu::InstanceDescriptor {
      backends: Backends::PRIMARY,
      ..wgpu::InstanceDescriptor::from_env_or_default()
    };
    let instance = info_span!("create_instance")
      .in_scope(|| Instance::new(&instance_descriptor));

    let adapter = info_span!("request_adapter")
      .in_scope(|| {
        instance
          .request_adapter(&RequestAdapterOptions::default())
          .block_on()
      })
      .expect("no suitable adapter");
    let (device, queue) = info_span!("request_device")
      .in_scope(|| {
        adapter
          .request_device(&DeviceDescriptor::default())
          .block_on()
      })
      .expect("failed to create device");

    Ok(Self {
      adapter,
      queue,
      device,
      instance,
    })
  }

  pub fn adapter(&self) -> &Adapter { &self.adapter }

  pub fn queue(&self) -> &Queue { &self.queue }

  pub fn device(&self) -> &Device { &self.device }

  pub fn instance(&self) -> &Instance { &self.instance }
}
