//! Ordered layer groups: a mask reads the output of all preceding layers.
//! Two alternating RGB buffers and one weight buffer bound pixel scratch,
//! independently of the number of groups. No inter-group alpha conversion.
use super::*;
use tr_core::editing::layers::LayerStack;

fn groups(stack: &LayerStack) -> Result<Vec<LayerStack>> {
    stack.validate()?;
    ensure!(
        stack.point_count() <= 512,
        "Layer mask points exceed admitted metadata"
    );
    let mut groups: Vec<LayerStack> = Vec::new();
    for layer in stack
        .layers
        .iter()
        .filter(|l| l.enabled && !l.operators.is_empty())
    {
        if groups.is_empty() || layer.mask.root.is_some() {
            groups.push(LayerStack {
                base_process: stack.base_process,
                layers: Vec::new(),
            });
        }
        groups.last_mut().unwrap().layers.push(layer.clone());
    }
    ensure!(!groups.is_empty(), "No enabled layer slot");
    for group in &groups {
        crate::layer_compute::parameters(group)?;
    }
    Ok(groups)
}

pub(super) fn working_bytes(stack: &LayerStack, size: [u32; 2]) -> Result<u64> {
    let count = groups(stack)?.len() as u64;
    let pixels = u64::from(size[0]) * u64::from(size[1]);
    // Parameters, mask topology and brush segments remain alive in the encoded
    // command buffer; only the pixel storage is reused between dispatches.
    Ok(pixels * (16 * count.min(2) + 4) + count * 8 * 65536)
}
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
        let groups = groups(stack)?;
        let pixels = u64::from(size[0]) * u64::from(size[1]);
        let buffer = |bytes, label| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: bytes,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            })
        };
        let weights = buffer(pixels * 4, "TR reused layer mask weights");
        let outputs: Vec<_> = (0..groups.len().min(2))
            .map(|_| buffer(pixels * 16, "TR alternating layer RGB"))
            .collect();
        for (index, group) in groups.iter().enumerate() {
            ensure!(!cancelled(), "Editing annullato");
            let source = if index == 0 {
                input
            } else {
                &outputs[(index - 1) % 2]
            };
            self.masks.encode_into(
                device,
                encoder,
                source,
                origin,
                size,
                full,
                native,
                &group.layers[0].mask,
                &weights,
                index != 0,
                cancelled,
            )?;
            let mut words = crate::layer_compute::parameters(group)?;
            words[0][2] = u32::from(index != 0) as f32;
            words[0][3] = u32::from(index + 1 < groups.len()) as f32;
            self.operators.encode_words(
                device,
                encoder,
                source,
                &outputs[index % 2],
                &weights,
                &words,
                pixels as u32,
            );
        }
        ensure!(!cancelled(), "Editing annullato");
        Ok(outputs[(groups.len() - 1) % 2].clone())
    }
}
