use ash::vk;

use super::{
    AllocError, AllocatorCreateInfo, BlockAllocation, BlockAllocator, BlockAllocatorAddressSpace,
};
use crossbeam::queue::SegQueue;
use std::ops::Range;
use std::sync::atomic::{AtomicU64, Ordering};

pub struct IntegratedBlockAllocator {
    device: ash::Device,
    bind_transfer_queue: vk::Queue,
    bind_transfer_queue_family: u32,
    graphics_queue_family: u32,
    buffer_size: u64,

    block_size: u64,
    memory_properties: vk::PhysicalDeviceMemoryProperties,
}

struct IntegratedAddressSpace {
    buffer: vk::Buffer,
    memtype: u32,
    current_offset: AtomicU64,
    free_offsets: SegQueue<u64>,
}

unsafe impl Send for IntegratedBlockAllocator {}
unsafe impl Sync for IntegratedBlockAllocator {}

impl IntegratedBlockAllocator {
    pub unsafe fn new(
        device: ash::Device,
        memory_properties: &vk::PhysicalDeviceMemoryProperties,
        create_info: &AllocatorCreateInfo,
    ) -> Self {
        Self {
            bind_transfer_queue: create_info.bind_transfer_queue,
            memory_properties: memory_properties.clone(),
            block_size: create_info.block_size,
            device,
            buffer_size: create_info.max_storage_buffer_size,
            bind_transfer_queue_family: create_info.bind_transfer_queue_family,
            graphics_queue_family: create_info.graphics_queue_family,
        }
    }
}

impl BlockAllocator for IntegratedBlockAllocator {
    unsafe fn create_address_space(&self) -> BlockAllocatorAddressSpace {
        let queue_family_indices = [self.graphics_queue_family, self.bind_transfer_queue_family];
        let mut buffer_create_info = vk::BufferCreateInfo::builder()
            .size(self.buffer_size)
            .usage(vk::BufferUsageFlags::STORAGE_BUFFER)
            .flags(vk::BufferCreateFlags::SPARSE_BINDING | vk::BufferCreateFlags::SPARSE_RESIDENCY);

        if self.graphics_queue_family == self.bind_transfer_queue_family {
            buffer_create_info = buffer_create_info.sharing_mode(vk::SharingMode::EXCLUSIVE);
        } else {
            buffer_create_info = buffer_create_info
                .sharing_mode(vk::SharingMode::CONCURRENT)
                .queue_family_indices(&queue_family_indices);
        }

        let device_buffer = self
            .device
            .create_buffer(&buffer_create_info.build(), None)
            .unwrap();
        let requirements = self.device.get_buffer_memory_requirements(device_buffer);
        let memtype = select_integrated_memtype(&self.memory_properties, &requirements);
        let address_space = Box::new(IntegratedAddressSpace {
            buffer: device_buffer,
            memtype,
            current_offset: AtomicU64::new(0),
            free_offsets: SegQueue::new(),
        });
        let ptr = Box::leak(address_space) as *mut _ as usize;
        BlockAllocatorAddressSpace(ptr)
    }
    unsafe fn destroy_address_space(&self, address_space: BlockAllocatorAddressSpace) {
        let address_space = Box::from_raw(address_space.0 as *mut IntegratedAddressSpace);
        self.device.destroy_buffer(address_space.buffer, None);
    }
    unsafe fn allocate_block(
        &self,
        address_space: &BlockAllocatorAddressSpace,
    ) -> Result<(*mut u8, BlockAllocation), AllocError> {
        let address_space = &*(address_space.0 as *const IntegratedAddressSpace);
        let resource_offset = address_space
            .free_offsets
            .pop()
            .unwrap_or_else(|| address_space.current_offset.fetch_add(1, Ordering::Relaxed));
        let mem = self
            .device
            .allocate_memory(
                &vk::MemoryAllocateInfo::builder()
                    .allocation_size(self.block_size)
                    .memory_type_index(address_space.memtype)
                    .build(),
                None,
            )
            .unwrap();
        let ptr = self
            .device
            .map_memory(mem, 0, vk::WHOLE_SIZE, vk::MemoryMapFlags::empty())
            .map_err(super::AllocError::from)? as *mut u8;
        self.device
            .queue_bind_sparse(
                self.bind_transfer_queue,
                &[vk::BindSparseInfo::builder()
                    .buffer_binds(&[vk::SparseBufferMemoryBindInfo::builder()
                        .buffer(address_space.buffer)
                        .binds(&[vk::SparseMemoryBind {
                            resource_offset: resource_offset * self.block_size as u64,
                            size: self.block_size,
                            memory: mem,
                            memory_offset: 0,
                            flags: vk::SparseMemoryBindFlags::empty(),
                        }])
                        .build()])
                    .build()],
                vk::Fence::null(),
            )
            .map_err(super::AllocError::from)?;
        let allocation = BlockAllocation(std::mem::transmute(mem));
        Ok((ptr, allocation))
    }

