mod grid;

use std::sync::Arc;

use super::arena_alloc::{ArenaAllocated, ArenaAllocator, Handle};
use super::block_alloc::BlockAllocator;

fn mask_location_nth_one(mask: u8, location: u8) -> u8 {
    (mask & ((1 << location) - 1)).count_ones() as u8
}
struct Header {
    child_mask: u8,
    occupancy_mask: u8,
}
impl Header {
    #[inline]
    pub fn has_child_at_corner_u8(&self, corner: u8) -> bool {
        self.child_mask & (1 << corner) != 0
    }

    #[inline]
    pub fn occupancy_at_corner_u8(&self, corner: u8) -> bool {
        self.occupancy_mask & (1 << corner) != 0
    }

    #[inline]
    pub unsafe fn child_at_corner_u8(&self, corner: u8) -> &Body {
        // Given a mask and a location, returns n where the given '1' on the location
        // is the nth '1' counting from the least significant bit.

        let ptr = self as *const Self as *const Slot;
        let body_slot_ptr =
            ptr.add(1 + mask_location_nth_one(self.child_mask, corner as u8) as usize);
        let body_slot = &*body_slot_ptr;
        &body_slot.body
    }

    #[inline]
    pub unsafe fn child_at_corner_mut_u8(&mut self, corner: u8) -> &mut Body {
        // Given a mask and a location, returns n where the given '1' on the location
        // is the nth '1' counting from the least significant bit.

        let ptr = self as *mut Self as *mut Slot;
        let body_slot_ptr =
            ptr.add(1 + mask_location_nth_one(self.child_mask, corner as u8) as usize);
        let body_slot = &mut *body_slot_ptr;
        &mut body_slot.body
    }

    #[inline]
    pub fn set_occupancy_at_corner_u8(&mut self, corner: u8, occupied: bool) {
        if occupied {
            self.occupancy_mask |= 1 << corner;
        } else {
            self.occupancy_mask &= 1 << corner;
        }
    }
}

struct Body {
    handle: Handle,
}
pub union Slot {
    header: Header,
    body: Body,
}
impl Default for Slot {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}
impl ArenaAllocated for Slot {}

pub struct Svdag {
    arena: ArenaAllocator<Slot>,
    roots: Vec<Handle>,
}

impl Svdag {
    pub fn new(block_allocator: Arc<dyn BlockAllocator>, num_roots: u32) -> Self {
        let arena: ArenaAllocator<Slot> = ArenaAllocator::new(block_allocator);
        Svdag {
            arena,
            roots: vec![Handle::none(); num_roots as usize],
        }
    }
    #[cfg(test)]
    pub fn potato() -> Self {
        let block_allocator = Arc::new(super::block_alloc::SystemBlockAllocator::new(
            super::arena_alloc::BLOCK_SIZE as usize,
        ));

        Self::new(block_allocator, 1)
    }

    pub fn flush_all(&self) {
        self.arena.flush_all();
    }

    pub fn get_roots(&self) -> &[Handle] {
        &self.roots
    }
}
