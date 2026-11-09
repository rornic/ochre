use crate::generate::trees::hash;
use ochre_base::block::BlockType;

// Salts for the flora's hashes, so they don't line up with the trees' or each other.
const PLANT_SALT: u64 = 0x5EED_F10A;
const DRIFT_SALT: u64 = 0xD81F_7F10;
const KIND_SALT: u64 = 0x6A55_0F3B;
const SHRUB_SALT: u64 = 0x5B2B_0AF1;
const SHRUB_PATCH_SALT: u64 = 0x9A7C_3D15;

// How wide the patches flowers grow in drifts across are, in blocks.
const DRIFT: i64 = 6;
// The share of patches with flowers in, and how thick they grow in the thickest.
const DRIFT_SHARE: f64 = 0.3;
const DRIFT_DENSITY: f64 = 0.3;
// How much of the open grass carries a tuft of tall grass, and of the forest floor.
const GRASS_DENSITY: f64 = 0.3;
const FOREST_GRASS_DENSITY: f64 = 0.12;
// How much of the forest floor, and the ground under a lone tree's crown, is ferns.
const FERN_DENSITY: f64 = 0.25;
// How much of the grass is short tufts, in the open and in the shade, and how much is wild
// grass gone to seed; the rest is tall grass.
const SHORT_SHARE: f64 = 0.45;
const SHADED_SHORT_SHARE: f64 = 0.65;
const SEED_SHARE: f64 = 0.15;
// How much thicker (or thinner) a biome's floor under trees is in ferns and shaded grass than a forest's.
const JUNGLE_UNDERSTORY: f64 = 2.0;
const TAIGA_UNDERSTORY: f64 = 0.5;
// The temperatures, see `NoiseGenerator::temperature`, the flora thins out between, to none
// at the first.
const COLD: (f32, f32) = (0.10, 0.35);

// Shrubs grow in clusters, a carpet of several close together with bare grass between. Each `CLUSTER` block cell holds at most
// one, centred in it so that its reach stays inside the cell, and a block within the cluster's radius takes a plant by
// `CLUSTER_DENSITY`. A cluster's radius is between `CLUSTER_RADIUS`'s two, so about five or six plants stand in one.
const CLUSTER: i64 = 16;
const CLUSTER_MARGIN: i64 = 4;
const CLUSTER_RADIUS: (f64, f64) = (2.2, 3.2);
const CLUSTER_DENSITY: f64 = 0.5;

/// The big plant a biome has in its open ground, by name, and the share of its `CLUSTER` cells that hold a cluster of them:
/// shrubs in warm grassland, and low heath where it is cold. Forests, jungles and deserts have none, and the shade has none.
fn shrubs(biome: Option<&str>) -> Option<(BlockType, f64)> {
    match biome {
        Some("grassland") => Some((BlockType::Shrub, 0.35)),
        Some("savanna") => Some((BlockType::Shrub, 0.4)),
        Some("steppe") => Some((BlockType::Shrub, 0.3)),
        Some("swamp") => Some((BlockType::Shrub, 0.25)),
        Some("taiga") => Some((BlockType::Heath, 0.25)),
        Some("tundra") => Some((BlockType::Heath, 0.7)),
        _ => None,
    }
}

// Whether a plant of a cluster stands at (x, z), for the `cells` share of cells that hold one.
fn in_cluster(seed: u64, x: i64, z: i64, cells: f64) -> bool {
    let (cx, cz) = (x.div_euclid(CLUSTER), z.div_euclid(CLUSTER));
    let h = hash(seed ^ SHRUB_PATCH_SALT, cx, cz);
    if unit(h) >= cells {
        return false;
    }
    let span = (CLUSTER - 2 * CLUSTER_MARGIN) as u64;
    let centre = (
        cx * CLUSTER + CLUSTER_MARGIN + ((h >> 8) % span) as i64,
        cz * CLUSTER + CLUSTER_MARGIN + ((h >> 24) % span) as i64,
    );
    let radius = CLUSTER_RADIUS.0 + (CLUSTER_RADIUS.1 - CLUSTER_RADIUS.0) * unit(h >> 40 | 1);
    let (dx, dz) = ((x - centre.0) as f64 + 0.0, (z - centre.1) as f64);
    dx * dx + dz * dz <= radius * radius && unit(hash(seed ^ SHRUB_SALT, x, z)) < CLUSTER_DENSITY
}

