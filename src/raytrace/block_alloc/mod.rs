mod discrete;
mod integrated;

#[cfg(test)]
mod system;

pub use discrete::DiscreteBlockAllocator;
pub use integrated::IntegratedBlockAllocator;

#[cfg(test)]
pub use system::SystemBlockAllocator;

use ash::vk;
use std::ops::Range;

#[derive(Debug)]
pub enum AllocError {
    OutOfHostMemory,
    OutOfDeviceMemory,
    MappingFailed,
    TooManyObjects,
}

impl From<vk::Result> for AllocError {
    fn from(result: vk::Result) -> AllocError {
        match result {
            vk::Result::ERROR_OUT_OF_DEVICE_MEMORY => AllocError::OutOfDeviceMemory,
            vk::Result::ERROR_OUT_OF_HOST_MEMORY => AllocError::OutOfHostMemory,
            vk::Result::ERROR_MEMORY_MAP_FAILED => AllocError::MappingFailed,
            vk::Result::ERROR_TOO_MANY_OBJECTS => AllocError::TooManyObjects,
            _ => panic!("{:?}", result),
        }
    }
}

pub struct BlockAllocation(pub u64);

pub trait BlockAllocator: Send + Sync {
    // Allocate a block. Returns the host pointer to the block, and an allocation token which needs to be returned.
    unsafe fn allocate_block(&self) -> Result<(*mut u8, BlockAllocation), AllocError>;
    unsafe fn deallocate_block(&self, block: BlockAllocation);

    // Flush all host writes to the device.
    unsafe fn flush(&self, ranges: &mut dyn Iterator<Item = (&BlockAllocation, Range<u32>)>);

    // Returns false if the async copy is still busy.
    fn can_flush(&self) -> bool;

    fn get_blocksize(&self) -> u64;
}

pub struct AllocatorCreateInfo {
    pub bind_transfer_queue: vk::Queue,
    pub bind_transfer_queue_family: u32,
    pub graphics_queue_family: u32,
    pub block_size: u64,
    pub max_storage_buffer_size: u64,
}
