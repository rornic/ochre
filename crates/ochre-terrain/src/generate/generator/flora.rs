use crate::generate::trees::hash;
use ochre_base::block::BlockType;
use ochre_base::math::smoothstep_f64 as smoothstep;
use ochre_base::noise::perlin;

// Salts for the flora's hashes, so they don't line up with the trees' or each other.
const PLANT_SALT: u64 = 0x5EED_F10A;
const DRIFT_SALT: u64 = 0xD81F_7F10;
const KIND_SALT: u64 = 0x6A55_0F3B;
const SHRUB_SALT: u64 = 0x5B2B_0AF1;
const SHRUB_PATCH_SALT: u64 = 0x9A7C_3D15;

// Steppe's share of ground carrying a tuft at the thinnest and thickest, and the blocks across its broad patches and the finer variation.
const STEPPE_GRASS: (f64, f64) = (0.05, 0.42);
const STEPPE_PATCH: f64 = 180.0;
const STEPPE_FINE: f64 = 40.0;
// Bare earth shows on steppe's crests in patches this many blocks across, up to about an eighth of the ground at the most exposed.
const STEPPE_BARE: f64 = 60.0;
// The share of drifts that hold the steppe's flower, and how thick they grow, against other open ground's flowers.
const STEPPE_FLOWERS: (f64, f64) = (0.5, 0.8);
const STEPPE_BARE_FROM: f64 = 0.71;
const STEPPE_PATCH_SALT: u32 = 0x57E9_9E01;
const STEPPE_FINE_SALT: u32 = 0x57E9_9E02;
const STEPPE_BARE_SALT: u32 = 0x57E9_9E03;

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
        Some("steppe") => Some((BlockType::Shrub, 0.08)),
        Some("swamp") => Some((BlockType::Shrub, 0.25)),
        Some("taiga") => Some((BlockType::Heath, 0.25)),
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

// Large, irregular heath drifts, with the existing small clusters supplying their texture.
fn heath_drift(seed: u64, x: i64, z: i64) -> bool {
    let seed = seed as u32 ^ 0xD81F_7A31;
    let (x, z) = (x as f64, z as f64);
    ochre_base::noise::fbm(seed, x, z, 120.0, 3) > -0.04
}

