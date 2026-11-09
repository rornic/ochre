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

// How wide the patches of shrubland are, in blocks, and how much thicker than average the thickest are and thinner the
// thinnest: shrubs grow in clumps with bare grass between, not evenly.
const SHRUB_PATCH: i64 = 14;
const SHRUB_PATCH_SPREAD: (f64, f64) = (0.25, 1.75);

/// The big plant a biome has in its open ground, by name, and how much of that ground it covers on average: shrubs in
/// warm grassland, and low heath where it is cold. Forests, jungles and deserts have none, and the shade has none.
fn shrubs(biome: Option<&str>) -> Option<(BlockType, f64)> {
    match biome {
        Some("grassland") => Some((BlockType::Shrub, 0.03)),
        Some("savanna") => Some((BlockType::Shrub, 0.035)),
        Some("steppe") => Some((BlockType::Shrub, 0.025)),
        Some("swamp") => Some((BlockType::Shrub, 0.02)),
        Some("taiga") => Some((BlockType::Heath, 0.02)),
        Some("tundra") => Some((BlockType::Heath, 0.08)),
        _ => None,
    }
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
    if let Some((plant, cover)) = shrubs(biome).filter(|_| !shaded) {
        let patch = unit(hash(seed ^ SHRUB_PATCH_SALT, x.div_euclid(SHRUB_PATCH), z.div_euclid(SHRUB_PATCH)));
        let (thin, thick) = SHRUB_PATCH_SPREAD;
        if unit(hash(seed ^ SHRUB_SALT, x, z)) < cover * (thin + (thick - thin) * patch * patch * patch) {
            return Some(plant);
        }
    }
    let roll = unit(hash(seed ^ PLANT_SALT, x, z));
    let warm = ((temperature - COLD.0) / (COLD.1 - COLD.0)).clamp(0.0, 1.0) as f64;

    let drift = hash(seed ^ DRIFT_SALT, x.div_euclid(DRIFT), z.div_euclid(DRIFT));
    let in_drift = unit(drift);
    let flowers = if in_drift < DRIFT_SHARE {
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
    fn test_shrubs_grow_in_patches_in_open_grassland_and_heath_in_the_cold() {
        let big = |biome: Option<&str>, shaded: bool| -> Vec<(i64, i64, BlockType)> {
            (0..400)
                .flat_map(|x| (0..400).map(move |z| (x, z)))
                .filter_map(|(x, z)| match flora_at(7, x, z, shaded, 0.6, biome) {
                    Some(p @ (BlockType::Shrub | BlockType::Heath)) => Some((x, z, p)),
                    _ => None,
                })
                .collect()
        };
        let share = |v: &[(i64, i64, BlockType)]| v.len() as f64 / 160_000.0;
        let grassland = big(Some("grassland"), false);
        assert!((0.01..0.04).contains(&share(&grassland)), "{}", share(&grassland));
        assert!(grassland.iter().all(|p| p.2 == BlockType::Shrub));
        let tundra = big(Some("tundra"), false);
        assert!(share(&tundra) > share(&grassland) && tundra.iter().all(|p| p.2 == BlockType::Heath));
        assert!(!big(Some("taiga"), false).is_empty());
        // none in the shade, a desert, a jungle or a forest, or where the biome is unknown
        for none in [Some("desert"), Some("jungle"), Some("forest"), None] {
            assert!(big(none, false).is_empty(), "{none:?}");
        }
        assert!(big(Some("grassland"), true).is_empty());
        // they clump: the thickest 14 block patches hold several times what the thinnest do
        let mut patches = std::collections::HashMap::new();
        for &(x, z, _) in &grassland {
            *patches.entry((x.div_euclid(SHRUB_PATCH), z.div_euclid(SHRUB_PATCH))).or_insert(0usize) += 1;
        }
        let (most, least) = (patches.values().max().unwrap(), patches.values().min().unwrap());
        assert!(*most >= 4 * (*least).max(1), "{most} {least}");
    }

    #[test]
    fn test_flora_thins_out_in_the_cold() {
        let (mild, chilly, frozen) = (count(false, 0.6), count(false, 0.2), count(false, 0.05));
        let sum = |c: [usize; 4]| c.iter().sum::<usize>();
        assert!(sum(chilly) < sum(mild) && sum(chilly) > 0);
        assert_eq!(0, sum(frozen));
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
