#![feature(maybe_uninit_uninit_array)]
#![feature(alloc_layout_extra)]
#![feature(untagged_unions)]
#![feature(allocator_api)]
#![feature(slice_ptr_get)]
#![feature(asm)]
#![feature(core_intrinsics)]
#![feature(const_cstr_unchecked)]
#![feature(adt_const_params)]

mod device_info;
mod queues;
mod raytrace;
mod render;
mod util;

pub use raytrace::VoxelModel;

use device_info::DeviceInfo;

use bevy::prelude::*;

pub type Allocator = gpu_alloc::GpuAllocator<ash::vk::DeviceMemory>;
pub type MemoryBlock = gpu_alloc::MemoryBlock<ash::vk::DeviceMemory>;
pub use queues::Queues;
pub use raytrace::Raytraced;

#[derive(Default)]
pub struct DustPlugin;

impl Plugin for DustPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugin(render::RenderPlugin::default())
            .add_plugin(raytrace::RaytracePlugin::default());
    }
}
