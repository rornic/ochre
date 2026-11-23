//! What the headless examples share: the terrain systems on `MinimalPlugins`, with asset types registered by hand.
use bevy::prelude::*;
use ochre_engine::settings::{DebugSettings, Settings};
use ochre_engine::DevEnv;
use ochre_terrain::material::{ChunkMaterial, SprayMaterial, WaterMaterial};
use ochre_terrain::{task_pool_plugin, TerrainPlugin};

/// An app running `TerrainPlugin` over the world `dev` describes, with a Startup system left for the caller to add.
pub fn headless_app(settings: &Settings, dev: &DevEnv) -> App {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins.set(task_pool_plugin()),
        AssetPlugin::default(),
        TerrainPlugin::new(
            ochre_engine::load_world(settings, &dev.world),
            settings.renderer,
            DebugSettings::default(),
        ),
    ))
    .init_asset::<Mesh>()
    .init_asset::<Image>()
    .init_asset::<ChunkMaterial>()
    .init_asset::<WaterMaterial>()
    .init_asset::<SprayMaterial>();
    app
}
