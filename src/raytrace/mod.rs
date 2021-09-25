mod arena_alloc;
mod block_alloc;
mod ray_pass_driver;
mod ray_shaders;
mod svdag;
mod tlas;
mod vox;

use ash::vk;

use bevy::render2::camera::PerspectiveProjection;
use bevy::render2::view::{ExtractedView, ViewMeta};
pub use tlas::Raytraced;
pub use vox::VoxelModel;

use bevy::prelude::*;
use bevy::render2::render_graph::{Node, RenderGraph, SlotInfo, SlotType};
use bevy::render2::RenderApp;

use self::block_alloc::{
    AllocatorCreateInfo, BlockAllocator, DiscreteBlockAllocator, IntegratedBlockAllocator,
};
use self::ray_shaders::RayShaders;
use self::tlas::TlasState;
use crate::device_info::DeviceInfo;
use crate::Queues;

use std::sync::Arc;

#[derive(Default)]
pub struct RaytracePlugin;

mod raytracing_graph {
    pub const NAME: &str = "ray_pass";
    pub mod input {
        pub const VIEW_ENTITY: &str = "view_entity";
        pub const RENDER_TARGET: &str = "render_target";
        pub const DEPTH: &str = "depth";
    }
}

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

        let raytracing_node = RaytracingNode::new(&mut render_app.world);

        let mut raytracing_graph = RenderGraph::default();
        raytracing_graph.add_node(RaytracingNode::NAME, raytracing_node);

        let input_node_id = raytracing_graph.set_input(vec![
            SlotInfo::new(raytracing_graph::input::VIEW_ENTITY, SlotType::Entity),
            SlotInfo::new(
                raytracing_graph::input::RENDER_TARGET,
                SlotType::TextureView,
            ),
            SlotInfo::new(raytracing_graph::input::DEPTH, SlotType::TextureView),
        ]);

        raytracing_graph
            .add_slot_edge(
                input_node_id,
                raytracing_graph::input::RENDER_TARGET,
                RaytracingNode::NAME,
                RaytracingNode::IN_COLOR_ATTACHMENT,
            )
            .unwrap();
        raytracing_graph
            .add_slot_edge(
                input_node_id,
                raytracing_graph::input::DEPTH,
                RaytracingNode::NAME,
                RaytracingNode::IN_DEPTH,
            )
            .unwrap();
        raytracing_graph
            .add_slot_edge(
                input_node_id,
                raytracing_graph::input::VIEW_ENTITY,
                RaytracingNode::NAME,
                RaytracingNode::IN_VIEW,
            )
            .unwrap();

        let mut graph = render_app.world.get_resource_mut::<RenderGraph>().unwrap();
        graph.add_sub_graph(raytracing_graph::NAME, raytracing_graph);
        graph.add_node(
            ray_pass_driver::RayPassDriverNode::NAME,
            ray_pass_driver::RayPassDriverNode,
        );
        graph
            .add_node_edge(
                bevy::core_pipeline::node::MAIN_PASS_DRIVER,
                ray_pass_driver::RayPassDriverNode::NAME,
            )
            .unwrap();

        render_app.add_plugin(tlas::TlasPlugin::default());

        self.add_block_allocator(app);
        app.add_plugin(vox::VoxPlugin::default());

        app.sub_app(RenderApp)
            .init_resource::<ray_shaders::RayShaders>();
    }
}

pub struct RaytracingNode {}

impl RaytracingNode {
    const NAME: &'static str = "main_pass";
    pub const IN_COLOR_ATTACHMENT: &'static str = "color_attachment";
    pub const IN_DEPTH: &'static str = "depth";
    pub const IN_VIEW: &'static str = "view";
    pub fn new(world: &mut World) -> Self {
        RaytracingNode {}
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
        let view_meta = world.get_resource::<ViewMeta>().unwrap();
        let ray_shaders = world.get_resource::<RayShaders>().unwrap();
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
            let mut write_desc_set_as_ext = vk::WriteDescriptorSetAccelerationStructureKHR::default();
            write_desc_set_as_ext.acceleration_structure_count = 1;
            write_desc_set_as_ext.p_acceleration_structures = &tlas_state.tlas;
            let mut write_desc_set_as = vk::WriteDescriptorSet::builder()
                .dst_set(ray_shaders.target_img_desc_set)
                .dst_binding(2)
                .descriptor_type(vk::DescriptorType::ACCELERATION_STRUCTURE_KHR)
                .build();
            write_desc_set_as.p_next =
                &write_desc_set_as_ext as *const _ as *const std::ffi::c_void;
            write_desc_set_as.descriptor_count = 1;

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
                    write_desc_set_as,
                ],
                &[],
            );
            render_context
                .command_encoder
                .run_raw_command::<wgpu_hal::api::Vulkan, _>(|command_buffer| {
                    let command_buffer = command_buffer.active;
                    assert_ne!(command_buffer, vk::CommandBuffer::null());

                    let desc_sets = [ray_shaders.target_img_desc_set];
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
                        &ray_shaders.raygen_shader_binding_tables,
                        &ray_shaders.miss_shader_binding_tables,
                        &ray_shaders.hit_shader_binding_tables,
                        &ray_shaders.callable_shader_binding_tables,
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
