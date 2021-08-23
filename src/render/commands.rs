use ash::vk;

use super::{RenderState, SwapchainRebuilt};
use bevy::prelude::*;

pub(super) fn record_command_buffers_system(
    mut swapchain_rebuilt_events: EventReader<SwapchainRebuilt>,
    device: Res<ash::Device>,
    render_state: Res<RenderState>,
    raytracing_pipeline_loader: Res<ash::extensions::khr::RayTracingPipeline>,
    raytracing_pipeline_state: Res<crate::render::raytracer::RaytracingPipelineState>,
) {
    if swapchain_rebuilt_events.iter().next().is_none() {
        return;
    }
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
            device.cmd_bind_pipeline(
                command_buffer,
                vk::PipelineBindPoint::RAY_TRACING_KHR,
                raytracing_pipeline_state.pipeline,
            );
            device.cmd_bind_descriptor_sets(
                command_buffer,
                vk::PipelineBindPoint::RAY_TRACING_KHR,
                raytracing_pipeline_state.pipeline_layout,
                0,
                &[
                    swapchain_image.image_desc_set
                ],
                &[]
            );
            raytracing_pipeline_loader.cmd_trace_rays(
                command_buffer,
                &raytracing_pipeline_state.raygen_shader_binding_tables,
                &raytracing_pipeline_state.miss_shader_binding_tables,
                &raytracing_pipeline_state.hit_shader_binding_tables,
                &raytracing_pipeline_state.callable_shader_binding_tables,
                render_state.extent.width,
                render_state.extent.height,
                1
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
