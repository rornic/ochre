//! The world around the terrain: the map it's carved from (erosion, rivers, lakes and climate),
//! and the sky, clouds, weather and ambient sound over it.

pub mod ambience;
pub mod clouds;
pub mod godrays;
pub mod music;
pub mod sky;
pub mod sky_dome;
pub mod storms;
pub mod tonemap;
pub mod weather;
pub mod worldmap;

use std::path::PathBuf;
use std::sync::Arc;

use ochre_terrain::world::World;
use worldmap::{io::load_or_generate, WorldConfig, WorldTerrain};

/// Where a world comes from, other than its config: all of it optional, for repeating a world or starting at a place.
#[derive(Clone, Debug)]
pub struct WorldOptions {
    /// The seed, a random one if unset, for chasing bugs that depend on the terrain.
    pub seed: Option<u32>,
    /// Where maps are saved, the first time a seed is played, and read from after.
    pub dir: PathBuf,
    /// Whether the seed draws the world's plates, relief, warmth and wind, see `worldmap::character`.
    pub character: bool,
    pub spawn: Option<SpawnAt>,
}

/// Where the player starts instead of the nearest lowland: over `x`, `z`, `lift` blocks above the ground.
#[derive(Clone, Copy, Debug)]
pub struct SpawnAt {
    pub x: f64,
    pub z: f64,
    pub lift: Option<f64>,
}

impl Default for WorldOptions {
    fn default() -> Self {
        Self {
            seed: None,
            dir: "worlds".into(),
            character: true,
            spawn: None,
        }
    }
}

/// A world over a generated map. `region_distance` is how far, in chunks, terrain is built from
/// full-detail regions, see `LodLayout::region_reach`.
pub fn load_world(region_distance: u32, config: WorldConfig, options: &WorldOptions) -> World {
    let start = std::time::Instant::now();
    let seed = options.seed.unwrap_or_else(rand::random);
    let config = worldmap::character::world_config_with(seed, config, options.character);
    tracing::info!(
        "world {seed}: {} plates, relief {}, warmth {}, wind ({}, {}), character {}",
        config.tectonics.plates,
        config.tectonics.relief,
        config.climate.warmth,
        config.climate.wind_x,
        config.climate.wind_z,
        if options.character { "on" } else { "off" }
    );
    let map = load_or_generate(&options.dir, seed, config);
    tracing::info!("world map for seed {seed} ready in {:.1?}", start.elapsed());
    let world = World::with_terrain(
        seed,
        region_distance,
        Arc::new(WorldTerrain::new(Arc::new(map))),
    );
    match options.spawn {
        Some(SpawnAt { x, z, lift }) => world.with_spawn(x, z, lift),
        None => world,
    }
}
