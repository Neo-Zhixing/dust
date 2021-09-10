mod ray_pass_driver;
mod ray_shaders;
mod tlas;
use ash::vk;

pub use tlas::Raytraced;

use bevy::prelude::*;
use bevy::render2::render_graph::{Node, RenderGraph, SlotInfo, SlotType};
use bevy::render2::RenderApp;

use crate::Queues;

use self::ray_shaders::RayShaders;
use self::tlas::TlasState;

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
        graph.add_node_edge(bevy::core_pipeline::node::MAIN_PASS_DRIVER, ray_pass_driver::RayPassDriverNode::NAME).unwrap();

        render_app.add_plugin(tlas::TlasPlugin::default());
        render_app.init_resource::<ray_shaders::RayShaders>();
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
        let raytracing_pipeline_loader = world.get_resource::<ash::extensions::khr::RayTracingPipeline>().unwrap();
        let device = world.get_resource::<ash::Device>().unwrap();
        let ray_shaders  = world.get_resource::<RayShaders>().unwrap();
        let tlas_state  = world.get_resource::<TlasState>().unwrap();
        let queues = world.get_resource::<Queues>().unwrap();
        //let view = graph.get_input_entit y(Self::IN_VIEW).unwrap();


        let render_target = graph.get_input_texture(Self::IN_COLOR_ATTACHMENT).unwrap();
        let mut extent: (u32, u32) = (0, 0);
        let mut image_view = vk::ImageView::null();
        let mut image = vk::Image::null();
        render_target.as_hal::<wgpu_hal::api::Vulkan, _>(|texture| {
            let texture = texture.unwrap();
            extent = (texture.extent.width, texture.extent.height);
            image_view = texture.raw.raw;
        });
        unsafe {
            render_target.get_texture().as_hal::<wgpu_hal::api::Vulkan, _>(|texture| {
                image = texture.unwrap().raw_handle()
            });
        }
        unsafe {
            device.update_descriptor_sets(
                &[vk::WriteDescriptorSet::builder()
                    .dst_set(ray_shaders.target_img_desc_set)
                    .dst_binding(0)
                    .descriptor_type(vk::DescriptorType::STORAGE_IMAGE)
                    .image_info(&[vk::DescriptorImageInfo {
                        sampler: vk::Sampler::null(),
                        image_view,
                        image_layout: vk::ImageLayout::GENERAL, // TODO: ???
                    }])
                    .build()],
                &[],
            );
            render_context.command_encoder.run_raw_command::<wgpu_hal::api::Vulkan, _>(|command_buffer| {
                let command_buffer = command_buffer.active;
                assert_ne!(command_buffer, vk::CommandBuffer::null());

                let desc_sets = [ray_shaders.target_img_desc_set, tlas_state.desc_set];
                device.cmd_bind_descriptor_sets(
                    command_buffer,
                    vk::PipelineBindPoint::RAY_TRACING_KHR,
                    ray_shaders.pipeline_layout,
                    0,
                    &desc_sets,
                    &[],
                );
                device.cmd_pipeline_barrier(
                    command_buffer,
                    vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
                    vk::PipelineStageFlags::RAY_TRACING_SHADER_KHR,
                    vk::DependencyFlags::BY_REGION,
                    &[],
                    &[],
                    &[
                        vk::ImageMemoryBarrier::builder()
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
                        .build()
                    ]
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
                        .build()
                    ]
                );
            });
        }
        Ok(())
    }
}
