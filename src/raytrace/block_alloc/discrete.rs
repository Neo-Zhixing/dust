use super::{
    AllocError, AllocatorCreateInfo, BlockAllocation, BlockAllocator, BlockAllocatorAddressSpace,
};
use ash::vk;
use crossbeam::queue::SegQueue;
use std::ops::Range;
use std::sync::atomic::{AtomicU16, AtomicU64, Ordering};
use std::sync::Arc;

pub struct DiscreteBlock {
    system_mem: vk::DeviceMemory,
    system_buf: vk::Buffer,
    device_mem: vk::DeviceMemory,
    offset: u64,
}

struct DiscreteAddressSpace {
    device_buffer: vk::Buffer,
    current_offset: AtomicU64,
    free_offsets: SegQueue<u64>,
    device_memtype: u32,
}

pub struct DiscreteBlockAllocator {
    device: ash::Device,
    block_size: u64,
    bind_transfer_queue: vk::Queue,
    bind_transfer_queue_family: u32,
    graphics_queue_family: u32,
    device_buffer_size: u64,

    command_pool: vk::CommandPool,
    command_buffer: vk::CommandBuffer,
    copy_completion_fence: vk::Fence,
    memory_properties: vk::PhysicalDeviceMemoryProperties,
}
unsafe impl Send for DiscreteBlockAllocator {}
unsafe impl Sync for DiscreteBlockAllocator {}

impl DiscreteBlockAllocator {
    pub unsafe fn new(
        device: ash::Device,
        memory_properties: &vk::PhysicalDeviceMemoryProperties,
        create_info: &AllocatorCreateInfo,
    ) -> Self {
        let command_pool = device
            .create_command_pool(
                &vk::CommandPoolCreateInfo::builder()
                    .flags(vk::CommandPoolCreateFlags::TRANSIENT)
                    .queue_family_index(create_info.bind_transfer_queue_family)
                    .build(),
                None,
            )
            .unwrap();
        let mut command_buffer = vk::CommandBuffer::null();
        device
            .fp_v1_0()
            .allocate_command_buffers(
                device.handle(),
                &vk::CommandBufferAllocateInfo::builder()
                    .command_pool(command_pool)
                    .command_buffer_count(1)
                    .level(vk::CommandBufferLevel::PRIMARY)
                    .build() as *const vk::CommandBufferAllocateInfo,
                &mut command_buffer as *mut vk::CommandBuffer,
            )
            .result()
            .unwrap();
        let copy_completion_fence = device
            .create_fence(
                &vk::FenceCreateInfo::builder()
                    .flags(vk::FenceCreateFlags::SIGNALED)
                    .build(),
                None,
            )
            .unwrap();

        let device_buffer_size =
            (create_info.max_storage_buffer_size / create_info.block_size) * create_info.block_size;
        Self {
            block_size: create_info.block_size,
            bind_transfer_queue: create_info.bind_transfer_queue,
            bind_transfer_queue_family: create_info.bind_transfer_queue_family,
            graphics_queue_family: create_info.graphics_queue_family,
            command_pool,
            command_buffer,
            copy_completion_fence,
            memory_properties: memory_properties.clone(),
            device,
            device_buffer_size,
        }
    }
}

