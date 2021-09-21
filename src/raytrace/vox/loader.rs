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
            let model = &scene.models[0];
            let size = model.size.x.max(model.size.y).max(model.size.z);
            let size = crate::util::next_pow2_sqrt(size) as u8;
            let mut svdag = Svdag::new(self.block_allocator.clone(), 1);
            let mut grid = svdag.get_grid_accessor_mut(size, 0);

            for voxel in model.voxels.iter() {
                grid.set(voxel.x as u32, voxel.y as u32, voxel.z as u32, true);
            }

            svdag.flush_all();
            load_context.set_default_asset(LoadedAsset::new(VoxelModel { svdag }));
            Ok(())
        })
    }

    fn extensions(&self) -> &[&str] {
        &["vox"]
    }
}
