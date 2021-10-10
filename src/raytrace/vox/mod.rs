use super::svdag::Svdag;

mod loader;

use bevy::app::App;
use bevy::prelude::AddAsset;
use bevy::reflect::TypeUuid;

#[derive(TypeUuid)]
#[uuid = "a6fbaf37-f393-4d5e-92ba-4b0944f7c9cf"]
pub struct VoxelModel {
    pub svdag: Svdag,
}

#[derive(Default)]
pub struct VoxPlugin;

impl bevy::app::Plugin for VoxPlugin {
    fn build(&self, app: &mut App) {
        app.init_asset_loader::<loader::VoxLoader>()
            .add_asset::<VoxelModel>();
    }
}
