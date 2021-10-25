use crate::render::RenderState;
use bevy::ecs::prelude::*;
use bevy::math::{Mat3, Vec3};
use bevy::prelude::GlobalTransform;

use super::PerspectiveCamera;

pub(crate) struct RaytracingNodeViewConstants {
    pub camera_view_col0: [f32; 3],
    pub padding0: f32,
    pub camera_view_col1: [f32; 3],
    pub padding1: f32,
    pub camera_view_col2: [f32; 3],
    pub padding2: f32,

    pub camera_position: Vec3,
    pub tan_half_fov: f32,
}

pub(super) fn extract_uniform_data(
    mut render_world: ResMut<crate::render::RenderWorld>,
    query: Query<(&PerspectiveCamera, &GlobalTransform)>,
) {
    let mut cameras = query.iter();
    let (camera, transform) = cameras.next().expect("Requires at least one camera");
    if cameras.next().is_some() {
        unimplemented!("Supports at most one camera for now");
    }

    let view_constants = unsafe {
        let rotation_matrix = Mat3::from_quat(transform.rotation).to_cols_array_2d();
        let mut contants: RaytracingNodeViewConstants =
            std::mem::MaybeUninit::uninit().assume_init();
        contants.camera_view_col0 = rotation_matrix[0];
        contants.camera_view_col1 = rotation_matrix[1];
        contants.camera_view_col2 = rotation_matrix[2];
        contants.camera_position = transform.translation;
        contants.tan_half_fov = (camera.fov / 2.0).tan(); // TODO
        contants
    };
    render_world.insert_resource(view_constants);
}

pub(super) fn prepare_uniform_data(
    view_constants: Res<RaytracingNodeViewConstants>,
    render_state: Res<RenderState>,
) {
    let current_frame = render_state.current_frame().clone();
    let view_constants = &*view_constants;
    unsafe {
        std::ptr::copy_nonoverlapping(
            view_constants as *const RaytracingNodeViewConstants as *const u8,
            current_frame.uniform_buffer_ptr as *mut u8,
            std::mem::size_of_val(view_constants),
        );
    }
}
