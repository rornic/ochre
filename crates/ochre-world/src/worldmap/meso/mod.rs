//! Scores a generated world for distinctive places, from the ground a player sees at 4 km: relief of either sign at three scales, and steepness, ranked
//! within each biome. It only reads. It adds no layer to the map, never runs in `generate`, and so never moves `MAP_HASH`. See `docs/designs/meso-scale-geography.md`.
//!
//! Every number here is a first pass, provisional until the baseline (`docs/meso-baseline.csv`) shows what the world gives.

pub mod canyons;
pub mod eskers;
pub mod landmarks;
pub mod ponds;
pub mod relief;
pub mod stacks;
mod rank;
mod terms;

pub use rank::{csv, Kind, Params, Place};

use super::climate::BiomeKind;
use super::WorldTerrain;
use ochre_base::terrain_source::TerrainSource;

/// The most distinctive places of `terrain`'s world, per biome, best first within each biome, biomes in a fixed order.
pub fn score(terrain: &WorldTerrain, params: &Params) -> Vec<Place> {
    let map = terrain.map();
    let half = map.config.half_size() as f64;
    let dim = (2.0 * half / params.step).floor() as usize;
    let at = |i: usize| {
        let (x, z) = (i % dim, i / dim);
        (-half + (x as f64 + 0.5) * params.step, -half + (z as f64 + 0.5) * params.step)
    };
    let mut heights = vec![0.0f32; dim * dim];
    let mut biome = vec![BiomeKind::Sea as u8; dim * dim];
    // the ground as far LOD draws it, at the map cell's 32 blocks
    super::par_rows(dim, &mut heights, |row, cells| {
        for (col, cell) in cells.iter_mut().enumerate() {
            let (x, z) = at(row * dim + col);
            *cell = terrain.sample_for_cell(x, z, 32.0).height as f32;
        }
    });
    for (i, b) in biome.iter_mut().enumerate() {
        let (x, z) = at(i);
        let (u, v) = map.to_cell(x, z);
        let max = (map.dim() - 1) as f64;
        *b = map.biome[map.index(u.round().clamp(0.0, max) as usize, v.round().clamp(0.0, max) as usize)];
    }
    let terms = terms::from_heights(&heights, dim, params.step);
    let water = terms::water(terrain, dim, params.step, at);
    rank::pick(&terms, &water, &biome, dim, params, at)
}
