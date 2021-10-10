use super::Svdag;
use crate::raytrace::arena_alloc::Handle;

pub struct GridAccessor<'a> {
    pub(super) dag: &'a Svdag,
    pub(super) size: u8,
    pub(super) root_index: usize,
}

impl<'a> GridAccessor<'a> {
    pub fn get(&self, mut x: u32, mut y: u32, mut z: u32) -> bool {
        let root = self.dag.roots[self.root_index];
        if root.is_none() {
            return false;
        }
        let mut gridsize = 1 << self.size;
        let mut handle = root;
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
    pub(super) root_index: usize,
}

impl<'a> GridAccessorMut<'a> {
    pub fn get(&self, x: u32, y: u32, z: u32) -> bool {
        let accessor = GridAccessor {
            dag: self.dag,
            size: self.size,
            root_index: self.root_index,
        };
        accessor.get(x, y, z)
    }
    pub fn set(&mut self, x: u32, y: u32, z: u32, occupancy: bool) {
        let mut root = self.dag.roots[self.root_index];
        unsafe {
            self.set_recursive(&mut root, x, y, z, 1 << self.size, occupancy);
        }
        self.dag.roots[self.root_index] = root;
    }

    // Returns: avg
    // Base case: when gridsize = 2 and parent node is non-null, set the occupancy corner in the parent node.
    //            if this causes the parent to have uniform occupancy and no children, collapse the parent by deallocating it.
    // Induction step: for gridsize > 2 and parent node is non-null, call avg = self(gridsize / 2) and set the occupancy in the parent node.
    //                 if this causes the parent node to have uniform
    unsafe fn set_recursive(
        &mut self,
        handle: &mut Handle,
        mut x: u32,
        mut y: u32,
        mut z: u32,
        mut gridsize: u32,
        occupancy: bool,
    ) -> bool {
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
            if std::intrinsics::unlikely(handle.is_none()) {
                // This happens only when gridsize = 2.
                // TODO: set handle to be something.
                *handle = self.dag.arena.alloc(1);
                let header = &mut self.dag.arena.get_mut(*handle).header;
                header.child_mask = 0;
                header.occupancy_mask = 0;
            }
            let header = &mut self.dag.arena.get_mut(*handle).header;
            header.set_occupancy_at_corner_u8(corner, occupancy);
            if header.has_child_at_corner_u8(corner) {
                // has children. Cut them off.
                todo!()
            }
        } else {
            let mut new_handle = Handle::none();
            if !handle.is_none() {
                let header = &self.dag.arena.get(*handle).header;
                if header.has_child_at_corner_u8(corner) {
                    new_handle = header.child_at_corner_u8(corner).handle;
                }
            }
            let avg = self.set_recursive(&mut new_handle, x, y, z, gridsize, occupancy);

            if new_handle.is_none() {
                self.remove_children(handle, corner);
            } else {
                // children exists.
                // put new_handle into the parent node
                if handle.is_none() {
                    // Allocate new
                    *handle = self.dag.arena.alloc(2);
                    let header = &mut self.dag.arena.get_mut(*handle).header;
                    header.child_mask = 1 << corner;
                    header.occupancy_mask = 0;
                } else {
                    // Parent already exists.
                    self.insert_children(handle, corner);
                }
                self.dag
                    .arena
                    .get_mut(*handle)
                    .header
                    .child_at_corner_mut_u8(corner)
                    .handle = new_handle;
            }
            let header = &mut self.dag.arena.get_mut(*handle).header;
            if avg {
                header.occupancy_mask |= 1 << corner;
            } else {
                header.occupancy_mask &= !(1 << corner);
            }
        }

        let header = &mut self.dag.arena.get_mut(*handle).header;
        if header.child_mask == 0 {
            // node has no children
            // collapse node
            let occupancy_mask = header.occupancy_mask;
            if occupancy_mask == 0 || occupancy_mask == 0xFF {
                let block_size = header.child_mask.count_ones() as u8 + 1;
                self.dag.arena.free(*handle, block_size);
                *handle = Handle::none();
                return occupancy_mask == 0xFF;
            }
        }
        header.occupancy_mask != 0
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
        let occupancy_mask = old_slot.header.occupancy_mask;
        let old_mask = old_slot.header.child_mask;
        if old_mask == new_mask {
            return old_handle;
        }
        let old_slot_num_child = old_slot.header.child_mask.count_ones() as u8;

        let new_slot_num_child = new_mask.count_ones() as u8;
        let new_handle = self.dag.arena.alloc((new_slot_num_child + 1) as u32);
        let _new_slot = self.dag.arena.get(new_handle);

        let mut old_slot_num: u8 = 0;
        let mut new_slot_num: u8 = 0;
        for i in 0..8 {
            let old_have_children_at_i = old_mask & (1 << i) != 0;
            let new_have_children_at_i = new_mask & (1 << i) != 0;
            if old_have_children_at_i && new_have_children_at_i {
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
            if old_have_children_at_i {
                old_slot_num += 1;
            }
            if new_have_children_at_i {
                new_slot_num += 1;
            }
        }
        self.dag.arena.free(old_handle, old_slot_num_child + 1);

        let new_slot = self.dag.arena.get_mut(new_handle);
        new_slot.header.child_mask = new_mask;
        new_slot.header.occupancy_mask = occupancy_mask;

        new_handle
    }

    unsafe fn insert_children(&mut self, handle: &mut Handle, corner: u8) {
        let old_handle = *handle;
        let old_mask = self.dag.arena.get(old_handle).header.child_mask;
        let new_handle = self.reshape(old_handle, old_mask | (1 << corner));
        *handle = new_handle;
    }
    unsafe fn remove_children(&mut self, handle: &mut Handle, corner: u8) {
        let old_handle = *handle;
        let old_mask = self.dag.arena.get(old_handle).header.child_mask;
        let new_handle = self.reshape(old_handle, old_mask & !(1 << corner));
        *handle = new_handle;
    }
}

impl Svdag {
    // Access a certain frame of the DAG in a uniform grid of side length 2^size
    pub fn get_grid_accessor(&self, size: u8, frame: usize) -> GridAccessor {
        GridAccessor {
            dag: self,
            size,
            root_index: frame,
        }
    }
    pub fn get_grid_accessor_mut(&mut self, size: u8, frame: usize) -> GridAccessorMut {
        GridAccessorMut {
            dag: self,
            size,
            root_index: frame,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Svdag;

    #[test]
    fn test_set() {
        let mut dag = Svdag::potato();
        let mut grid = dag.get_grid_accessor_mut(2, 0);

        assert!(!grid.get(0, 0, 0));
        grid.set(0, 0, 0, true);
        assert!(grid.get(0, 0, 0));
        assert!(!grid.get(1, 0, 0));
        assert!(!grid.get(0, 1, 0));
        assert!(!grid.get(0, 1, 1));
        assert_eq!(grid.dag.arena.get_size(), 3); // Root node, and root node has one children.

        for x in 0..=1 {
            for y in 0..=1 {
                grid.set(x, y, 1, true);
            }
        }
        grid.set(0, 1, 0, true);
        grid.set(1, 0, 0, true);
        assert_eq!(grid.dag.arena.get_size(), 3);

        grid.set(3, 3, 3, true);
        assert!(grid.get(3, 3, 3));
        assert_eq!(grid.dag.arena.get_size(), 5);

        // Fill in the last peace before collapse
        assert!(!grid.get(1, 1, 0));
        grid.set(1, 1, 0, true);
        assert!(grid.get(1, 1, 0));
        assert_eq!(grid.dag.arena.get_size(), 3);
    }
}
