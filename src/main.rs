use std::str::FromStr;

use bevy::prelude::*;
use dust_new::Raytraced;
fn main() {
    App::new()
        .add_plugin(bevy::core::CorePlugin::default())
        .add_plugin(bevy::transform::TransformPlugin::default())
        .add_plugin(bevy::input::InputPlugin::default())
        .add_plugin(bevy::window::WindowPlugin::default())
        .add_plugin(bevy::winit::WinitPlugin::default())
        .add_plugin(bevy::asset::AssetPlugin::default())
        .add_plugin(dust_new::DustPlugin::default())
        .add_plugin(bevy::render2::RenderPlugin::default())
        .add_plugin(bevy::core_pipeline::CorePipelinePlugin::default())
        //.add_startup_system(setup)
        .run();
}

fn setup(mut commands: Commands) {
    commands
        .spawn()
        .insert(Raytraced {
            aabb_extent: bevy::math::Vec3::new(1.0, 2.0, 1.0),
        })
        .insert(GlobalTransform::default())
        .insert(Transform::default());
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
