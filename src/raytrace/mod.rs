mod arena_alloc;
mod block_alloc;
mod ray_shaders;
mod sbt;
mod svdag;
mod tlas;
mod vox;
mod commands;

use ash::vk;

pub use tlas::Raytraced;
pub use vox::VoxelModel;

use bevy::prelude::*;
use crate::render::{RenderApp, RenderStage};

use self::block_alloc::{
    AllocatorCreateInfo, BlockAllocator, DiscreteBlockAllocator, IntegratedBlockAllocator,
};
pub use self::ray_shaders::RayShaders;
use self::tlas::TlasState;
use crate::device_info::DeviceInfo;
use crate::Queues;

use std::sync::Arc;

#[derive(Default)]
pub struct RaytracePlugin;


impl RaytracePlugin {
    fn add_block_allocator(&self, app: &mut App) {
        let render_app = app.sub_app(RenderApp);
        let device_info = render_app.world.get_resource::<DeviceInfo>().unwrap();
        let device = render_app
            .world
            .get_resource::<ash::Device>()
            .unwrap()
            .clone();
        let queue = render_app.world.get_resource::<Queues>().unwrap();
        let create_info = AllocatorCreateInfo {
            bind_transfer_queue: queue.transfer_binding_queue,
            bind_transfer_queue_family: queue.transfer_binding_queue_family,
            graphics_queue_family: queue.graphics_queue_family,
            block_size: arena_alloc::BLOCK_SIZE,
            max_storage_buffer_size: device_info
                .physical_device_properties
                .limits
                .max_storage_buffer_range as u64,
        };
        let block_allocator: Arc<dyn BlockAllocator> =
            match device_info.physical_device_properties.device_type {
                vk::PhysicalDeviceType::DISCRETE_GPU => unsafe {
                    let allocator = DiscreteBlockAllocator::new(
                        device,
                        &device_info.memory_properties,
                        &create_info,
                    );
                    Arc::new(allocator)
                },
                vk::PhysicalDeviceType::INTEGRATED_GPU => unsafe {
                    let allocator = IntegratedBlockAllocator::new(
                        device,
                        &device_info.memory_properties,
                        &create_info,
                    );
                    Arc::new(allocator)
                },
                _ => panic!("Unsupported GPU"),
            };
        render_app.insert_resource(block_allocator.clone());
        app.insert_resource(block_allocator);
    }
}

impl Plugin for RaytracePlugin {
    fn build(&self, app: &mut App) {
        let render_app = app.sub_app(RenderApp);
        render_app.add_plugin(tlas::TlasPlugin::default());
        render_app.add_system_to_stage(RenderStage::Prepare, update_desc_sets);

        self.add_block_allocator(app);
        app.add_plugin(vox::VoxPlugin::default());

        app.sub_app(RenderApp)
            .init_resource::<ray_shaders::RayShaders>()
            .add_system_to_stage(RenderStage::Queue, commands::record_raytracing_commands_system);
    }
}


struct RaytracingNodeViewConstants {
    pub camera_view_col0: [f32; 3],
    pub padding0: f32,
    pub camera_view_col1: [f32; 3],
    pub padding1: f32,
    pub camera_view_col2: [f32; 3],
    pub padding2: f32,

    pub camera_position: Vec3,
    pub tan_half_fov: f32,
}