impl BlockAllocator for DiscreteBlockAllocator {
    unsafe fn create_address_space(&self) -> BlockAllocatorAddressSpace {
        let mut buffer_create_info = vk::BufferCreateInfo::builder()
            .size(self.device_buffer_size)
            .usage(vk::BufferUsageFlags::STORAGE_BUFFER | vk::BufferUsageFlags::TRANSFER_DST)
            .flags(vk::BufferCreateFlags::SPARSE_BINDING | vk::BufferCreateFlags::SPARSE_RESIDENCY);

        let queue_family_indices = [self.graphics_queue_family, self.bind_transfer_queue_family];
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
        let device_buf_requirements = self.device.get_buffer_memory_requirements(device_buffer);
        let device_memtype =
            select_device_memtype(&self.memory_properties, &device_buf_requirements);
        let address_space = Box::new(DiscreteAddressSpace {
            device_buffer,
            current_offset: AtomicU64::new(0),
            free_offsets: SegQueue::new(),
            device_memtype,
        });
        unsafe {
            let ptr = Box::leak(address_space) as *mut DiscreteAddressSpace as usize;
            BlockAllocatorAddressSpace(ptr)
        }
    }
    unsafe fn destroy_address_space(&self, address_space: BlockAllocatorAddressSpace) {
        let ptr = address_space.0;
        std::mem::forget(address_space);
        let address_space = Box::from_raw(ptr as *mut DiscreteAddressSpace);
        self.device
            .destroy_buffer(address_space.device_buffer, None);
    }
    unsafe fn allocate_block(
        &self,
        address_space: &BlockAllocatorAddressSpace,
    ) -> Result<(*mut u8, BlockAllocation), AllocError> {
        let address_space: &DiscreteAddressSpace =
            &*(address_space.0 as *const DiscreteAddressSpace);
        let resource_offset = address_space
            .free_offsets
            .pop()
            .unwrap_or_else(|| address_space.current_offset.fetch_add(1, Ordering::Relaxed));

        let system_buf = self
            .device
            .create_buffer(
                &vk::BufferCreateInfo::builder()
                    .size(self.block_size)
                    .usage(vk::BufferUsageFlags::TRANSFER_SRC)
                    .sharing_mode(vk::SharingMode::EXCLUSIVE)
                    .build(),
                None,
            )
            .unwrap();

        let system_buf_requirements = self.device.get_buffer_memory_requirements(system_buf);
        let system_memtype =
            select_system_memtype(&self.memory_properties, &system_buf_requirements);
        let system_mem = self
            .device
            .allocate_memory(
                &vk::MemoryAllocateInfo::builder()
                    .memory_type_index(system_memtype)
                    .allocation_size(self.block_size)
                    .build(),
                None,
            )
            .map_err(super::AllocError::from)?;
        self.device
            .bind_buffer_memory(system_buf, system_mem, 0)
            .unwrap();
        let ptr = self
            .device
            .map_memory(system_mem, 0, vk::WHOLE_SIZE, vk::MemoryMapFlags::empty())
            .map_err(super::AllocError::from)? as *mut u8;

        let device_mem = self
            .device
            .allocate_memory(
                &vk::MemoryAllocateInfo::builder()
                    .memory_type_index(address_space.device_memtype)
                    .allocation_size(self.block_size)
                    .build(),
                None,
            )
            .map_err(super::AllocError::from)?;

        // Immediately submit the request
        let fence = self
            .device
            .create_fence(&vk::FenceCreateInfo::default(), None)
            .unwrap();
        self.device
            .queue_bind_sparse(
                self.bind_transfer_queue,
                &[vk::BindSparseInfo::builder()
                    .buffer_binds(&[vk::SparseBufferMemoryBindInfo::builder()
                        .buffer(address_space.device_buffer)
                        .binds(&[vk::SparseMemoryBind {
                            resource_offset: resource_offset * self.block_size as u64,
                            size: self.block_size,
                            memory: device_mem,
                            memory_offset: 0,
                            flags: vk::SparseMemoryBindFlags::empty(),
                        }])
                        .build()])
                    .build()],
                fence,
            )
            .map_err(super::AllocError::from)
            .unwrap();
        println!("Sparse Binded");
        self.device
            .wait_for_fences(&[fence], true, u64::MAX)
            .unwrap();
        self.device.destroy_fence(fence, None);
        let block = DiscreteBlock {
            system_mem,
            device_mem,
            system_buf,
            offset: resource_offset,
        };
        let block = Box::new(block);
        let allocation = BlockAllocation(Box::into_raw(block) as u64);
        Ok((ptr, allocation))
    }

    unsafe fn deallocate_block(
        &self,
        address_space: &BlockAllocatorAddressSpace,
        allocation: BlockAllocation,
    ) {
        let address_space: &DiscreteAddressSpace =
            &*(address_space.0 as *const DiscreteAddressSpace);
        let block = allocation.0 as *mut DiscreteBlock;
        let block = Box::from_raw(block);

        self.device.destroy_buffer(block.system_buf, None);
        self.device.free_memory(block.system_mem, None);
        self.device.free_memory(block.device_mem, None);
        address_space.free_offsets.push(block.offset);
        std::mem::forget(allocation);
    }

    unsafe fn flush(
        &self,
        ranges: &mut dyn Iterator<
            Item = (&BlockAllocatorAddressSpace, &BlockAllocation, Range<u32>),
        >,
    ) {
        self.device
            .reset_command_pool(self.command_pool, vk::CommandPoolResetFlags::empty())
            .unwrap();
        self.device
            .begin_command_buffer(
                self.command_buffer,
                &vk::CommandBufferBeginInfo::builder()
                    .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT)
                    .build(),
            )
            .unwrap();

