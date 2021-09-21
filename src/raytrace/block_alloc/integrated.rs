use ash::vk;

use super::{AllocError, AllocatorCreateInfo, BlockAllocation, BlockAllocator};
use crossbeam::queue::SegQueue;
use std::ops::Range;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

pub struct IntegratedBlockAllocator {
    device: ash::Device,
    bind_transfer_queue: vk::Queue,
    memtype: u32,
    pub buffer: vk::Buffer,

    current_offset: AtomicU64,
    free_offsets: SegQueue<u64>,
    block_size: u64,
}
unsafe impl Send for IntegratedBlockAllocator {}
unsafe impl Sync for IntegratedBlockAllocator {}

impl IntegratedBlockAllocator {
    pub unsafe fn new(
        device: ash::Device,
        memory_properties: &vk::PhysicalDeviceMemoryProperties,
        create_info: &AllocatorCreateInfo,
    ) -> Self {
        let queue_family_indices = [
            create_info.graphics_queue_family,
            create_info.bind_transfer_queue_family,
        ];
        let mut buffer_create_info = vk::BufferCreateInfo::builder()
            .size(create_info.max_storage_buffer_size)
            .usage(vk::BufferUsageFlags::STORAGE_BUFFER)
            .flags(vk::BufferCreateFlags::SPARSE_BINDING | vk::BufferCreateFlags::SPARSE_RESIDENCY);

        if create_info.graphics_queue_family == create_info.bind_transfer_queue_family {
            buffer_create_info = buffer_create_info.sharing_mode(vk::SharingMode::EXCLUSIVE);
        } else {
            buffer_create_info = buffer_create_info
                .sharing_mode(vk::SharingMode::CONCURRENT)
                .queue_family_indices(&queue_family_indices);
        }

        let device_buffer = device
            .create_buffer(&buffer_create_info.build(), None)
            .unwrap();
        let requirements = device.get_buffer_memory_requirements(device_buffer);
        let memtype = select_integrated_memtype(memory_properties, &requirements);
        Self {
            bind_transfer_queue: create_info.bind_transfer_queue,
            memtype,
            buffer: device_buffer,
            current_offset: AtomicU64::new(0),
            free_offsets: SegQueue::new(),
            block_size: create_info.block_size,
            device,
        }
    }
}

impl BlockAllocator for IntegratedBlockAllocator {
    unsafe fn allocate_block(&self) -> Result<(*mut u8, BlockAllocation), AllocError> {
        let resource_offset = self
            .free_offsets
            .pop()
            .unwrap_or_else(|| self.current_offset.fetch_add(1, Ordering::Relaxed));
        let mem = self
            .device
            .allocate_memory(
                &vk::MemoryAllocateInfo::builder()
                    .allocation_size(self.block_size)
                    .memory_type_index(self.memtype)
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
                        .buffer(self.buffer)
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

    unsafe fn deallocate_block(&self, block: BlockAllocation) {
        let memory: vk::DeviceMemory = std::mem::transmute(block);
        self.device.free_memory(memory, None);
    }

    unsafe fn flush(&self, ranges: &mut dyn Iterator<Item = (&BlockAllocation, Range<u32>)>) {
        // TODO: only do this for non-coherent memory
        self.device
            .flush_mapped_memory_ranges(
                &ranges
                    .map(|(allocation, range)| {
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
    fn get_buffer(&self) -> vk::Buffer {
        self.buffer
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
