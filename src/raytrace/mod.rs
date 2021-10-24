mod arena_alloc;
mod block_alloc;
mod commands;
mod ray_shaders;
mod sbt;
mod svdag;
mod tlas;
mod vox;

use ash::vk;

pub use tlas::Raytraced;
pub use vox::VoxelModel;

use crate::render::{RenderApp, RenderStage};
use bevy::prelude::*;

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
        //render_app.add_system_to_stage(RenderStage::Prepare, update_desc_sets);

        self.add_block_allocator(app);
        app.add_plugin(vox::VoxPlugin::default());

        app.sub_app(RenderApp)
            .init_resource::<ray_shaders::RayShaders>()
            .add_system_to_stage(
                RenderStage::Queue,
                commands::record_raytracing_commands_system,
            );
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

// This system makes sure that RayShaders.target_img_desc_set always points to the correct acceleration structure and buffer
/*
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
*/
