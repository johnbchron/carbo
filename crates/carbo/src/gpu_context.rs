use tracing::instrument;
use wgpu::{
  Backends, Device, DeviceDescriptor, Instance, Queue, RequestAdapterOptions,
};

/// Holds long-lived [`wgpu`] GPU resources used in all rendering operations. It
/// can be constructed once and shared everywhere.
#[derive(Debug)]
pub struct GpuContext {
  queue:    Queue,
  device:   Device,
  instance: Instance,
}

impl GpuContext {
  /// Constructs and provisions all the resources needed in [`GpuContext`].
  #[instrument]
  pub fn new() -> miette::Result<Self> {
    // no support for GL
    let instance_descriptor = wgpu::InstanceDescriptor {
      backends: Backends::PRIMARY,
      ..wgpu::InstanceDescriptor::from_env_or_default()
    };
    let instance = Instance::new(&instance_descriptor);

    let (device, queue) = pollster::block_on(async {
      let adapter = instance
        .request_adapter(&RequestAdapterOptions::default())
        .await
        .expect("no suitable adapter");
      let (device, queue) = adapter
        .request_device(&DeviceDescriptor::default())
        .await
        .expect("failed to create device");
      (device, queue)
    });

    Ok(Self {
      instance,
      device,
      queue,
    })
  }

  pub fn queue(&self) -> &Queue { &self.queue }

  pub fn device(&self) -> &Device { &self.device }

  pub fn instance(&self) -> &Instance { &self.instance }
}
