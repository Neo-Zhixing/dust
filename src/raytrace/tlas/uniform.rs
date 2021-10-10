use ash::vk;
use bevy::core::{Pod, Zeroable};
use crevice::std140::{AsStd140, Std140, Std140Padded};
use gpu_alloc::MemoryBlock;
use gpu_alloc_ash::AshMemoryDevice;
use std::ffi::c_void;

#[derive(Clone, Copy, Debug)]
pub struct DeviceAddress(pub u64);

#[repr(C)]
#[derive(AsStd140, Clone)]
pub struct UniformEntry {
    pub device: DeviceAddress,
    pub parent: u32,
}

unsafe impl Std140 for DeviceAddress {
    const ALIGNMENT: usize = 8;
    type Padded = Std140Padded<Self, 8>;
}

unsafe impl Zeroable for DeviceAddress {}
unsafe impl Pod for DeviceAddress {}

pub struct UniformArray {
    staging_buf: vk::Buffer,
    device_buf: vk::Buffer,
    staging_mem: Option<MemoryBlock<vk::DeviceMemory>>,
    device_mem: Option<MemoryBlock<vk::DeviceMemory>>,
    staging_ptr: *mut c_void,
    capacity: u32,
}
unsafe impl Send for UniformArray {}
unsafe impl Sync for UniformArray {}

impl UniformArray {
    pub fn new() -> UniformArray {
        UniformArray {
            staging_buf: vk::Buffer::null(),
            device_buf: vk::Buffer::null(),
            staging_mem: None,
            device_mem: None,
            capacity: 0,
            staging_ptr: std::ptr::null_mut(),
        }
    }
    unsafe fn resize_and_clear(
        &mut self,
        new_capacity: u32,
        device: &ash::Device,
        allocator: &mut crate::Allocator,
    ) {
        // Clean up the old ones
        device.device_wait_idle().unwrap();
        if self.staging_buf != vk::Buffer::null() {
            device.destroy_buffer(self.staging_buf, None);
            self.staging_buf = vk::Buffer::null();
        }
        if self.device_buf != vk::Buffer::null() {
            device.destroy_buffer(self.device_buf, None);
            self.device_buf = vk::Buffer::null();
        }
        if let Some(staging_mem) = self.staging_mem.take() {
            allocator.dealloc(AshMemoryDevice::wrap(device), staging_mem);
        }
        if let Some(device_mem) = self.device_mem.take() {
            allocator.dealloc(AshMemoryDevice::wrap(device), device_mem);
        }

        let size = UniformEntry::std140_size_static();
        let array_size = size as u64 * new_capacity as u64;

        let staging_buf = device
            .create_buffer(
                &vk::BufferCreateInfo::builder()
                    .size(array_size)
                    .usage(vk::BufferUsageFlags::TRANSFER_SRC)
                    .sharing_mode(vk::SharingMode::EXCLUSIVE)
                    .build(),
                None,
            )
            .unwrap();
        let staging_buf_requirement = device.get_buffer_memory_requirements(staging_buf);
        let mut staging_mem = allocator
            .alloc(
                AshMemoryDevice::wrap(&device),
                gpu_alloc::Request {
                    size: staging_buf_requirement.size,
                    align_mask: staging_buf_requirement.alignment,
                    usage: gpu_alloc::UsageFlags::UPLOAD,
                    memory_types: staging_buf_requirement.memory_type_bits,
                },
            )
            .unwrap();

        let device_buf = device
            .create_buffer(
                &vk::BufferCreateInfo::builder()
                    .size(array_size)
                    .usage(
                        vk::BufferUsageFlags::TRANSFER_DST | vk::BufferUsageFlags::STORAGE_BUFFER,
                    )
                    .sharing_mode(vk::SharingMode::EXCLUSIVE)
                    .build(),
                None,
            )
            .unwrap();
        let device_buf_requirement = device.get_buffer_memory_requirements(device_buf);
        let device_mem = allocator
            .alloc(
                AshMemoryDevice::wrap(&device),
                gpu_alloc::Request {
                    size: device_buf_requirement.size,
                    align_mask: device_buf_requirement.alignment,
                    usage: gpu_alloc::UsageFlags::FAST_DEVICE_ACCESS,
                    memory_types: device_buf_requirement.memory_type_bits,
                },
            )
            .unwrap();

        // Map memory
        device
            .bind_buffer_memory(device_buf, *device_mem.memory(), device_mem.offset())
            .unwrap();
        device
            .bind_buffer_memory(staging_buf, *staging_mem.memory(), staging_mem.offset())
            .unwrap();
        self.staging_ptr = staging_mem
            .map(AshMemoryDevice::wrap(device), 0, array_size as usize)
            .unwrap()
            .as_ptr() as *mut c_void;

        self.staging_mem = Some(staging_mem);
        self.staging_buf = staging_buf;
        self.device_mem = Some(device_mem);
        self.device_buf = device_buf;
        self.capacity = new_capacity;
    }
    pub unsafe fn write(
        &mut self,
        items: impl ExactSizeIterator<Item = UniformEntry>,
        device: &ash::Device,
        allocator: &mut crate::Allocator,
    ) {
        if items.len() == 0 {
            return;
        }
        if items.len() as u32 > self.capacity {
            self.resize_and_clear(items.len() as u32, device, allocator);
        }
        let entry_size = UniformEntry::std140_size_static();
        let mut dst = self.staging_ptr as *mut u8;
        for entry in items {
            std::ptr::copy_nonoverlapping(
                &entry.as_std140() as *const _ as *const u8,
                dst,
                entry_size,
            );
            dst = dst.add(entry_size);
        }
    }
    pub fn get_buffer(&self) -> vk::Buffer {
        self.device_buf
    }
    pub fn get_staging_buffer(&self) -> vk::Buffer {
        self.staging_buf
    }
    pub fn get_full_size(&self) -> u64 {
        self.capacity as u64 * UniformEntry::std140_size_static() as u64
    }
}
