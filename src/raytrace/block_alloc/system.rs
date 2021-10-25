use super::{AllocError, BlockAllocation, BlockAllocator};
use std::alloc::{Allocator, Global, Layout};
use std::ops::Range;
use std::ptr::NonNull;

pub struct SystemBlockAllocator<A: Allocator = Global> {
    allocator: A,
    block_size: usize,
}

impl SystemBlockAllocator {
    pub fn new(block_size: usize) -> SystemBlockAllocator<Global> {
        SystemBlockAllocator {
            allocator: Global,
            block_size,
        }
    }
}

impl BlockAllocator for SystemBlockAllocator {
    unsafe fn create_address_space(&self) -> super::BlockAllocatorAddressSpace {
        super::BlockAllocatorAddressSpace(0)
    }
    unsafe fn destroy_address_space(&self, _address_space: super::BlockAllocatorAddressSpace) {}
    unsafe fn allocate_block(
        &self,
        _address_space: &super::BlockAllocatorAddressSpace,
    ) -> Result<(*mut u8, BlockAllocation), AllocError> {
        let mem = self
            .allocator
            .allocate(Layout::from_size_align_unchecked(self.block_size, 1))
            .map_err(|_| AllocError::OutOfHostMemory)?;
        let ptr = mem.as_mut_ptr();
        Ok((mem.as_mut_ptr(), BlockAllocation(ptr as u64)))
    }

    unsafe fn deallocate_block(
        &self,
        _address_space: &super::BlockAllocatorAddressSpace,
        block: BlockAllocation,
    ) {
        let _layout = Layout::new::<u8>().repeat(self.block_size).unwrap();
        self.allocator.deallocate(
            NonNull::new(block.0 as *mut u8).unwrap(),
            Layout::from_size_align_unchecked(self.block_size, 1),
        );
        std::mem::forget(block);
    }

    unsafe fn flush(
        &self,
        _ranges: &mut dyn Iterator<
            Item = (
                &super::BlockAllocatorAddressSpace,
                &BlockAllocation,
                Range<u32>,
            ),
        >,
    ) {
    }

    fn can_flush(&self) -> bool {
        true
    }
    fn get_blocksize(&self) -> u64 {
        self.block_size as u64
    }
    fn get_buffer(&self, _address_space: &super::BlockAllocatorAddressSpace) -> ash::vk::Buffer {
        unimplemented!()
    }
    fn get_device_buffer_size(&self) -> u64 {
        unimplemented!()
    }
    fn get_buffer_device_address(
        &self,
        _address_space: &super::BlockAllocatorAddressSpace,
    ) -> ash::vk::DeviceAddress {
        unimplemented!()
    }
}
