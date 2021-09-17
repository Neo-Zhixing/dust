mod grid;

use std::default;

use self::grid::GridAccessor;

use super::arena_alloc::{ArenaAllocated, ArenaAllocator, Handle};

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
        fn mask_location_nth_one(mask: u8, location: u8) -> u8 {
            (mask & ((1 << location) - 1)).count_ones() as u8
        }

        let ptr = self as *const Self as *const Slot;
        let body_slot_ptr = ptr.add(mask_location_nth_one(self.child_mask, corner as u8) as usize);
        let body_slot = &*body_slot_ptr;
        &body_slot.body
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
union Slot {
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
    // Access a certain frame of the DAG in a uniform grid of side length 2^size
    pub fn get_grid_accessor(&self, size: u8, frame: usize) -> GridAccessor {
        GridAccessor {
            dag: self,
            size,
            root: self.roots[frame],
        }
    }
}
