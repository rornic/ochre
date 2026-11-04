//! A diagnostic for how open and how dramatic the view is from the spawn regions the gameplay pass found. It reads
//! the world's ground only and changes no generation, and is not a way of placing anything.

use super::{FeatureKind, WorldTerrain};
use ochre_base::terrain_source::TerrainSource;

/// How far, in blocks, a ray looks, and how far apart its samples are.
const RAY_LENGTH: f64 = 3000.0;
const RAY_STEP: f64 = 32.0;
/// Rays cast round a site, and how far one must run clear to count as open.
const RAYS: usize = 16;
const OPEN_FROM: f64 = 1500.0;
/// How high over the ground the eye is.
const EYE: f64 = 2.0;
/// The most sites listed.
pub const MOST_SITES: usize = 20;

/// A spawn region and what can be seen from it.
#[derive(Debug, Clone, PartialEq)]
pub struct Site {
    pub x: i32,
    pub z: i32,
    /// How many of the rays ran 1.5 km or more before ground rose over the eye line.
    pub open_rays: usize,
    /// The highest ground any ray saw, over the eye.
    pub relief: f64,
    /// A direction, x y z, toward the highest ground seen, for framing a screenshot.
    pub look: (f64, f64, f64),
}

/// The spawn regions in the square `half` blocks each way from (`x`, `z`), best first as the gameplay pass scored them, at
/// most `MOST_SITES`, each with the view from it.
pub fn view_sites(terrain: &WorldTerrain, x: i64, z: i64, half: i64) -> Vec<Site> {
    terrain
        .map()
        .features
        .iter()
        .filter(|f| f.kind == FeatureKind::Spawn)
        .filter(|f| ((f.x as i64 - x).abs() <= half) && ((f.z as i64 - z).abs() <= half))
        .take(MOST_SITES)
        .map(|f| site(terrain, f.x, f.z))
        .collect()
}

fn site(terrain: &WorldTerrain, x: i32, z: i32) -> Site {
    let eye = terrain.height(x as f64, z as f64) + EYE;
    let (mut open_rays, mut relief, mut look) = (0, 0.0f64, (0.0, 0.0, 1.0));
    for k in 0..RAYS {
        let angle = k as f64 * std::f64::consts::TAU / RAYS as f64;
        let (dx, dz) = (libm::cos(angle), libm::sin(angle));
        let (mut run, mut highest) = (RAY_LENGTH, f64::MIN);
        let mut blocked = false;
        let mut at = RAY_STEP;
        while at <= RAY_LENGTH {
            let ground = terrain.height(x as f64 + dx * at, z as f64 + dz * at);
            highest = highest.max(ground);
            if !blocked && ground > eye {
                run = at;
                blocked = true;
            }
            at += RAY_STEP;
        }
        open_rays += (run >= OPEN_FROM) as usize;
        if highest - eye > relief {
            relief = highest - eye;
            look = (dx, 0.05, dz);
        }
    }
    Site {
        x,
        z,
        open_rays,
        relief,
        look,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worldmap::{generate, WorldConfig};
    use std::sync::Arc;

    #[test]
    fn sites_are_spawn_regions_in_score_order_with_a_view_each() {
        let config = WorldConfig {
            size: 32768,
            map_cell: 32,
            ..Default::default()
        };
        let map = Arc::new(generate(3, config));
        let spawns: Vec<(i32, i32)> = map
            .features
            .iter()
            .filter(|f| f.kind == FeatureKind::Spawn)
            .map(|f| (f.x, f.z))
            .collect();
        assert!(!spawns.is_empty(), "no spawn regions");
        let terrain = WorldTerrain::new(map);
        let sites = view_sites(&terrain, 0, 0, 40_000);
        assert_eq!(spawns.len().min(MOST_SITES), sites.len());
        let found: Vec<(i32, i32)> = sites.iter().map(|s| (s.x, s.z)).collect();
        assert_eq!(
            spawns[..found.len()],
            found[..],
            "not in the gameplay pass's order"
        );
        for s in &sites {
            assert!(s.open_rays <= RAYS);
            assert!(s.relief.is_finite());
            assert!(
                s.look.1 > 0.0 && (s.look.0 * s.look.0 + s.look.2 * s.look.2 - 1.0).abs() < 1e-9
            );
        }
        // a small window finds fewer
        assert!(view_sites(&terrain, 0, 0, 0).len() <= sites.len());
    }
}
