mod arena_alloc;
mod block_alloc;
mod commands;
mod ray_shaders;
mod sbt;
mod svdag;
mod tlas;
mod uniform;
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
use crate::{PerspectiveCamera, Queues};
pub(crate) use uniform::RaytracingNodeViewConstants;

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
        app.add_plugin(crate::render::RenderPlugin {
            uniform_size: std::mem::size_of::<RaytracingNodeViewConstants>() as u64,
        });

        let render_app = app.sub_app(RenderApp);
        render_app.add_plugin(tlas::TlasPlugin::default());
        //render_app.add_system_to_stage(RenderStage::Prepare, update_desc_sets);

        self.add_block_allocator(app);
        app.add_plugin(vox::VoxPlugin::default());

        app.sub_app(RenderApp)
            .add_system_to_stage(RenderStage::Extract, uniform::extract_uniform_data)
            .add_system_to_stage(RenderStage::Prepare, uniform::prepare_uniform_data)
            .init_resource::<ray_shaders::RayShaders>()
            .add_system_to_stage(
                RenderStage::Queue,
                commands::record_raytracing_commands_system,
            );
    }
}
