use std::str::FromStr;

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

use bevy::pbr2::{DirectionalLight, DirectionalLightBundle, PbrBundle, StandardMaterial};
use bevy::render2::color::Color;
use bevy::render2::mesh::shape;
use bevy::render2::mesh::Mesh;
fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.spawn_bundle(PbrBundle {
        mesh: meshes.add(Mesh::from(shape::Cube { size: 1.0 })),
        material: materials.add(StandardMaterial {
            base_color: Color::PINK,
            ..Default::default()
        }),
        transform: Transform::from_xyz(0.0, 0.0, 0.0),
        ..Default::default()
    });

    // directional 'sun' light
    const HALF_SIZE: f32 = 10.0;
    commands.spawn_bundle(DirectionalLightBundle {
        directional_light: DirectionalLight {
            // Configure the projection to better fit the scene
            shadow_projection: bevy::render2::camera::OrthographicProjection {
                left: -HALF_SIZE,
                right: HALF_SIZE,
                bottom: -HALF_SIZE,
                top: HALF_SIZE,
                near: -10.0 * HALF_SIZE,
                far: 10.0 * HALF_SIZE,
                ..Default::default()
            },
            ..Default::default()
        },
        transform: Transform {
            translation: Vec3::new(0.0, 2.0, 0.0),
            rotation: Quat::from_rotation_x(-std::f32::consts::FRAC_PI_4),
            ..Default::default()
        },
        ..Default::default()
    });

    // camera
    commands.spawn_bundle(bevy::render2::camera::PerspectiveCameraBundle {
        transform: Transform::from_xyz(-2.0, 2.5, 5.0).looking_at(Vec3::ZERO, Vec3::Y),
        ..Default::default()
    })
    .insert(flycamera::FlyCamera::default());

    commands
        .spawn()
        .insert(Raytraced {
            aabb_extent: bevy::math::Vec3::new(1.0, 1.0, 10.0),
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
