use bevy::{
    asset::{AssetLoader, LoadContext, LoadedAsset},
    prelude::*,
    utils::BoxedFuture,
};
use dot_vox::{DotVoxData, SceneNode};

use std::sync::Arc;

use super::VoxelModel;

use crate::raytrace::block_alloc::BlockAllocator;
use crate::raytrace::svdag::Svdag;

pub struct VoxLoader {
    block_allocator: Arc<dyn BlockAllocator>,
}

impl FromWorld for VoxLoader {
    fn from_world(world: &mut World) -> Self {
        let block_allocator = world
            .get_resource_mut::<Arc<dyn BlockAllocator>>()
            .unwrap()
            .clone();
        VoxLoader { block_allocator }
    }
}

impl AssetLoader for VoxLoader {
    fn load<'a>(
        &'a self,
        bytes: &'a [u8],
        load_context: &'a mut LoadContext,
    ) -> BoxedFuture<'a, Result<(), anyhow::Error>> {
        Box::pin(async move {
            let mut svdag = Svdag::new(self.block_allocator.clone(), 1);
            println!("started loading vox");
            let scene = dot_vox::load_bytes(bytes).map_err(|err| anyhow::Error::msg(err))?;
            println!("end loading vox");

            let _translation_min = Vec3 {
                x: i32::MAX,
                y: i32::MAX,
                z: i32::MAX,
            };
            let _translation_max = Vec3 {
                x: i32::MIN,
                y: i32::MIN,
                z: i32::MIN,
            };

            let mut translation_min = Vec3::MAX;
            let mut translation_max = Vec3::MIN;
            self.traverse(&scene, |model_id, translation, rotation| {
                let model = &scene.models[model_id as usize];
                let size: Vec3 = Vec3 {
                    x: model.size.x as i32,
                    y: model.size.y as i32,
                    z: model.size.z as i32,
                };
                let size = rotation * size;
                let halfsize = size / 2;
                translation_min.x = translation_min.x.min(translation.x - halfsize.x.abs());
                translation_min.y = translation_min.y.min(translation.y - halfsize.y.abs());
                translation_min.z = translation_min.z.min(translation.z - halfsize.z.abs());
                translation_max.x = translation_max.x.max(translation.x + halfsize.x.abs());
                translation_max.y = translation_max.y.max(translation.y + halfsize.y.abs());
                translation_max.z = translation_max.z.max(translation.z + halfsize.z.abs());
            });
            let scene_size = translation_max - translation_min;
            let scene_size = scene_size.x.max(scene_size.y).max(scene_size.z);
            let mut grid = svdag
                .get_grid_accessor_mut(crate::util::next_pow2_sqrt(scene_size as u32) as u8, 0);
            let offset = -translation_min;
            self.traverse(&scene, |model_id, translation, rotation| {
                /*

                */

                let model = &scene.models[model_id as usize];
                let half_size = Vec3 {
                    x: model.size.x as i32,
                    y: model.size.y as i32,
                    z: model.size.z as i32,
                } / 2;
                for voxel in model.voxels.iter() {
                    let local_position = Vec3 {
                        x: voxel.x as i32,
                        y: voxel.y as i32,
                        z: voxel.z as i32,
                    } - half_size;
                    let location =
                        translation + offset + (rotation * (local_position * 2 + Vec3::ONE)) / 2;
                    assert!(0 <= location.x && location.x < 2048);
                    assert!(0 <= location.y && location.y < 2048);
                    assert!(0 <= location.z && location.z < 2048);
                    grid.set(
                        location.x as u32,
                        location.z as u32,
                        location.y as u32,
                        true,
                    );
                }
            });
            svdag.flush_all();
            load_context.set_default_asset(LoadedAsset::new(VoxelModel { svdag }));
            Ok(())
        })
    }

    fn extensions(&self) -> &[&str] {
        &["vox"]
    }
}

