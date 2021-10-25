#![feature(maybe_uninit_uninit_array)]
#![feature(alloc_layout_extra)]
#![feature(untagged_unions)]
#![feature(allocator_api)]
#![feature(slice_ptr_get)]
#![feature(asm)]
#![feature(core_intrinsics)]
#![feature(const_cstr_unchecked)]
#![feature(adt_const_params)]
#![feature(inline_const)]

mod camera;
mod device_info;
mod queues;
mod raytrace;
mod render;
mod util;
use ash::vk;
pub use camera::PerspectiveCamera;

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
        app.add_plugin(raytrace::RaytracePlugin::default());
    }
}

// TODO: move to its own file.
// TODO: Change all to use this.
pub trait VulkanAllocator {
    unsafe fn alloc_for_buffer(
        &mut self,
        device: &ash::Device,
        buffer: vk::Buffer,
        usage: gpu_alloc::UsageFlags,
    ) -> MemoryBlock;
    unsafe fn alloc_with_device(
        &mut self,
        device: &ash::Device,
        request: gpu_alloc::Request,
    ) -> MemoryBlock;
}
impl VulkanAllocator for Allocator {
    unsafe fn alloc_for_buffer(
        &mut self,
        device: &ash::Device,
        buffer: vk::Buffer,
        usage: gpu_alloc::UsageFlags,
    ) -> MemoryBlock {
        use gpu_alloc::Request;
        use gpu_alloc_ash::AshMemoryDevice;
        let requirements = device.get_buffer_memory_requirements(buffer);
        let mem = self
            .alloc(
                AshMemoryDevice::wrap(device),
                Request {
                    size: requirements.size,
                    align_mask: requirements.alignment,
                    memory_types: requirements.memory_type_bits,
                    usage,
                },
            )
            .unwrap();
        device
            .bind_buffer_memory(buffer, *mem.memory(), mem.offset())
            .unwrap();
        mem
    }
    unsafe fn alloc_with_device(
        &mut self,
        device: &ash::Device,
        request: gpu_alloc::Request,
    ) -> MemoryBlock {
        use gpu_alloc_ash::AshMemoryDevice;
        self.alloc(AshMemoryDevice::wrap(device), request).unwrap()
    }
}
