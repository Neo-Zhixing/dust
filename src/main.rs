

use bevy::prelude::*;

fn main() {
    App::new()
        .add_plugin(bevy::core::CorePlugin::default())
        .add_plugin(bevy::transform::TransformPlugin::default())
        .add_plugin(bevy::input::InputPlugin::default())
        .add_plugin(bevy::window::WindowPlugin::default())
        .add_plugin(bevy::winit::WinitPlugin::default())
        .add_plugin(dust_new::DustPlugin::default())
        .run();
}
