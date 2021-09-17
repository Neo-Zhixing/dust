use super::block_alloc::{BlockAllocation, BlockAllocator};

use std::mem::{size_of, ManuallyDrop};

use std::ptr::NonNull;
use std::sync::Arc;

pub const BLOCK_MASK_DEGREE: u32 = 20;
pub const NUM_SLOTS_IN_BLOCK: u32 = 1 << BLOCK_MASK_DEGREE;
pub const BLOCK_SIZE: u64 = NUM_SLOTS_IN_BLOCK as u64 * 24;
pub const BLOCK_MASK: u32 = NUM_SLOTS_IN_BLOCK - 1;

#[derive(Copy, Clone, Debug, Ord, PartialOrd, Eq, PartialEq)]
pub struct Handle(u32);
impl Handle {
    #[inline]
    pub const fn none() -> Self {
        Handle(u32::MAX)
    }
    #[inline]
    pub fn is_none(&self) -> bool {
        self.0 == u32::MAX
    }
    #[inline]
    pub fn offset(&self, n: u32) -> Self {
        Handle(self.0 + n)
    }
    #[inline]
    pub fn get_slot_num(&self) -> u32 {
        self.0 & BLOCK_MASK
    }
    #[inline]
    pub fn get_chunk_num(&self) -> u32 {
        self.0 >> BLOCK_MASK_DEGREE
    }
    #[inline]
    pub fn from_index(chunk_index: u32, block_index: u32) -> Handle {
        Handle(chunk_index << BLOCK_MASK_DEGREE | block_index)
    }
}

impl Default for Handle {
    fn default() -> Self {
        Handle::none()
    }
}

pub type ArenaBlockAllocator = dyn BlockAllocator;

#[repr(C)]
struct FreeSlot {
    next: Handle, // 32 bits
}

union ArenaSlot<T: ArenaAllocated> {
    occupied: ManuallyDrop<T>,
    free: FreeSlot,
}

pub trait ArenaAllocated: Sized + Default {}

pub struct ArenaAllocator<T: ArenaAllocated> {
    block_allocator: Arc<ArenaBlockAllocator>,
    chunks: Vec<(NonNull<ArenaSlot<T>>, BlockAllocation)>,
    freelist_heads: [Handle; 9],
    newspace_top: Handle, // new space to be allocated
    size: u32,            // number of allocated slots
    num_segments: u32,    // number of allocated segments
    num_blocks: u32,      // number of blocks allocated from block_allocator
}

// ArenaAllocator contains NunNull which makes it !Send and !Sync.
// NonNull is !Send and !Sync because the data they reference may be aliased.
// Here we guarantee that NonNull will never be aliased.
// Therefore ArenaAllocator should be Send and Sync.
unsafe impl<T: ArenaAllocated> Send for ArenaAllocator<T> {}
unsafe impl<T: ArenaAllocated> Sync for ArenaAllocator<T> {}

impl<T: ArenaAllocated> ArenaAllocator<T> {
    pub fn new(block_allocator: Arc<ArenaBlockAllocator>) -> Self {
        debug_assert!(size_of::<T>() >= size_of::<FreeSlot>(),);
        Self {
            block_allocator,
            chunks: vec![],
            freelist_heads: [Handle::none(); 9],
            // Space pointed by this is guaranteed to have free space > 8
            newspace_top: Handle::none(),
            size: 0,
            num_segments: 0,
            num_blocks: 0,
        }
    }
    #[cfg(test)]
    pub fn potato() -> Self {
        use super::block_alloc::SystemBlockAllocator;
        Self {
            block_allocator: Arc::new(SystemBlockAllocator::new(
                NUM_SLOTS_IN_BLOCK as usize * std::mem::size_of::<ArenaSlot<T>>(),
            )),
            chunks: vec![],
            freelist_heads: [Handle::none(); 9],
            // Space pointed by this is guaranteed to have free space > 8
            newspace_top: Handle::none(),
            size: 0,
            num_segments: 0,
            num_blocks: 0,
        }
    }