// Larger, uneven carpets, with centres and radii varying independently from one another.
fn tundra_heath(seed: u64, x: i64, z: i64) -> bool {
    const CELL: i64 = 24;
    let (cx, cz) = (x.div_euclid(CELL), z.div_euclid(CELL));
    for dz in -1..=1 {
        for dx in -1..=1 {
            let (gx, gz) = (cx + dx, cz + dz);
            let h = hash(seed ^ SHRUB_PATCH_SALT ^ 0x00CA_4E71, gx, gz);
            let px = gx * CELL + ((h >> 8) % CELL as u64) as i64;
            let pz = gz * CELL + ((h >> 24) % CELL as u64) as i64;
            if unit(h) >= 0.75 || !heath_drift(seed, px, pz) {
                continue;
            }
            let radius = 3.0 + 6.0 * unit(hash(h, 1, 0));
            let dx = (x - px) as f64;
            let dz = (z - pz) as f64;
            if dx * dx + dz * dz >= (radius + 1.5).powi(2) {
                continue;
            }
            let edge = radius + 1.5 * ochre_base::noise::perlin(seed as u32 ^ 0x4EA7, x as f64 / 8.0, z as f64 / 8.0);
            if dx * dx + dz * dz < edge * edge && unit(hash(seed ^ SHRUB_SALT, x, z)) < 0.45 {
                return true;
            }
        }
    }
    false
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

// How thick steppe's grass grows at (x, z), 0..1: hollows and broad patches thick, crests thin, with variation inside a patch.
fn steppe_thickness(seed: u32, x: i64, z: i64, exposure: f32) -> f64 {
    let patch = perlin(seed ^ STEPPE_PATCH_SALT, x as f64 / STEPPE_PATCH, z as f64 / STEPPE_PATCH) * 0.5 + 0.5;
    let fine = perlin(seed ^ STEPPE_FINE_SALT, x as f64 / STEPPE_FINE, z as f64 / STEPPE_FINE) * 0.5 + 0.5;
    let base = 0.55 * (1.0 - exposure as f64) + 0.45 * smoothstep(0.3, 0.7, patch);
    (base * (0.75 + 0.5 * fine)).clamp(0.0, 1.0)
}

/// Whether steppe ground at (x, z) is bare earth: patches on the most exposed crests, none on level ground or in hollows.
pub(super) fn steppe_bare(seed: u32, x: i64, z: i64, exposure: f32) -> bool {
    let n = perlin(seed ^ STEPPE_BARE_SALT, x as f64 / STEPPE_BARE, z as f64 / STEPPE_BARE) * 0.5 + 0.5;
    n > STEPPE_BARE_FROM + 0.6 * (1.0 - exposure as f64)
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
    exposure: f32,
) -> Option<BlockType> {
    let understory = understory(biome);
    let steppe = !shaded && biome == Some("steppe");
    if steppe && steppe_bare(seed, x, z, exposure) {
        return None;
    }
    let thickness = if steppe { steppe_thickness(seed, x, z, exposure) } else { 0.0 };
    let seed = seed as u64;
    if !shaded && biome == Some("tundra") && tundra_heath(seed, x, z) {
        return Some(BlockType::Heath);
    }
    if let Some((plant, cells)) = shrubs(biome).filter(|_| !shaded) {
        // scrub shelters in hollows
        let cells = if steppe { cells * (0.4 + 1.2 * (1.0 - exposure as f64)) } else { cells };
        if in_cluster(seed, x, z, cells) {
            return Some(plant);
        }
    }
    let roll = unit(hash(seed ^ PLANT_SALT, x, z));
    let warm = ((temperature - COLD.0) / (COLD.1 - COLD.0)).clamp(0.0, 1.0) as f64;

    let drift = hash(seed ^ DRIFT_SALT, x.div_euclid(DRIFT), z.div_euclid(DRIFT));
    let in_drift = unit(drift);
    // the steppe's flower grows in a few drifts, thicker in its hollows
    let (drift_share, drift_density) = if steppe {
        (DRIFT_SHARE * STEPPE_FLOWERS.0, DRIFT_DENSITY * STEPPE_FLOWERS.1 * (0.3 + 0.7 * thickness))
    } else {
        (DRIFT_SHARE, DRIFT_DENSITY)
    };
    let flowers = if in_drift < drift_share && biome != Some("tundra") {
        drift_density * (1.0 - in_drift / drift_share)
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
    } else if steppe {
        STEPPE_GRASS.0 + (STEPPE_GRASS.1 - STEPPE_GRASS.0) * thickness
    } else {
        GRASS_DENSITY
    };

    let roll = roll / warm.max(f64::EPSILON);
    if roll < flowers {
        // one kind of flower to a drift
        Some(if steppe {
            BlockType::SteppeFlower
        } else if drift & 1 == 0 {
            BlockType::Poppy
        } else {
            BlockType::Dandelion
        })
    } else if roll < flowers + ferns {
        Some(BlockType::Fern)
    } else if roll < flowers + ferns + grass {
        let kind = unit(hash(seed ^ KIND_SALT, x, z));
        // steppe's crests are close-cropped and its thick patches tall, fine-stemmed and going to seed
        let (short, seeded, feather) = if steppe {
            (0.8 - 0.55 * thickness, 0.03 + 0.12 * thickness, 0.06 + 0.5 * thickness)
        } else if shaded {
            (SHADED_SHORT_SHARE, SEED_SHARE, 0.0)
        } else {
            (SHORT_SHARE, SEED_SHARE, 0.0)
        };
        Some(if kind < short {
            BlockType::ShortGrass
        } else if kind > 1.0 - seeded {
            BlockType::SeedGrass
        } else if kind > 1.0 - seeded - feather {
            BlockType::FeatherGrass
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
                if let Some(plant) = flora_at(7, x, z, shaded, temperature, None, 0.5) {
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
                .filter(|&(x, z)| flora_at(7, x, z, true, 0.6, name, 0.5).is_some())
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
                .filter_map(|(x, z)| match flora_at(7, x, z, shaded, 0.6, biome, 0.5) {
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
        for &(x, z, _) in &grassland {
            *clusters.entry((x.div_euclid(CLUSTER), z.div_euclid(CLUSTER))).or_insert(0usize) += 1;
        }
        let mean = grassland.len() as f64 / clusters.len() as f64;
        assert!((3.0..10.0).contains(&mean), "{mean} to a cluster");
        // and each is within a few blocks of the rest of its cluster, so none is out on its own
        let alone = grassland
            .iter()
            .filter(|&&(x, z, _)| !grassland.iter().any(|&(ox, oz, _)| (ox, oz) != (x, z) && (ox - x).abs() <= 3 && (oz - z).abs() <= 3))
            .count();
        assert!((alone as f64) < 0.1 * grassland.len() as f64, "{alone} of {} are alone", grassland.len());
    }

    #[test]
    fn tundra_heath_drifts_have_broad_connected_ground_and_gaps() {
        let mut within = 0;
        let mut outside = 0;
        let mut same_as_next = 0;
        for x in -200..200 {
            for z in -200..200 {
                let drift = heath_drift(7, x, z);
                within += usize::from(drift);
                outside += usize::from(!drift);
                same_as_next += usize::from(drift == heath_drift(7, x + 1, z));
            }
        }
        assert!(within > 20_000 && outside > 20_000, "{within} in, {outside} out");
        assert!(same_as_next > 150_000, "{same_as_next} adjacent pairs agree");
        let counts: Vec<_> = (0..40).flat_map(|cx| (0..40).map(move |cz| {
            (0..8).flat_map(|dx| (0..8).map(move |dz| (cx * 8 + dx, cz * 8 + dz)))
                .filter(|&(x, z)| tundra_heath(7, x, z)).count()
        })).collect();
        assert!(counts.iter().any(|&n| n >= 20), "no broad heath carpets");
        assert!(counts.iter().filter(|&&n| n == 0).count() > 400, "no open ground");
    }

    #[test]
    fn test_the_tundra_has_grass_and_heath_but_no_flowers() {
        let mut grass = 0;
        for x in 0..300 {
            for z in 0..300 {
                match flora_at(7, x, z, false, 0.2, Some("tundra"), 0.5) {
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
                .filter_map(|(x, z)| flora_at(3, dx + x, dz + z, false, 0.6, None, 0.5))
                .filter(|p| matches!(p, BlockType::Poppy | BlockType::Dandelion))
                .collect();
            assert!(kinds.windows(2).all(|w| w[0] == w[1]), "{kinds:?}");
        }
    }

    // the plants and the bare earth over a 300 block square of `biome` at one exposure
    fn steppe_ground(biome: &str, exposure: f32) -> (usize, usize, usize) {
        let (mut plants, mut tall, mut bare) = (0, 0, 0);
        for x in 0..300 {
            for z in 0..300 {
                if steppe_bare(7, x, z, exposure) {
                    bare += 1;
                }
                match flora_at(7, x, z, false, 0.5, Some(biome), exposure) {
                    Some(BlockType::Shrub) | None => {}
                    Some(BlockType::TallGrass | BlockType::SeedGrass | BlockType::FeatherGrass) => {
                        plants += 1;
                        tall += 1;
                    }
                    Some(_) => plants += 1,
                }
            }
        }
        (plants, tall, bare)
    }

    #[test]
    fn steppe_grass_is_thin_and_short_on_crests_and_thick_and_tall_in_hollows() {
        let (crest, crest_tall, _) = steppe_ground("steppe", 0.95);
        let (hollow, hollow_tall, _) = steppe_ground("steppe", 0.05);
        assert!(hollow > 2 * crest, "{hollow} plants in a hollow, {crest} on a crest");
        assert!(hollow_tall as f64 / hollow as f64 > crest_tall as f64 / crest.max(1) as f64 + 0.1);
    }

    #[test]
    fn steppe_shows_bare_earth_only_on_exposed_crests_and_never_more_than_an_eighth() {
        let (_, _, level) = steppe_ground("steppe", 0.5);
        let (_, _, hollow) = steppe_ground("steppe", 0.1);
        let (_, _, crest) = steppe_ground("steppe", 1.0);
        assert_eq!((0, 0), (level, hollow));
        assert!(crest > 3_000 && crest < 90_000 / 8, "{crest} of 90000 bare");
        // bare earth carries no plant
        assert!((0..300).all(|x| (0..300).all(|z| !steppe_bare(7, x, z, 1.0) || flora_at(7, x, z, false, 0.5, Some("steppe"), 1.0).is_none())));
    }

    #[test]
    fn other_biomes_ignore_exposure() {
        for biome in ["grassland", "savanna", "taiga", "tundra"] {
            let (low, high) = (steppe_ground(biome, 0.05), steppe_ground(biome, 0.95));
            assert_eq!((low.0, low.1), (high.0, high.1), "{biome}");
        }
    }

    // how many flowers and shrubs grow over a 300 block square of `biome` on level ground
    fn flowers_and_shrubs(biome: &str) -> (usize, usize) {
        let (mut flowers, mut shrubs) = (0, 0);
        for x in 0..300 {
            for z in 0..300 {
                match flora_at(7, x, z, false, 0.5, Some(biome), 0.5) {
                    Some(BlockType::Poppy | BlockType::Dandelion | BlockType::SteppeFlower) => flowers += 1,
                    Some(BlockType::Shrub) => shrubs += 1,
                    _ => {}
                }
            }
        }
        (flowers, shrubs)
    }

    #[test]
    fn steppe_has_far_fewer_flowers_and_shrubs_than_grassland() {
        let (steppe_flowers, steppe_shrubs) = flowers_and_shrubs("steppe");
        let (flowers, shrubs) = flowers_and_shrubs("grassland");
        assert!(steppe_flowers * 3 < flowers, "{steppe_flowers} flowers on steppe, {flowers} on grassland");
        assert!(steppe_shrubs * 3 < shrubs, "{steppe_shrubs} shrubs on steppe, {shrubs} on grassland");
        assert!(steppe_flowers > 0 && steppe_shrubs > 0, "rare, not none");
    }

    #[test]
    fn the_steppe_flower_is_the_steppes_alone_and_steppe_has_no_poppies_or_dandelions() {
        for biome in [None, Some("grassland"), Some("savanna"), Some("forest")] {
            assert!((0..300).all(|x| (0..300).all(|z| flora_at(7, x, z, false, 0.5, biome, 0.5) != Some(BlockType::SteppeFlower))));
        }
        assert!((0..300).all(|x| (0..300).all(|z| !matches!(flora_at(7, x, z, false, 0.5, Some("steppe"), 0.1), Some(BlockType::Poppy | BlockType::Dandelion)))));
    }

    // how many feather grass tufts grow over a 300 block square of `biome` at one exposure
    fn feather_grass(biome: &str, exposure: f32) -> usize {
        (0..300)
            .flat_map(|x| (0..300).map(move |z| (x, z)))
            .filter(|&(x, z)| flora_at(7, x, z, false, 0.5, Some(biome), exposure) == Some(BlockType::FeatherGrass))
            .count()
    }

    #[test]
    fn feather_grass_is_the_steppes_alone_and_thickest_in_hollows() {
        let (hollow, crest) = (feather_grass("steppe", 0.05), feather_grass("steppe", 0.95));
        assert!(hollow > 3 * crest.max(1), "{hollow} tufts in a hollow, {crest} on a crest");
        assert!(hollow > 1_000, "{hollow} tufts in a hollow is not a signature plant");
        assert!(crest > 0, "even a crest has a few");
        for biome in ["grassland", "savanna", "taiga", "tundra", "forest", "jungle", "swamp"] {
            assert_eq!(0, feather_grass(biome, 0.05), "{biome}");
        }
    }
}