    unsafe fn deallocate_block(
        &self,
        address_space: &BlockAllocatorAddressSpace,
        block: BlockAllocation,
    ) {
        let memory: vk::DeviceMemory = std::mem::transmute(block);
        self.device.free_memory(memory, None);
    }

    unsafe fn flush(
        &self,
        ranges: &mut dyn Iterator<
            Item = (&BlockAllocatorAddressSpace, &BlockAllocation, Range<u32>),
        >,
    ) {
        // TODO: only do this for non-coherent memory
        self.device
            .flush_mapped_memory_ranges(
                &ranges
                    .map(|(address_space, allocation, range)| {
                        let memory: vk::DeviceMemory = std::mem::transmute(allocation.0);
                        vk::MappedMemoryRange::builder()
                            .memory(memory)
                            .offset(range.start as u64)
                            .size((range.end - range.start) as u64)
                            .build()
                    })
                    .collect::<Vec<_>>(),
            )
            .unwrap();
    }
    fn can_flush(&self) -> bool {
        true
    }
    fn get_blocksize(&self) -> u64 {
        self.block_size
    }
    fn get_buffer(&self, address_space: &BlockAllocatorAddressSpace) -> vk::Buffer {
        let address_space = unsafe { &*(address_space.0 as *const IntegratedAddressSpace) };
        address_space.buffer
    }
    fn get_device_buffer_size(&self) -> u64 {
        self.buffer_size
    }
    fn get_buffer_device_address(
        &self,
        address_space: &BlockAllocatorAddressSpace,
    ) -> vk::DeviceAddress {
        let address_space: &IntegratedAddressSpace =
            unsafe { &*(address_space.0 as *const IntegratedAddressSpace) };
        unsafe {
            self.device.get_buffer_device_address(
                &vk::BufferDeviceAddressInfo::builder()
                    .buffer(address_space.buffer)
                    .build(),
            )
        }
    }
}

fn select_integrated_memtype(
    memory_properties: &vk::PhysicalDeviceMemoryProperties,
    requirements: &vk::MemoryRequirements,
) -> u32 {
    let heaps = &memory_properties.memory_heaps[0..memory_properties.memory_heap_count as usize];

    // Select a heap.
    // For AMD iGPUs, this selects the heap without DEVICE_LOCAL because DEVICE_LOCAL heaps are small and slow for CPU access.
    // For Intel iGPUs, this selects the only heap.
    let heap = heaps
        .iter()
        .enumerate()
        .find(|(_, &heap)| !heap.flags.contains(vk::MemoryHeapFlags::DEVICE_LOCAL))
        .map_or(0, |(i, _)| i) as u32;

    let types = &memory_properties.memory_types[0..memory_properties.memory_type_count as usize];
    let selected_index = types
        .iter()
        .enumerate()
        .position(|(id, memory_type)| {
            requirements.memory_type_bits & (1 << id) != 0
                && memory_type.heap_index == heap
                && memory_type.property_flags.contains(
                    vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_CACHED,
                )
        })
        .or_else(|| {
            types.iter().enumerate().position(|(id, memory_type)| {
                requirements.memory_type_bits & (1 << id) != 0
                    && memory_type.heap_index == heap
                    && memory_type.property_flags.contains(
                        vk::MemoryPropertyFlags::DEVICE_LOCAL
                            | vk::MemoryPropertyFlags::HOST_VISIBLE,
                    )
            })
        })
        .unwrap() as u32;
    let selected_index = 3_u32;
    selected_index
}