/*

impl Node for RaytracingNode {
    fn input(&self) -> Vec<SlotInfo> {
        vec![
            SlotInfo::new(Self::IN_COLOR_ATTACHMENT, SlotType::TextureView),
            SlotInfo::new(Self::IN_DEPTH, SlotType::TextureView),
            SlotInfo::new(Self::IN_VIEW, SlotType::Entity),
        ]
    }

    fn run(
        &self,
        graph: &mut bevy::render2::render_graph::RenderGraphContext,
        render_context: &mut bevy::render2::renderer::RenderContext,
        world: &World,
    ) -> Result<(), bevy::render2::render_graph::NodeRunError> {
        let tlas_state = world.get_resource::<TlasState>().unwrap();
        if tlas_state.tlas == vk::AccelerationStructureKHR::null() {
            return Ok(());
        }
        let raytracing_pipeline_loader = world
            .get_resource::<ash::extensions::khr::RayTracingPipeline>()
            .unwrap();
        let device = world.get_resource::<ash::Device>().unwrap();
        let ray_shaders = world.get_resource::<RayShaders>().unwrap();
        let uniform_arr = world.get_resource::<tlas::UniformArray>().unwrap();
        let queues = world.get_resource::<Queues>().unwrap();
        let view = graph.get_input_entity(Self::IN_VIEW).unwrap();
        let view = world.get_entity(view).unwrap();
        let projection = view.get::<PerspectiveProjection>().unwrap();
        let view = view.get::<ExtractedView>().unwrap();
        let extent: (u32, u32) = (view.width, view.height);
        let view = unsafe {
            let rotation_matrix = Mat3::from_quat(view.transform.rotation).to_cols_array_2d();
            let mut contants: RaytracingNodeViewConstants =
                std::mem::MaybeUninit::uninit().assume_init();
            contants.camera_view_col0 = rotation_matrix[0];
            contants.camera_view_col1 = rotation_matrix[1];
            contants.camera_view_col2 = rotation_matrix[2];
            contants.camera_position = view.transform.translation;
            contants.tan_half_fov = (projection.fov / 2.0).tan(); // TODO
            contants
        };
        let (image, image_view) = {
            let render_target = graph.get_input_texture(Self::IN_COLOR_ATTACHMENT).unwrap();
            let mut image_view = vk::ImageView::null();
            let mut image = vk::Image::null();
            render_target.as_hal::<wgpu_hal::api::Vulkan, _>(|texture| {
                let texture = texture.unwrap();
                assert_eq!(extent.0, texture.extent.width);
                assert_eq!(extent.1, texture.extent.height);
                image_view = texture.raw.raw;
            });
            unsafe {
                render_target
                    .get_texture()
                    .as_hal::<wgpu_hal::api::Vulkan, _>(|texture| {
                        image = texture.unwrap().raw_handle()
                    });
            }
            (image, image_view)
        };
        let (depth_image, depth_image_view) = {
            let depth_texture_view = graph.get_input_texture(Self::IN_DEPTH).unwrap();
            let mut image_view = vk::ImageView::null();
            let mut image = vk::Image::null();
            depth_texture_view.as_hal::<wgpu_hal::api::Vulkan, _>(|texture| {
                let texture = texture.unwrap();
                assert_eq!(extent.0, texture.extent.width);
                assert_eq!(extent.1, texture.extent.height);
                image_view = texture.raw.raw;
            });
            unsafe {
                depth_texture_view
                    .get_texture()
                    .as_hal::<wgpu_hal::api::Vulkan, _>(|texture| {
                        image = texture.unwrap().raw_handle()
                    });
            }
            (image, image_view)
        };

        unsafe {
            device.update_descriptor_sets(
                &[
                    vk::WriteDescriptorSet::builder()
                        .dst_set(ray_shaders.target_img_desc_set)
                        .dst_binding(0)
                        .descriptor_type(vk::DescriptorType::STORAGE_IMAGE)
                        .image_info(&[vk::DescriptorImageInfo {
                            sampler: vk::Sampler::null(),
                            image_view,
                            image_layout: vk::ImageLayout::GENERAL, // TODO: ???
                        }])
                        .build(),
                    vk::WriteDescriptorSet::builder()
                        .dst_set(ray_shaders.target_img_desc_set)
                        .dst_binding(1)
                        .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                        .image_info(&[vk::DescriptorImageInfo {
                            sampler: ray_shaders.depth_sampler,
                            image_view: depth_image_view,
                            image_layout: vk::ImageLayout::GENERAL, // TODO: ???
                        }])
                        .build(),
                ],
                &[],
            );
            render_context
                .command_encoder
                .run_raw_command::<wgpu_hal::api::Vulkan, _>(|command_buffer| {
                    let command_buffer = command_buffer.active;
                    assert_ne!(command_buffer, vk::CommandBuffer::null());

                    let desc_sets = [ray_shaders.target_img_desc_set];
                    device.cmd_copy_buffer(
                        command_buffer,
                        uniform_arr.get_staging_buffer(),
                        uniform_arr.get_buffer(),
                        &[vk::BufferCopy {
                            src_offset: 0,
                            dst_offset: 0,
                            size: uniform_arr.get_full_size(),
                        }],
                    );
                    device.cmd_pipeline_barrier(
                        command_buffer,
                        vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
                        vk::PipelineStageFlags::RAY_TRACING_SHADER_KHR,
                        vk::DependencyFlags::BY_REGION,
                        &[],
                        &[],
                        &[vk::ImageMemoryBarrier::builder()
                            .src_access_mask(vk::AccessFlags::COLOR_ATTACHMENT_WRITE)
                            .dst_access_mask(vk::AccessFlags::SHADER_READ)
                            .old_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
                            .new_layout(vk::ImageLayout::GENERAL)
                            .src_queue_family_index(queues.graphics_queue_family)
                            .dst_queue_family_index(queues.graphics_queue_family)
                            .image(image)
                            .subresource_range(vk::ImageSubresourceRange {
                                aspect_mask: vk::ImageAspectFlags::COLOR,
                                base_mip_level: 0,
                                level_count: 1,
                                base_array_layer: 0,
                                layer_count: 1,
                            })
                            .build()],
                    );
                    device.cmd_pipeline_barrier(
                        command_buffer,
                        vk::PipelineStageFlags::LATE_FRAGMENT_TESTS,
                        vk::PipelineStageFlags::RAY_TRACING_SHADER_KHR,
                        vk::DependencyFlags::BY_REGION,
                        &[],
                        &[],
                        &[vk::ImageMemoryBarrier::builder()
                            .src_access_mask(vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE)
                            .dst_access_mask(vk::AccessFlags::SHADER_READ)
                            .old_layout(vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL)
                            .new_layout(vk::ImageLayout::GENERAL)
                            .src_queue_family_index(queues.graphics_queue_family)
                            .dst_queue_family_index(queues.graphics_queue_family)
                            .image(depth_image)
                            .subresource_range(vk::ImageSubresourceRange {
                                aspect_mask: vk::ImageAspectFlags::DEPTH,
                                base_mip_level: 0,
                                level_count: 1,
                                base_array_layer: 0,
                                layer_count: 1,
                            })
                            .build()],
                    );
                    device.cmd_bind_descriptor_sets(
                        command_buffer,
                        vk::PipelineBindPoint::RAY_TRACING_KHR,
                        ray_shaders.pipeline_layout,
                        0,
                        &desc_sets,
                        &[],
                    );
                    device.cmd_push_constants(
                        command_buffer,
                        ray_shaders.pipeline_layout,
                        vk::ShaderStageFlags::RAYGEN_KHR,
                        0,
                        &std::slice::from_raw_parts(
                            &view as *const RaytracingNodeViewConstants as *const u8,
                            std::mem::size_of_val(&view),
                        ),
                    );
                    device.cmd_bind_pipeline(
                        command_buffer,
                        vk::PipelineBindPoint::RAY_TRACING_KHR,
                        ray_shaders.pipeline,
                    );
                    raytracing_pipeline_loader.cmd_trace_rays(
                        command_buffer,
                        &ray_shaders.sbt.raygen_shader_binding_tables,
                        &ray_shaders.sbt.miss_shader_binding_tables,
                        &ray_shaders.sbt.hit_shader_binding_tables,
                        &ray_shaders.sbt.callable_shader_binding_tables,
                        extent.0,
                        extent.1,
                        1,
                    );
                    device.cmd_pipeline_barrier(
                        command_buffer,
                        vk::PipelineStageFlags::RAY_TRACING_SHADER_KHR,
                        vk::PipelineStageFlags::BOTTOM_OF_PIPE,
                        vk::DependencyFlags::BY_REGION,
                        &[],
                        &[],
                        &[
                            vk::ImageMemoryBarrier::builder()
                                .src_access_mask(vk::AccessFlags::SHADER_WRITE)
                                .dst_access_mask(vk::AccessFlags::empty())
                                .old_layout(vk::ImageLayout::GENERAL)
                                .new_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
                                .src_queue_family_index(queues.graphics_queue_family)
                                .dst_queue_family_index(queues.graphics_queue_family)
                                .image(image)
                                .subresource_range(vk::ImageSubresourceRange {
                                    aspect_mask: vk::ImageAspectFlags::COLOR,
                                    base_mip_level: 0,
                                    level_count: 1,
                                    base_array_layer: 0,
                                    layer_count: 1,
                                })
                                .build(),
                            vk::ImageMemoryBarrier::builder()
                                .src_access_mask(vk::AccessFlags::SHADER_WRITE)
                                .dst_access_mask(vk::AccessFlags::empty())
                                .old_layout(vk::ImageLayout::GENERAL)
                                .new_layout(vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL)
                                .src_queue_family_index(queues.graphics_queue_family)
                                .dst_queue_family_index(queues.graphics_queue_family)
                                .image(depth_image)
                                .subresource_range(vk::ImageSubresourceRange {
                                    aspect_mask: vk::ImageAspectFlags::DEPTH,
                                    base_mip_level: 0,
                                    level_count: 1,
                                    base_array_layer: 0,
                                    layer_count: 1,
                                })
                                .build(),
                        ],
                    );
                });
        }
        Ok(())
    }
}

*/


