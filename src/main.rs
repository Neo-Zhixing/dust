use bevy::prelude::*;
use dust_new::PerspectiveCamera;
use dust_new::Raytraced;
mod flycamera;
use bevy::{
    input::{keyboard::KeyCode, Input},
};



fn main() {
    App::new()
        .add_plugin(bevy::core::CorePlugin::default())
        .add_plugin(bevy::transform::TransformPlugin::default())
        .add_plugin(bevy::input::InputPlugin::default())
        .add_plugin(bevy::window::WindowPlugin::default())
        .add_plugin(bevy::asset::AssetPlugin::default())
        .add_plugin(dust_new::DustPlugin::default())
        .add_plugin(bevy::winit::WinitPlugin::default())
        .add_plugin(flycamera::FlyCameraPlugin)
        .add_startup_system(setup)
        .add_system(watertank_move_system)
        .run();
}
fn setup(mut commands: Commands, asset_server: Res<AssetServer>) {
    let scene_handle: Handle<dust_new::VoxelModel> = asset_server.load("castle.vox");
    let watertank_handle: Handle<dust_new::VoxelModel> = asset_server.load("water_tank.vox");

    // directional 'sun' light
    const HALF_SIZE: f32 = 10.0;
    // camera

    commands
        .spawn()
        .insert(Raytraced {
            aabb_extent: bevy::math::Vec3::new(128.0, 128.0, 128.0),
        })
        .insert(scene_handle)
        .insert(GlobalTransform::default())
        .insert(Transform::from_xyz(10.0, 10.0, 10.0));
    commands
        .spawn()
        .insert(Raytraced {
            aabb_extent: bevy::math::Vec3::new(4.0, 4.0, 4.0),
        })
        .insert(watertank_handle)
        .insert(GlobalTransform::default())
        .insert(Watertank)
        .insert(Transform::from_xyz(10.0, 15.0, 10.0));

    let mut transform = Transform::from_xyz(64.0, 64.0, 64.0);
    transform.look_at(Vec3::new(128.0, 128.0, 128.0), Vec3::Y);
    commands
        .spawn()
        .insert(transform)
        .insert(GlobalTransform::default())
        .insert(PerspectiveCamera::default())
        .insert(flycamera::FlyCamera::default());
    /*
    commands
        .spawn()
        .insert(Raytraced {
            aabb_extent: bevy::math::Vec3::new(1.0, 1.0, 1.0),
        })
        .insert(GlobalTransform::default())
        .insert(Transform::from_xyz(1.0, 2.0, 3.0));
        */
}

struct Watertank;
fn watertank_move_system(
    time: Res<Time>,
    mut query: Query<(&mut Transform), With<Watertank>>,
    keyboard_input: Res<Input<KeyCode>>
) {
    if keyboard_input.pressed(KeyCode::J) {
        for mut entity in query.iter_mut() {
            entity.translation.y = time.time_since_startup().as_secs_f32().cos() * 10.0 + 20.0;
        }
    }

}
