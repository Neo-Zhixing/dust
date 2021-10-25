use bevy::ecs::reflect::ReflectComponent;
use bevy::math::Mat4;
use bevy::reflect::{Reflect, ReflectDeserialize};

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