    unsafe fn alloc_block(&mut self) -> Handle {
        let chunk_index = self.chunks.len() as u32;
        let (chunk, allocation) = self.block_allocator.allocate_block().unwrap();
        self.chunks
            .push((NonNull::new_unchecked(chunk as _), allocation));
        self.num_blocks += 1;
        Handle::from_index(chunk_index, 0)
    }
    pub unsafe fn alloc(&mut self, len: u32) -> Handle {
        assert!(0 < len && len <= 9, "Only supports block size between 1-8!");
        self.size += len;
        self.num_segments += 1;

        // Retrieve the head of the freelist
        let sized_head = self.freelist_pop(len as u8);
        let handle: Handle = if sized_head.is_none() {
            // If the head is none, it means we need to allocate some new slots
            if self.newspace_top.is_none() {
                // We've run out of newspace.
                // Allocate a new memory chunk from the underlying block allocator.
                let alloc_head = self.alloc_block();
                self.newspace_top = Handle::from_index(alloc_head.get_chunk_num(), len);
                alloc_head
            } else {
                // There's still space remains to be allocated in the current chunk.
                let handle = self.newspace_top;
                let slot_index = handle.get_slot_num();
                let chunk_index = handle.get_chunk_num();
                let remaining_space = NUM_SLOTS_IN_BLOCK - slot_index - len;

                let new_handle = Handle::from_index(chunk_index, slot_index + len);
                if remaining_space > 9 {
                    self.newspace_top = new_handle;
                } else {
                    if remaining_space > 0 {
                        self.freelist_push(remaining_space as u8, new_handle);
                    }
                    self.newspace_top = Handle::none();
                }
                handle
            }
        } else {
            // There's previously used blocks stored in the freelist. Use them first.
            sized_head
        };

        // initialize to zero
        let slot_index = handle.get_slot_num();
        let chunk_index = handle.get_chunk_num();
        unsafe {
            let base = self.chunks[chunk_index as usize]
                .0
                .as_ptr()
                .add(slot_index as usize);
            for i in 0..len {
                let i = &mut *base.add(i as usize);
                i.occupied = Default::default();
            }
        }
        handle
    }
    pub unsafe fn free(&mut self, handle: Handle, block_size: u8) {
        debug_assert!(0 < block_size && block_size <= 9);
        self.freelist_push(block_size, handle);
        self.size -= block_size as u32;
        self.num_segments -= 1;
    }
    unsafe fn freelist_push(&mut self, n: u8, handle: Handle) {
        debug_assert!(0 < n && n <= 9);
        self.get_slot_mut(handle).free.next = self.freelist_heads[(n - 1) as usize];
        self.freelist_heads[(n - 1) as usize] = handle;
    }
    unsafe fn freelist_pop(&mut self, n: u8) -> Handle {
        debug_assert!(0 < n && n <= 9);
        let sized_head = self.freelist_heads[(n - 1) as usize];
        if !sized_head.is_none() {
            self.freelist_heads[(n - 1) as usize] = self.get_slot(sized_head).free.next;
        }
        sized_head
    }
    #[inline]
    unsafe fn get_slot(&self, handle: Handle) -> &ArenaSlot<T> {
        let slot_index = handle.get_slot_num();
        let chunk_index = handle.get_chunk_num();
        unsafe {
            let base = self.chunks[chunk_index as usize].0.as_ptr();
            &*base.add(slot_index as usize)
        }
    }
    #[inline]
    unsafe fn get_slot_mut(&mut self, handle: Handle) -> &mut ArenaSlot<T> {
        let slot_index = handle.get_slot_num();
        let chunk_index = handle.get_chunk_num();
        unsafe {
            let base = self.chunks[chunk_index as usize].0.as_ptr();
            &mut *base.add(slot_index as usize)
        }
    }
    #[inline]
    pub fn get(&self, index: Handle) -> &T {
        unsafe {
            let slot = self.get_slot(index);
            &slot.occupied
        }
    }
    #[inline]
    pub fn get_mut(&mut self, index: Handle) -> &mut T {
        unsafe {
            let slot = self.get_slot_mut(index);
            &mut slot.occupied
        }
    }

    #[inline]
    pub fn get_size(&self) -> u32 {
        self.size
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::mem::size_of;

    impl ArenaAllocated for u128 {}

    #[test]
    fn test_alloc() {
        let mut arena: ArenaAllocator<u128> = ArenaAllocator::potato();
        unsafe {
            // Allocate until we have 9 slots left
            for i in 0..NUM_SLOTS_IN_BLOCK - 9 {
                let handle = arena.alloc(1);
                assert_eq!(handle.get_slot_num(), i);
                assert_eq!(handle.get_chunk_num(), 0);
            }
            // At this point there shouldn't be any extra allocations
            assert_eq!(arena.num_blocks, 1);

            // Allocate one more
            let handle = arena.alloc(1);

            // This new slot should be in a new chunk
            assert_eq!(handle.get_slot_num(), 0);
            assert_eq!(handle.get_chunk_num(), 1);
            // A new chunk was allocated
            assert_eq!(arena.num_blocks, 2);

            // The remaining 9 slot was put into the freelist
            let handle = arena.alloc(9);
            assert_eq!(handle.get_slot_num(), NUM_SLOTS_IN_BLOCK - 9);
            assert_eq!(handle.get_chunk_num(), 0);
        }
    }

    #[test]
    fn test_free() {
        let mut arena: ArenaAllocator<u128> = ArenaAllocator::potato();
        unsafe {
            let handles: Vec<Handle> = (0..8).map(|_| arena.alloc(4)).collect();
            for handle in handles.iter().rev() {
                unsafe { arena.free(*handle, 4) };
            }
            assert_eq!(arena.alloc(1), Handle(8 * 4));
            for handle in handles.iter() {
                let new_handle = arena.alloc(4);
                assert_eq!(*handle, new_handle);
            }
        }
    }
}
