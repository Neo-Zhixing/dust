use bevy::{
    asset::{AssetLoader, LoadContext, LoadedAsset},
    prelude::*,
    reflect::TypeUuid,
    utils::BoxedFuture,
};

use std::sync::Arc;

use super::VoxelModel;
use crate::raytrace::arena_alloc::ArenaAllocator;
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
            println!("started loading vox");
            let scene = dot_vox::load_bytes(bytes).map_err(|err| anyhow::Error::msg(err))?;
            println!("end loading vox");

            for (i, model) in scene.models.iter().enumerate() {
                let svdag = Svdag::new(self.block_allocator.clone(), 1);
                let mut min: (u8, u8, u8) = (u8::MAX, u8::MAX, u8::MAX);
                let mut max: (u8, u8, u8) = (0, 0, 0);
                for v in model.voxels.iter() {
                    min.0 = min.0.min(v.x);
                    min.1 = min.1.min(v.y);
                    min.2 = min.2.min(v.z);
                    max.0 = max.0.max(v.x);
                    max.1 = max.1.max(v.y);
                    max.2 = max.2.max(v.z);
                }
                if model.size.x != max.0 as u32 - min.0 as u32 + 1 {
                    println!("Failed for {}", i);
                }
                if model.size.y != max.1 as u32 - min.1 as u32 + 1 {
                    println!("Failed for {}", i);
                }
                if model.size.z != max.2 as u32 - min.2 as u32 + 1 {
                    println!("Failed for {}", i);
                }
                println!("{} / {}", i, scene.models.len());

                load_context.set_labeled_asset(
                    &format!("model {}", i),
                    LoadedAsset::new(VoxelModel { svdag }),
                );
            }
            println!("All done");

            Ok(())
        })
    }

    fn extensions(&self) -> &[&str] {
        &["vox"]
    }
}
