use ash::vk;
use crate::raytrace::RayShaders;
use bevy::ecs::prelude::*;

use crate::render::RenderState;

pub fn record_raytracing_commands_system(
    device: Res<ash::Device>,
    render_state: Res<RenderState>,
    ray_shaders: Res<RayShaders>,
    entity_mapping_table: Res<super::tlas::UniformArray>,
    raytracing_pipeline_loader: Res<ash::extensions::khr::RayTracingPipeline>,
    queues: Res<crate::Queues>,
) {
    if entity_mapping_table.is_empty() {
        return;
    }
    let current_frame = render_state.current_frame().clone();
    assert_eq!(
        render_state.windows.len(),
        1,
        "TODO: Only supports 1 window at the moment."
    );

    let extracted_window = render_state.windows.values().next();
    if extracted_window.is_none() {
        println!("Cannot find the window!");
        return;
    }
    let extracted_window = extracted_window.unwrap();
    let surface_state = extracted_window.state.as_ref();
    if surface_state.is_none() {
        println!("Record commands: Cannot find the surface state!");
        return;
    }
    let surface_state = surface_state.unwrap();
    let command_buffer = current_frame.command_buffer;
    let swapchain_image = extracted_window
        .swapchain_image
        .as_ref()
        .expect("Record commands: Cannot find the swapchain image");

    unsafe {
        device
            .reset_command_buffer(
                command_buffer,
                vk::CommandBufferResetFlags::empty(),
            )
            .unwrap();
        device
            .begin_command_buffer(
                command_buffer,
                &vk::CommandBufferBeginInfo::builder()
                    .flags(vk::CommandBufferUsageFlags::empty())
                    .build(),
            )
            .unwrap();
        device.cmd_bind_pipeline(
            command_buffer,
            vk::PipelineBindPoint::RAY_TRACING_KHR,
            ray_shaders.pipeline,
        );
        device.cmd_bind_descriptor_sets(
            command_buffer,
            vk::PipelineBindPoint::RAY_TRACING_KHR,
            ray_shaders.pipeline_layout,
            0,
            &[swapchain_image.desc_set, ray_shaders.raytracing_resources_desc_set],
            &[]
        );

        // Sync entity mapping table
        device.cmd_copy_buffer(
            command_buffer,
            entity_mapping_table.get_staging_buffer(),
            entity_mapping_table.get_buffer(),
            &[vk::BufferCopy {
                src_offset: 0,
                dst_offset: 0,
                size: entity_mapping_table.get_full_size(),
            }],
        );


        device.cmd_pipeline_barrier(
            command_buffer,
            vk::PipelineStageFlags::TOP_OF_PIPE,
            vk::PipelineStageFlags::RAY_TRACING_SHADER_KHR,
            vk::DependencyFlags::BY_REGION,
            &[],
            &[],
            &[vk::ImageMemoryBarrier::builder()
                .src_access_mask(vk::AccessFlags::NONE_KHR)
                .dst_access_mask(vk::AccessFlags::SHADER_READ)
                .old_layout(vk::ImageLayout::UNDEFINED)
                .new_layout(vk::ImageLayout::GENERAL)
                .src_queue_family_index(queues.graphics_queue_family)
                .dst_queue_family_index(queues.graphics_queue_family)
                .image(swapchain_image.image)
                .subresource_range(vk::ImageSubresourceRange {
                    aspect_mask: vk::ImageAspectFlags::COLOR,
                    base_mip_level: 0,
                    level_count: 1,
                    base_array_layer: 0,
                    layer_count: 1,
                })
                .build()],
        );
        raytracing_pipeline_loader.cmd_trace_rays(
            command_buffer,
            &ray_shaders.sbt.raygen_shader_binding_tables,
            &ray_shaders.sbt.miss_shader_binding_tables,
            &ray_shaders.sbt.hit_shader_binding_tables,
            &ray_shaders.sbt.callable_shader_binding_tables,
            surface_state.extent.width,
            surface_state.extent.height,
            1,
        );
        device.cmd_pipeline_barrier(
            command_buffer,
            vk::PipelineStageFlags::RAY_TRACING_SHADER_KHR,
            vk::PipelineStageFlags::BOTTOM_OF_PIPE,
            vk::DependencyFlags::BY_REGION,
            &[],
            &[],
            &[vk::ImageMemoryBarrier::builder()
                .src_access_mask(vk::AccessFlags::SHADER_WRITE)
                .dst_access_mask(vk::AccessFlags::NONE_KHR)
                .old_layout(vk::ImageLayout::GENERAL)
                .new_layout(vk::ImageLayout::PRESENT_SRC_KHR)
                .src_queue_family_index(queues.graphics_queue_family)
                .dst_queue_family_index(queues.graphics_queue_family)
                .image(swapchain_image.image)
                .subresource_range(vk::ImageSubresourceRange {
                    aspect_mask: vk::ImageAspectFlags::COLOR,
                    base_mip_level: 0,
                    level_count: 1,
                    base_array_layer: 0,
                    layer_count: 1,
                })
                .build()],
        );
        device
            .end_command_buffer(current_frame.command_buffer)
            .unwrap();
    }
}
