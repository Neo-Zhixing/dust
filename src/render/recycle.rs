use ash::vk;
use bevy::ecs::prelude::*;

use super::{window::NUM_FRAMES_IN_FLIGHT, RenderState};

pub enum Garbage {
    AccelerationStructure(vk::AccelerationStructureKHR),
}

/// Rendering resources that you put into the Garbage Bin won't be deleted immediately.
/// They will be deleted exactly NUM_FRAMES_IN_FLIGHT frames later.
/// Doing so ensures that all frames using the resource are finished,
/// and that the resource is no longer in use.
pub struct GarbageBin {
    garbage: [Vec<Garbage>; NUM_FRAMES_IN_FLIGHT as usize],
    current_frame_garbage: Vec<Garbage>,
}

impl GarbageBin {
    pub fn new() -> Self {
        GarbageBin {
            current_frame_garbage: Vec::new(),
            garbage: [const { Vec::<Garbage>::new() }; NUM_FRAMES_IN_FLIGHT as usize],
        }
    }

    pub fn collect(&mut self, garbage: Garbage) {
        self.current_frame_garbage.push(garbage);
    }
}

// To be added to CLEANUP system.
pub(super) fn garbage_collection_system(
    mut garbage_bin: ResMut<GarbageBin>,
    render_state: Res<RenderState>,

    acceleration_structure_loader: Res<ash::extensions::khr::AccelerationStructure>,
) {
    let garbage_bin = &mut *garbage_bin;
    let current_frame_index = render_state.current_frame().index;
    let last_frame_garbage = &mut garbage_bin.garbage[current_frame_index as usize];
    for garbage in last_frame_garbage.drain(..) {
        unsafe {
            match garbage {
                Garbage::AccelerationStructure(acceleration_structure) => {
                    println!("Deleted as");
                    acceleration_structure_loader
                        .destroy_acceleration_structure(acceleration_structure, None);
                }
            }
        }
    }
    let current_frame_garbage = &mut garbage_bin.current_frame_garbage;
    std::mem::swap(last_frame_garbage, current_frame_garbage);
}
