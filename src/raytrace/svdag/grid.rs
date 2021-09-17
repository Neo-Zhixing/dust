use super::Svdag;
use crate::raytrace::arena_alloc::Handle;

pub struct GridAccessor<'a> {
    pub(super) dag: &'a Svdag,
    pub(super) size: u8,
    pub(super) root: Handle,
}

impl<'a> GridAccessor<'a> {
    pub fn get(&self, mut x: u32, mut y: u32, mut z: u32) -> bool {
        let mut gridsize = 1 << self.size;
        let mut handle = self.root;
        while gridsize > 2 {
            gridsize = gridsize / 2;
            let mut corner: u8 = 0;
            if x >= gridsize {
                corner |= 0b100;
                x -= gridsize;
            }
            if y >= gridsize {
                corner |= 0b010;
                y -= gridsize;
            }
            if z >= gridsize {
                corner |= 0b001;
                z -= gridsize;
            }
            let header = unsafe {
                let slot = self.dag.arena.get(handle);
                // Since we hold a handle to the slot, the slot must be a header.
                &slot.header
            };
            if !header.has_child_at_corner_u8(corner) {
                return header.occupancy_at_corner_u8(corner);
            }
            unsafe {
                handle = header.child_at_corner_u8(corner).handle;
            }
        }
        // gridsize is now equal to 2
        debug_assert_eq!(gridsize, 2);
        let mut corner: u8 = 0;
        if x >= 1 {
            corner |= 0b100;
        }
        if y >= 1 {
            corner |= 0b010;
        }
        if z >= 1 {
            corner |= 0b001;
        }
        unsafe {
            self.dag
                .arena
                .get(handle)
                .header
                .occupancy_at_corner_u8(corner)
        }
    }
}

pub struct GridAccessorMut<'a> {
    pub(super) dag: &'a mut Svdag,
    pub(super) size: u8,
    pub(super) root: Handle,
}

impl<'a> GridAccessorMut<'a> {
    pub fn get(&self, mut x: u32, mut y: u32, mut z: u32) -> bool {
        let accessor = GridAccessor {
            dag: self.dag,
            size: self.size,
            root: self.root,
        };
        accessor.get(x, y, z)
    }
    pub fn set(&mut self, x: u32, y: u32, z: u32, occupancy: bool) {
        self.set_recursive(self.root, x, y, z, 1 << self.size, occupancy);
    }

    // Returns: (avg, collapse)
    fn set_recursive(
        &mut self,
        mut handle: Handle,
        mut x: u32,
        mut y: u32,
        mut z: u32,
        mut gridsize: u32,
        occupancy: bool,
    ) -> (bool, bool) {
        gridsize = gridsize / 2;
        let mut corner: u8 = 0;
        if x >= gridsize {
            corner |= 0b100;
            x -= gridsize;
        }
        if y >= gridsize {
            corner |= 0b010;
            y -= gridsize;
        }
        if z >= gridsize {
            corner |= 0b001;
            z -= gridsize;
        }
        if gridsize <= 1 {
            // is leaf node
            let header = unsafe { &mut self.dag.arena.get_mut(handle).header };
            header.set_occupancy_at_corner_u8(corner, occupancy);
            if header.has_child_at_corner_u8(corner) {
                // has children. Cut them off.
                todo!()
            }
        } else {
            let mut header = unsafe { &mut self.dag.arena.get_mut(handle).header };
            if !header.has_child_at_corner_u8(corner) {
                // no children
                // create a new node at that location
                unsafe {
                    let new_mask = header.child_mask | (1 << corner);
                    handle = self.reshape(handle, new_mask);
                    header = &mut self.dag.arena.get_mut(handle).header
                }
            }

            let new_handle = unsafe { header.child_at_corner_u8(corner).handle };
            let (avg, collapsed) = self.set_recursive(new_handle, x, y, z, gridsize, occupancy);

            let mut header = unsafe { &mut self.dag.arena.get_mut(handle).header };
            header.set_occupancy_at_corner_u8(corner, avg);
            if collapsed {
                unsafe {
                    let new_mask = header.child_mask & !(1 << corner);
                    handle = self.reshape(handle, new_mask);
                    header = &mut self.dag.arena.get_mut(handle).header
                }
            }
        }

        let header = unsafe { &self.dag.arena.get_mut(handle).header };
        if header.child_mask == 0 {
            // node has no children
            // collapse node
            if header.occupancy_mask == 0xFF {
                return (true, true);
            } else if header.occupancy_mask == 0 {
                return (false, true);
            }
        }
        let avg = header.occupancy_mask.count_ones() >= 4;
        return (avg, false);
    }

    // Change the childmask of the node located at node_handle
    // while attempt to preserve the child nodes.
    // Specifically, for 0 <= n < 8,
    // - If old.has_child_at_corner_u8(n) and new.has_child_at_corner_u8(n), the content will be copied over
    // - If old.has_child_at_corner_u8(n) and !new.has_child_at_corner_u8(n), the old node will be freed
    // - If !old.has_child_at_corner_u8(n) and new.has_child_at_corner_u8(n), space will be reserved for the new node
    // - Otherwise, nothing happens.
    // TODO: make sure the freeing is recursive.
    unsafe fn reshape(&mut self, old_handle: Handle, new_mask: u8) -> Handle {
        let old_slot = self.dag.arena.get(old_handle);
        let old_mask = old_slot.header.child_mask;
        if old_mask == new_mask {
            return old_handle;
        }
        let old_slot_num_child = old_slot.header.child_mask.count_ones() as u8;

        let new_slot_num_child = new_mask.count_ones() as u8;
        let new_handle = self.dag.arena.alloc((new_slot_num_child + 1) as u32);
        let new_slot = self.dag.arena.get(new_handle);

        let mut old_slot_num: u8 = 0;
        let mut new_slot_num: u8 = 0;
        for i in 0..8 {
            let old_have_children_at_i = old_mask & (1 << i) != 0;
            let new_have_children_at_i = new_mask & (1 << i) != 0;
            if old_have_children_at_i && new_have_children_at_i {
                unsafe {
                    std::ptr::copy(
                        &self
                            .dag
                            .arena
                            .get(old_handle.offset((old_slot_num + 1) as u32))
                            .body,
                        &mut self
                            .dag
                            .arena
                            .get_mut(new_handle.offset((new_slot_num + 1) as u32))
                            .body,
                        1,
                    );
                }
            }
            if old_have_children_at_i {
                old_slot_num += 1;
            }
            if new_have_children_at_i {
                new_slot_num += 1;
            }
        }
        self.dag.arena.free(old_handle, old_slot_num_child + 1);
        new_handle
    }
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
    pub fn get_grid_accessor_mut(&mut self, size: u8, frame: usize) -> GridAccessorMut {
        let root = self.roots[frame];
        GridAccessorMut {
            dag: self,
            size,
            root,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Svdag;

    fn test_set() {
        let mut dag = Svdag::potato();
        let mut grid = dag.get_grid_accessor_mut(16, 0);

        assert!(!grid.get(0, 0, 0));
        grid.set(0, 0, 0, true);
        assert!(grid.get(0, 0, 0));
        assert_eq!(grid.dag.arena.get_size(), 3);
    }
}