/// How thick the floor under trees grows in a biome by name, 1 being a forest's.
pub(super) fn understory(biome: Option<&str>) -> f64 {
    match biome {
        Some("jungle") => JUNGLE_UNDERSTORY,
        Some("taiga") => TAIGA_UNDERSTORY,
        _ => 1.0,
    }
}

fn unit(h: u64) -> f64 {
    (h >> 11) as f64 / (1u64 << 53) as f64
}

// The tundra has no poppies or dandelions, only its grass and heath.

/// The plant on the grass at (x, z), if any: shrubs or heath in patches of the open ground of some biomes,
/// grasses in the open, ferns in the shade, flowers in drifts of one kind, all thinning out in the cold.
pub(super) fn flora_at(
    seed: u32,
    x: i64,
    z: i64,
    shaded: bool,
    temperature: f32,
    biome: Option<&str>,
) -> Option<BlockType> {
    let understory = understory(biome);
    let seed = seed as u64;
    if let Some((plant, cells)) = shrubs(biome).filter(|_| !shaded) {
        if in_cluster(seed, x, z, cells) {
            return Some(plant);
        }
    }
    let roll = unit(hash(seed ^ PLANT_SALT, x, z));
    let warm = ((temperature - COLD.0) / (COLD.1 - COLD.0)).clamp(0.0, 1.0) as f64;

    let drift = hash(seed ^ DRIFT_SALT, x.div_euclid(DRIFT), z.div_euclid(DRIFT));
    let in_drift = unit(drift);
    let flowers = if in_drift < DRIFT_SHARE && biome != Some("tundra") {
        DRIFT_DENSITY * (1.0 - in_drift / DRIFT_SHARE)
    } else {
        0.0
    };
    let ferns = if shaded {
        FERN_DENSITY * understory
    } else {
        0.0
    };
    let grass = if shaded {
        FOREST_GRASS_DENSITY * understory
    } else {
        GRASS_DENSITY
    };

    let roll = roll / warm.max(f64::EPSILON);
    if roll < flowers {
        // one kind of flower to a drift
        Some(if drift & 1 == 0 {
            BlockType::Poppy
        } else {
            BlockType::Dandelion
        })
    } else if roll < flowers + ferns {
        Some(BlockType::Fern)
    } else if roll < flowers + ferns + grass {
        let kind = unit(hash(seed ^ KIND_SALT, x, z));
        let short = if shaded {
            SHADED_SHORT_SHARE
        } else {
            SHORT_SHARE
        };
        Some(if kind < short {
            BlockType::ShortGrass
        } else if kind > 1.0 - SEED_SHARE {
            BlockType::SeedGrass
        } else {
            BlockType::TallGrass
        })
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // how many of each grass (all three kinds), fern, poppy and dandelion grow over a patch
    fn count(shaded: bool, temperature: f32) -> [usize; 4] {
        let mut counts = [0; 4];
        for x in 0..200 {
            for z in 0..200 {
                if let Some(plant) = flora_at(7, x, z, shaded, temperature, None) {
                    let kind = match plant {
                        BlockType::TallGrass | BlockType::ShortGrass | BlockType::SeedGrass => 0,
                        BlockType::Fern => 1,
                        BlockType::Poppy => 2,
                        BlockType::Dandelion => 3,
                        other => panic!("{other:?} isn't flora"),
                    };
                    counts[kind] += 1;
                }
            }
        }
        counts
    }

    #[test]
    fn test_grass_and_flowers_in_the_open_and_ferns_only_in_the_shade() {
        let open = count(false, 0.6);
        let total = 200.0 * 200.0;
        assert!((0.2..0.45).contains(&(open[0] as f64 / total)), "{open:?}");
        assert_eq!(0, open[1]);
        assert!(open[2] > 0 && open[3] > 0, "{open:?}");
        let forest = count(true, 0.6);
        assert!(forest[1] > forest[0], "{forest:?}");
    }

    #[test]
    fn test_jungle_floor_is_thick_and_taiga_floor_thin_and_other_biomes_are_unchanged() {
        let cover = |name: Option<&str>| {
            (0..200)
                .flat_map(|x| (0..200).map(move |z| (x, z)))
                .filter(|&(x, z)| flora_at(7, x, z, true, 0.6, name).is_some())
                .count()
        };
        let (jungle, forest, taiga) = (
            cover(Some("jungle")),
            cover(Some("forest")),
            cover(Some("taiga")),
        );
        assert!(
            jungle > forest + forest / 2 && forest > taiga,
            "{jungle} {forest} {taiga}"
        );
        assert_eq!(forest, cover(Some("desert")));
        assert_eq!(forest, cover(None));
        assert_eq!(forest, count(true, 0.6).iter().sum::<usize>());
    }

    #[test]
    fn test_shrubs_grow_in_clusters_of_about_five_in_open_grassland_and_heath_in_the_cold() {
        let big = |biome: Option<&str>, shaded: bool| -> Vec<(i64, i64, BlockType)> {
            (0..640)
                .flat_map(|x| (0..640).map(move |z| (x, z)))
                .filter_map(|(x, z)| match flora_at(7, x, z, shaded, 0.6, biome) {
                    Some(p @ (BlockType::Shrub | BlockType::Heath)) => Some((x, z, p)),
                    _ => None,
                })
                .collect()
        };
        let share = |v: &[(i64, i64, BlockType)]| v.len() as f64 / (640.0 * 640.0);
        let grassland = big(Some("grassland"), false);
        assert!((0.005..0.02).contains(&share(&grassland)), "{}", share(&grassland));
        assert!(grassland.iter().all(|p| p.2 == BlockType::Shrub));
        let tundra = big(Some("tundra"), false);
        assert!(share(&tundra) > share(&grassland) && tundra.iter().all(|p| p.2 == BlockType::Heath));
        assert!(!big(Some("taiga"), false).is_empty());
        // none in the shade, a desert, a jungle or a forest, or where the biome is unknown
        for none in [Some("desert"), Some("jungle"), Some("forest"), None] {
            assert!(big(none, false).is_empty(), "{none:?}");
        }
        assert!(big(Some("grassland"), true).is_empty());
        // they stand together: each in a cluster has its own cell's others, and most clusters hold several
        let mut clusters = std::collections::HashMap::new();
        for &(x, z, _) in &tundra {
            *clusters.entry((x.div_euclid(CLUSTER), z.div_euclid(CLUSTER))).or_insert(0usize) += 1;
        }
        let mean = tundra.len() as f64 / clusters.len() as f64;
        assert!((3.0..10.0).contains(&mean), "{mean} to a cluster");
        // and each is within a few blocks of the rest of its cluster, so none is out on its own
        let alone = tundra
            .iter()
            .filter(|&&(x, z, _)| !tundra.iter().any(|&(ox, oz, _)| (ox, oz) != (x, z) && (ox - x).abs() <= 3 && (oz - z).abs() <= 3))
            .count();
        assert!((alone as f64) < 0.1 * tundra.len() as f64, "{alone} of {} are alone", tundra.len());
    }

    #[test]
    fn test_the_tundra_has_grass_and_heath_but_no_flowers() {
        let mut grass = 0;
        for x in 0..300 {
            for z in 0..300 {
                match flora_at(7, x, z, false, 0.2, Some("tundra")) {
                    Some(BlockType::Poppy | BlockType::Dandelion) => panic!("a flower in the tundra"),
                    Some(BlockType::TallGrass | BlockType::ShortGrass | BlockType::SeedGrass) => grass += 1,
                    _ => {}
                }
            }
        }
        assert!(grass > 1000, "{grass} tufts");
    }

    #[test]
    fn test_a_drift_is_one_kind_of_flower() {
        for (dx, dz) in [(0, 0), (6, 0), (12, 18), (-6, 30)] {
            let kinds: Vec<BlockType> = (0..DRIFT)
                .flat_map(|x| (0..DRIFT).map(move |z| (x, z)))
                .filter_map(|(x, z)| flora_at(3, dx + x, dz + z, false, 0.6, None))
                .filter(|p| matches!(p, BlockType::Poppy | BlockType::Dandelion))
                .collect();
            assert!(kinds.windows(2).all(|w| w[0] == w[1]), "{kinds:?}");
        }
    }
}