impl VoxLoader {
    fn traverse<F>(&self, scene: &DotVoxData, mut callback: F)
    where
        F: FnMut(u32, Vec3, Rotation),
    {
        self.traverse_recursive(scene, 0, Vec3::ZERO, Rotation::IDENTITY, &mut callback)
    }
    fn traverse_recursive<F>(
        &self,
        scene: &DotVoxData,
        node: u32,
        mut translation: Vec3,
        mut rotation: Rotation,
        callback: &mut F,
    ) where
        F: FnMut(u32, Vec3, Rotation),
    {
        let node = &scene.scene[node as usize];
        match node {
            SceneNode::Transform {
                attributes: _,
                frames,
                child,
            } => {
                if frames.len() != 1 {
                    unimplemented!("Multiple frame in transform node");
                }
                let frame = &frames[0];
                if let Some(value) = frame.get("_t") {
                    let values: Vec<&str> = value.split(" ").collect();
                    assert_eq!(values.len(), 3);
                    translation += Vec3 {
                        x: values[0].parse::<i32>().unwrap(),
                        y: values[1].parse::<i32>().unwrap(),
                        z: values[2].parse::<i32>().unwrap(),
                    };
                }
                if let Some(value) = frame.get("_r") {
                    rotation = Rotation(value.parse::<u8>().unwrap());
                }

                self.traverse_recursive(scene, *child, translation, rotation, callback);
            }
            SceneNode::Group {
                attributes: _,
                children,
            } => {
                for &i in children {
                    self.traverse_recursive(scene, i, translation, rotation, callback);
                }
            }
            SceneNode::Shape {
                attributes: _,
                models,
            } => {
                // Shape nodes are leafs and correspond to models
                if models.len() != 1 {
                    unimplemented!("Multiple shape models in Shape node");
                }
                let model = &models[0];
                callback(model.model_id, translation, rotation);
            }
        }
    }
}

#[derive(Clone, Copy)]
struct Rotation(u8);
impl Rotation {
    const IDENTITY: Rotation = Rotation(0b0000100);
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct Vec3 {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}
impl Vec3 {
    const ZERO: Vec3 = Vec3 { x: 0, y: 0, z: 0 };
    const ONE: Vec3 = Vec3 { x: 1, y: 1, z: 1 };
    const MAX: Vec3 = Vec3 {
        x: i32::MAX,
        y: i32::MAX,
        z: i32::MAX,
    };
    const MIN: Vec3 = Vec3 {
        x: i32::MIN,
        y: i32::MIN,
        z: i32::MIN,
    };
    fn as_slice(&self) -> &[i32; 3] {
        unsafe { std::mem::transmute(self) }
    }
}
impl std::ops::Add<Vec3> for Vec3 {
    type Output = Vec3;

    fn add(self, rhs: Vec3) -> Vec3 {
        Vec3 {
            x: self.x + rhs.x,
            y: self.y + rhs.y,
            z: self.z + rhs.z,
        }
    }
}
impl std::ops::AddAssign<Vec3> for Vec3 {
    fn add_assign(&mut self, rhs: Vec3) {
        self.x += rhs.x;
        self.y += rhs.y;
        self.z += rhs.z;
    }
}
impl std::ops::Sub<Vec3> for Vec3 {
    type Output = Vec3;

    fn sub(self, rhs: Vec3) -> Vec3 {
        Vec3 {
            x: self.x - rhs.x,
            y: self.y - rhs.y,
            z: self.z - rhs.z,
        }
    }
}
impl std::ops::SubAssign<Vec3> for Vec3 {
    fn sub_assign(&mut self, rhs: Vec3) {
        self.x -= rhs.x;
        self.y -= rhs.y;
        self.z -= rhs.z;
    }
}
impl std::ops::Div<i32> for Vec3 {
    type Output = Vec3;
    fn div(self, rhs: i32) -> Vec3 {
        Vec3 {
            x: self.x.div_euclid(rhs),
            y: self.y.div_euclid(rhs),
            z: self.z.div_euclid(rhs),
        }
    }
}
impl std::ops::Mul<i32> for Vec3 {
    type Output = Vec3;
    fn mul(self, rhs: i32) -> Vec3 {
        Vec3 {
            x: self.x * rhs,
            y: self.y * rhs,
            z: self.z * rhs,
        }
    }
}
impl std::cmp::PartialEq for Vec3 {
    fn eq(&self, rhs: &Vec3) -> bool {
        self.x == rhs.x && self.y == rhs.y && self.z == rhs.z
    }
}

impl std::ops::Neg for Vec3 {
    type Output = Vec3;
    fn neg(self) -> Self::Output {
        Vec3 {
            x: -self.x,
            y: -self.y,
            z: -self.z,
        }
    }
}

impl std::ops::Mul<Vec3> for Rotation {
    type Output = Vec3;
    fn mul(self, rhs: Vec3) -> Vec3 {
        let index_nz1 = self.0 & 0b11;
        let index_nz2 = (self.0 >> 2) & 0b11;
        assert_ne!(index_nz1, index_nz2, "Invalid Rotation");
        let index_nz3 = 3 - index_nz1 - index_nz2;

        let row_1_sign = if self.0 & (1 << 4) == 0 { 1 } else { -1 };
        let row_2_sign = if self.0 & (1 << 5) == 0 { 1 } else { -1 };
        let row_3_sign = if self.0 & (1 << 6) == 0 { 1 } else { -1 };

        Vec3 {
            x: rhs.as_slice()[index_nz1 as usize] * row_1_sign,
            y: rhs.as_slice()[index_nz2 as usize] * row_2_sign,
            z: rhs.as_slice()[index_nz3 as usize] * row_3_sign,
        }
    }
}
