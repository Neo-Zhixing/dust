use ash::vk;

use super::RenderState;

pub(super) fn record_command_buffers(device: &ash::Device, render_state: &RenderState) {
    unsafe {
        for swapchain_image in render_state.swapchain_images.iter() {
            let command_buffer = swapchain_image.command_buffer;
            device
                .begin_command_buffer(
                    command_buffer,
                    &vk::CommandBufferBeginInfo::builder()
                        .flags(vk::CommandBufferUsageFlags::empty())
                        .build(),
                )
                .unwrap();

            device.cmd_set_viewport(
                command_buffer,
                0,
                &[vk::Viewport {
                    x: 0.0,
                    y: 0.0,
                    width: render_state.extent.width as f32,
                    height: render_state.extent.height as f32,
                    min_depth: 0.0,
                    max_depth: 1.0,
                }],
            );
            device.cmd_set_scissor(
                command_buffer,
                0,
                &[vk::Rect2D {
                    offset: vk::Offset2D { x: 0, y: 0 },
                    extent: render_state.extent,
                }],
            );
            device.cmd_pipeline_barrier(
                command_buffer,
                vk::PipelineStageFlags::COMPUTE_SHADER,
                vk::PipelineStageFlags::BOTTOM_OF_PIPE,
                vk::DependencyFlags::BY_REGION,
                &[],
                &[],
                &[vk::ImageMemoryBarrier::builder()
                    .image(swapchain_image.image)
                    .src_access_mask(vk::AccessFlags::SHADER_WRITE)
                    .dst_access_mask(vk::AccessFlags::empty())
                    .subresource_range(vk::ImageSubresourceRange {
                        aspect_mask: vk::ImageAspectFlags::COLOR,
                        base_mip_level: 0,
                        level_count: 1,
                        base_array_layer: 0,
                        layer_count: 1,
                    })
                    .old_layout(vk::ImageLayout::UNDEFINED)
                    .new_layout(vk::ImageLayout::PRESENT_SRC_KHR)
                    .build()],
            );
            device
                .end_command_buffer(swapchain_image.command_buffer)
                .unwrap();
        }
    }
}
