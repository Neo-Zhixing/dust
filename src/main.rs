use bevy::prelude::*;
use dust_new::Raytraced;
mod flycamera;
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
        .insert(Transform::from_xyz(0.0, 0.0, 0.0));
    commands
        .spawn()
        .insert(Raytraced {
            aabb_extent: bevy::math::Vec3::new(4.0, 4.0, 4.0),
        })
        .insert(watertank_handle)
        .insert(GlobalTransform::default())
        .insert(Transform::from_xyz(0.0, 20.0, 0.0));
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
