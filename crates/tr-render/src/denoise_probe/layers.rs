//! GPU continuation through the qualified layer operators and a dynamic first mask.
use super::*;
use tr_core::editing::layers::LayerStack;
pub(super) struct Layers {
    masks: mask::Masks,
    operators: crate::layer_compute::LayerCompute,
}
impl Layers {
    pub fn new(device: &wgpu::Device) -> Self {
        Self {
            masks: mask::Masks::new(device),
            operators: crate::layer_compute::LayerCompute::new(device),
        }
    }
    #[allow(clippy::too_many_arguments)]
    pub fn encode(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        input: &wgpu::Buffer,
        origin: [u32; 2],
        size: [u32; 2],
        full: [u32; 2],
        native: [u32; 2],
        stack: &LayerStack,
        cancelled: &(impl Fn() -> bool + Sync),
    ) -> Result<wgpu::Buffer> {
        ensure!(!cancelled(), "Editing annullato");
        crate::layer_compute::parameters(stack)?;
        ensure!(
            stack.point_count() <= 512,
            "Layer mask points exceed admitted metadata"
        );
        let first = stack
            .layers
            .iter()
            .find(|l| l.enabled && !l.operators.is_empty())
            .ok_or_else(|| anyhow::anyhow!("No enabled layer slot"))?;
        let weights = self.masks.encode(
            device,
            encoder,
            input,
            origin,
            size,
            full,
            native,
            &first.mask,
            cancelled,
        )?;
        let pixels = u64::from(size[0]) * u64::from(size[1]);
        let output = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: pixels * 16,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        self.operators.encode(
            device,
            encoder,
            input,
            &output,
            &weights,
            stack,
            pixels as u32,
        )?;
        ensure!(!cancelled(), "Editing annullato");
        Ok(output)
    }
}