        for (address_space, block_allocation, range) in ranges {
            // TODO: revisit for effiency improvements.
            let block = block_allocation.0 as *const DiscreteBlock;
            let block = &*block;
            let location = block.offset * self.block_size as u64 + range.start as u64;
            let address_space: &DiscreteAddressSpace =
                &*(address_space.0 as *const DiscreteAddressSpace);

            self.device.cmd_copy_buffer(
                self.command_buffer,
                block.system_buf,
                address_space.device_buffer,
                &[vk::BufferCopy {
                    src_offset: range.start as u64,
                    dst_offset: location,
                    size: (range.end - range.start) as u64,
                }],
            );
        }
        self.device.end_command_buffer(self.command_buffer).unwrap();

        self.device
            .reset_fences(&[self.copy_completion_fence])
            .unwrap();
        let command_buffers = [self.command_buffer];
        let submit_info = vk::SubmitInfo::builder()
            .command_buffers(&command_buffers)
            .build();
        self.device
            .queue_submit(
                self.bind_transfer_queue,
                &[submit_info],
                self.copy_completion_fence,
            )
            .unwrap();
    }
    fn can_flush(&self) -> bool {
        // If the previous copy hasn't completed: simply signal that we're busy at the moment.
        // The changes are going to be submitted to the queue in the next frame.
        let copy_completed = unsafe {
            self.device
                .get_fence_status(self.copy_completion_fence)
                .unwrap()
        };

        // Note that it's ok to have a copy command and a sparse binding command
        // in the queue at the same time. The copy command won't reference the newly
        // allocated memory ranges.
        copy_completed
    }
    fn get_blocksize(&self) -> u64 {
        self.block_size
    }
    fn get_device_buffer_size(&self) -> u64 {
        self.device_buffer_size
    }
    fn get_buffer(&self, address_space: &BlockAllocatorAddressSpace) -> vk::Buffer {
        let address_space: &DiscreteAddressSpace =
            unsafe { &*(address_space.0 as *const DiscreteAddressSpace) };
        address_space.device_buffer
    }
    fn get_buffer_device_address(
        &self,
        address_space: &BlockAllocatorAddressSpace,
    ) -> vk::DeviceAddress {
        let address_space: &DiscreteAddressSpace =
            unsafe { &*(address_space.0 as *const DiscreteAddressSpace) };
        unsafe {
            self.device.get_buffer_device_address(
                &vk::BufferDeviceAddressInfo::builder()
                    .buffer(address_space.device_buffer)
                    .build(),
            )
        }
    }
}

/// Returns SystemMemId, DeviceMemId
fn select_system_memtype(
    memory_properties: &vk::PhysicalDeviceMemoryProperties,
    system_buf_requirements: &vk::MemoryRequirements,
) -> u32 {
    memory_properties.memory_types[0..memory_properties.memory_type_count as usize]
        .iter()
        .enumerate()
        .position(|(id, memory_type)| {
            system_buf_requirements.memory_type_bits & (1 << id) != 0
                && memory_type.property_flags.contains(
                    vk::MemoryPropertyFlags::HOST_VISIBLE
                        | vk::MemoryPropertyFlags::HOST_COHERENT
                        | vk::MemoryPropertyFlags::HOST_CACHED,
                )
                && !memory_type
                    .property_flags
                    .contains(vk::MemoryPropertyFlags::DEVICE_LOCAL)
        })
        .unwrap() as u32
}

fn select_device_memtype(
    memory_properties: &vk::PhysicalDeviceMemoryProperties,
    device_buf_requirements: &vk::MemoryRequirements,
) -> u32 {
    let (device_heap_index, _device_heap) = memory_properties.memory_heaps
        [0..memory_properties.memory_heap_count as usize]
        .iter()
        .enumerate()
        .filter(|(_, heap)| heap.flags.contains(vk::MemoryHeapFlags::DEVICE_LOCAL))
        .max_by_key(|(_, heap)| heap.size)
        .unwrap();
    let device_heap_index = device_heap_index as u32;

    let (id, _memory_type) = memory_properties.memory_types
        [0..memory_properties.memory_type_count as usize]
        .iter()
        .enumerate()
        .find(|(id, memory_type)| {
            device_buf_requirements.memory_type_bits & (1 << id) != 0
                && memory_type
                    .property_flags
                    .contains(vk::MemoryPropertyFlags::DEVICE_LOCAL)
                && memory_type.heap_index == device_heap_index
        })
        .unwrap();
    id as u32
}
