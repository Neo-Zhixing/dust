use bevy::ecs::reflect::ReflectComponent;

use bevy::reflect::Reflect;

#[derive(Debug, Clone, Reflect)]
#[reflect(Component)]
pub struct PerspectiveCamera {
    pub fov: f32,
    pub near: f32,
    pub far: f32,
}

impl Default for PerspectiveCamera {
    fn default() -> Self {
        Self {
            fov: std::f32::consts::PI / 4.0,
            near: 0.1,
            far: 1000.0,
        }
    }
}