/// This system makes sure that RayShaders.target_img_desc_set always points to the correct acceleration structure and buffer
fn update_desc_sets(
    ray_shaders: Res<RayShaders>,
    tlas_state: Res<TlasState>,
    device: Res<ash::Device>,
    uniform_arr: Res<tlas::UniformArray>,
) {
    if uniform_arr.get_buffer() == vk::Buffer::null() {
        return;
    }
    unsafe {
        let mut write_desc_set_as_ext = vk::WriteDescriptorSetAccelerationStructureKHR::default();
        write_desc_set_as_ext.acceleration_structure_count = 1;
        write_desc_set_as_ext.p_acceleration_structures = &tlas_state.tlas;
        let mut write_desc_set_as = vk::WriteDescriptorSet::builder()
            .dst_set(ray_shaders.raytracing_resources_desc_set)
            .dst_binding(0)
            .descriptor_type(vk::DescriptorType::ACCELERATION_STRUCTURE_KHR)
            .build();
        write_desc_set_as.p_next = &write_desc_set_as_ext as *const _ as *const std::ffi::c_void;
        write_desc_set_as.descriptor_count = 1;
        device.update_descriptor_sets(
            &[
                write_desc_set_as,
                vk::WriteDescriptorSet::builder()
                    .dst_set(ray_shaders.raytracing_resources_desc_set)
                    .dst_binding(1)
                    .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                    .buffer_info(&[vk::DescriptorBufferInfo {
                        buffer: uniform_arr.get_buffer(),
                        range: uniform_arr.get_full_size(),
                        offset: 0,
                    }])
                    .build(),
            ],
            &[],
        );
    }
}