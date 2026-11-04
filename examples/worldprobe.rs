// A command line probe of a seed's world, for digging at a spot without the viewer's server. It builds the
// same columns chunks are built from (`RegionStore` ground and water), so what it prints is what the game draws.
//   cargo run --release --example worldprobe -- <seed> info <x> <z>
//   cargo run --release --example worldprobe -- <seed> line <x> <z> <x|z> <len>   one row a block along an axis
//   cargo run --release --example worldprobe -- <seed> grid <x> <z> <half>        ascii: water kind and block-rounded level
//   cargo run --release --example worldprobe -- <seed> png <x> <z> <half> <out.png> [pixels_per_block]
//   cargo run --release --example worldprobe -- <seed> route <x> <z> [cells]
// Real blocks, from `generate_chunk_with`, so the cliff pass, trees and flora can each be switched off:
//   ... <seed> slice <x> <z> <x|z> <len> <y0> <y1> [flags]   a vertical cross-section, one letter a block
//   ... <seed> top <x> <z> <half> <out.png> [flags]           top-down surface blocks, 1 pixel a block
//   ... <seed> passes <x> <z> <half>                         blocks each pass changes in the area, and where
// flags: --no-cliffs --no-trees --no-flora
use std::collections::HashMap;
use std::sync::Arc;

use bevy::math::{I64Vec3, U16Vec3};
use ochre_base::block::BlockType;
use ochre_terrain::chunk::{ChunkCoordinate, ChunkData};
use ochre_terrain::generate::generator::{generate_chunk_with, ChunkPasses};

use image::{Rgb, RgbImage};
use ochre_base::terrain_source::{TerrainSource, WaterKind};
use ochre_terrain::generate::region_store::RegionStore;
use ochre_world::worldmap::{WaterCell, WorldConfig, WorldMap, WorldTerrain, NEIGHBOURS};

fn kind_char(kind: WaterKind) -> char {
    match kind {
        WaterKind::River => 'r',
        WaterKind::Lake => 'L',
        WaterKind::Ocean => 's',
        _ => ' ',
    }
}

fn letter(block: BlockType) -> char {
    match block {
        BlockType::Air => '.',
        BlockType::Water => '~',
        BlockType::Stone => '#',
        BlockType::CompactedStone => '%',
        BlockType::CompactedLimestone => 'n',
        BlockType::CompactedClay => 'k',
        BlockType::CompactedBasalt => 'a',
        BlockType::CompactedMarble => 'w',
        BlockType::CompactedSandstone => 'z',
        BlockType::CompactedGranite => 'q',
        BlockType::CompactedShale => 'j',
        BlockType::Granite => 'G',
        BlockType::Shale => 'h',
        BlockType::Limestone => 'm',
        BlockType::Clay => 'c',
        BlockType::Basalt => 'b',
        BlockType::Marble => 'M',
        BlockType::Dirt => 'd',
        BlockType::Grass => 'g',
        BlockType::Sand => 's',
        BlockType::Sandstone => 'S',
        BlockType::Gravel => ',',
        BlockType::Snow => '*',
        BlockType::Ice => 'i',
        BlockType::Log => 'L',
        BlockType::Leaves => 'l',
        _ => 'f',
    }
}

fn colour(block: BlockType) -> [u8; 3] {
    match block {
        BlockType::Water => [40, 110, 220],
        BlockType::Stone | BlockType::CompactedStone => [120, 120, 120],
        BlockType::Limestone => [200, 194, 172],
        BlockType::Clay => [178, 122, 80],
        BlockType::Basalt => [62, 64, 70],
        BlockType::Marble => [224, 222, 216],
        BlockType::CompactedLimestone => [124, 120, 107],
        BlockType::CompactedClay => [110, 76, 50],
        BlockType::CompactedBasalt => [38, 40, 43],
        BlockType::CompactedMarble => [139, 138, 134],
        BlockType::CompactedSandstone => [124, 105, 68],
        BlockType::Granite => [172, 152, 142],
        BlockType::Shale => [92, 88, 84],
        BlockType::CompactedGranite => [107, 94, 88],
        BlockType::CompactedShale => [57, 55, 52],
        BlockType::Dirt => [110, 75, 40],
        BlockType::Grass => [70, 160, 60],
        BlockType::Sand | BlockType::Sandstone => [220, 200, 130],
        BlockType::Gravel => [150, 140, 130],
        BlockType::Snow => [245, 245, 250],
        BlockType::Ice => [180, 220, 240],
        BlockType::Log => [90, 60, 30],
        BlockType::Leaves => [30, 100, 30],
        BlockType::Air => [0, 0, 0],
        _ => [200, 60, 120],
    }
}

fn passes_from(flags: &[String]) -> ChunkPasses {
    ChunkPasses {
        cliffs: !flags.iter().any(|f| f == "--no-cliffs"),
        trees: !flags.iter().any(|f| f == "--no-trees"),
        flora: !flags.iter().any(|f| f == "--no-flora"),
    }
}

// Blocks by world coordinate, generated a chunk at a time the way the game does.
struct Blocks {
    regions: Arc<RegionStore>,
    passes: ChunkPasses,
    chunks: HashMap<(i64, i64, i64), ChunkData>,
}

impl Blocks {
    fn new(regions: Arc<RegionStore>, passes: ChunkPasses) -> Self {
        Self {
            regions,
            passes,
            chunks: HashMap::new(),
        }
    }

    fn at(&mut self, x: i64, y: i64, z: i64) -> BlockType {
        let key = (x.div_euclid(16), y.div_euclid(16), z.div_euclid(16));
        let (regions, passes) = (self.regions.clone(), self.passes);
        let chunk = self.chunks.entry(key).or_insert_with(|| {
            generate_chunk_with(
                regions,
                ChunkCoordinate(I64Vec3::new(key.0, key.1, key.2)),
                passes,
            )
        });
        chunk.get_block_at(U16Vec3::new(
            x.rem_euclid(16) as u16,
            y.rem_euclid(16) as u16,
            z.rem_euclid(16) as u16,
        ))
    }

    // The y of the highest non-flora block in a column, from `top` down.
    fn surface(&mut self, x: i64, z: i64, top: i64, bottom: i64) -> Option<(i64, BlockType)> {
        (bottom..=top).rev().find_map(|y| {
            let b = self.at(x, y, z);
            (b != BlockType::Air && b.occlusion_weight() > 0.0).then_some((y, b))
        })
    }
}

fn describe(
    terrain: &WorldTerrain,
    map: &WorldMap,
    regions: &RegionStore,
    x: i64,
    z: i64,
) -> String {
    let mut sampler = regions.sampler();
    let p = terrain.sample(x as f64, z as f64);
    let (u, v) = map.to_cell(x as f64, z as f64);
    let max = map.dim() as f64 - 1.0;
    let cell = map.index(
        u.round().clamp(0.0, max) as usize,
        v.round().clamp(0.0, max) as usize,
    );
    format!(
        "x={x} z={z} ground={:.2} dry={:.2} base={:.2} water={:.2} {:?} | chunk ground_top={} water_top={} | map cell {:?} at {:.2}",
        p.height,
        terrain.dry_ground(x as f64, z as f64, 1.0),
        map.height_at(x as f64, z as f64),
        p.water_surface,
        p.water_kind,
        sampler.ground_top(x, z),
        sampler.water_top(x, z),
        WaterCell::from_u8(map.water_kind[cell]),
        map.water_surface[cell],
    )
}

// Longest run of biome-boundary samples (4 blocks apart) in one axis-aligned line, in blocks.
fn longest_straight_edge(terrain: &WorldTerrain, x: i64, z: i64, half: i64) -> String {
    const STEP: i64 = 4;
    let n = (2 * half / STEP) as usize;
    let at = |i: usize, j: usize| {
        terrain.biome(
            (x - half) as f64 + (i as i64 * STEP) as f64,
            (z - half) as f64 + (j as i64 * STEP) as f64,
        )
    };
    let grid: Vec<Vec<_>> = (0..n).map(|j| (0..n).map(|i| at(i, j)).collect()).collect();
    // a boundary sample differs from its east neighbour (a vertical edge) or south neighbour (a horizontal one)
    let longest = |differs: &dyn Fn(usize, usize) -> bool| {
        let mut best = 0;
        for a in 0..n - 1 {
            let mut run = 0;
            for b in 0..n - 1 {
                run = if differs(a, b) { run + 1 } else { 0 };
                best = best.max(run);
            }
        }
        best
    };
    let vertical = longest(&|i, j| grid[j][i] != grid[j][i + 1]);
    let horizontal = longest(&|j, i| grid[j][i] != grid[j + 1][i]);
    let blocks = |r: usize| r as i64 * STEP;
    // a crossing is the flips along a row within 48 blocks of each other: one flip is a hard edge, three or more a mix
    let mut crossings: Vec<(usize, i64)> = Vec::new();
    for row in &grid {
        let flips: Vec<usize> = (0..n - 1).filter(|&i| row[i] != row[i + 1]).collect();
        let mut at = 0;
        while at < flips.len() {
            let mut end = at;
            while end + 1 < flips.len() && (flips[end + 1] - flips[end]) as i64 * STEP <= 48 {
                end += 1;
            }
            crossings.push((end - at + 1, (flips[end] - flips[at]) as i64 * STEP));
            at = end + 1;
        }
    }
    let share = |keep: &dyn Fn(usize) -> bool| {
        100 * crossings.iter().filter(|c| keep(c.0)).count() / crossings.len().max(1)
    };
    let mut spans: Vec<i64> = crossings.iter().map(|c| c.1).collect();
    spans.sort();
    format!(
        "longest straight edge: {} blocks (vertical {}, horizontal {})\ncrossings {}: {}% hard (one flip), {}% mixed (three or more), median span {} blocks",
        blocks(vertical.max(horizontal)),
        blocks(vertical),
        blocks(horizontal),
        crossings.len(),
        share(&|f| f == 1),
        share(&|f| f >= 3),
        spans.get(spans.len() / 2).copied().unwrap_or(0)
    )
}

// How many samples (4 blocks apart) over a square window carry each relief feature, by kind and by biome.
fn feature_counts(terrain: &WorldTerrain, x: i64, z: i64, half: i64) -> String {
    let biomes = ochre_base::biome::Biomes::new(0);
    let mut by_kind: std::collections::BTreeMap<String, usize> = Default::default();
    let mut first: std::collections::BTreeMap<String, (i64, i64)> = Default::default();
    let (mut samples, mut outside) = (0usize, 0usize);
    for i in (-half..half).step_by(4) {
        for j in (-half..half).step_by(4) {
            let (wx, wz) = ((x + i) as f64, (z + j) as f64);
            samples += 1;
            let allowed = biomes.relief(terrain.biome(wx, wz));
            for (kind, offset) in terrain.relief_at(wx, wz) {
                if offset.abs() >= 0.5 {
                    *by_kind.entry(format!("{kind:?}")).or_default() += 1;
                    if offset.abs() >= 1.0 {
                        first.entry(format!("{kind:?}")).or_insert((x + i, z + j));
                    }
                    outside += !allowed.contains(&kind) as usize;
                }
            }
        }
    }
    let counts: Vec<String> = by_kind
        .iter()
        .map(|(k, n)| match first.get(k) {
            Some((fx, fz)) => format!("{k} {n} (e.g. {fx},{fz})"),
            None => format!("{k} {n}"),
        })
        .collect();
    format!(
        "{samples} samples; {} ; outside their biome {outside}",
        if counts.is_empty() {
            "no features".into()
        } else {
            counts.join(", ")
        }
    )
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 4 {
        eprintln!("usage: worldprobe <seed> info|line|grid|png|route|edges <x> <z> ...");
        std::process::exit(2);
    }
    let seed: u32 = args[0].parse().unwrap();
    let (x, z): (i64, i64) = (args[2].parse().unwrap(), args[3].parse().unwrap());
    let map = Arc::new(ochre_world::worldmap::generate(
        seed,
        ochre_world::worldmap::character::world_config(seed, WorldConfig::default()),
    ));
    let terrain = Arc::new(WorldTerrain::new(map.clone()));
    let regions = Arc::new(RegionStore::new(terrain.clone()));
    match args[1].as_str() {
        "info" => println!("{}", describe(&terrain, &map, &regions, x, z)),
        "features" => {
            let half: i64 = args.get(4).map_or(1024, |s| s.parse().unwrap());
            println!("{}", feature_counts(&terrain, x, z, half));
        }
        "route" => {
            let cells: usize = args.get(4).map_or(256, |s| s.parse().unwrap());
            let (u, v) = map.to_cell(x as f64, z as f64);
            let dim = map.dim();
            let mut i = map.index(
                (u.round() as i64).clamp(0, dim as i64 - 1) as usize,
                (v.round() as i64).clamp(0, dim as i64 - 1) as usize,
            );
            for step in 0..cells {
                let (cx, cz) = (i % dim, i / dim);
                let (wx, wz) = map.cell_centre(cx, cz);
                let kind = WaterCell::from_u8(map.water_kind[i]);
                let d = map.down[i];
                let next = if d == 255 {
                    None
                } else {
                    let (dx, dz) = NEIGHBOURS[d as usize];
                    let (nx, nz) = (cx as i64 + dx, cz as i64 + dz);
                    ((0..dim as i64).contains(&nx) && (0..dim as i64).contains(&nz))
                        .then(|| map.index(nx as usize, nz as usize))
                };
                if step.is_multiple_of(8) || kind != WaterCell::River || next.is_none() {
                    println!(
                        "{step:4} {wx:7.0},{wz:7.0} ground={:.1} {:?} surface={:.1} flow={:.0} down={:?}",
                        map.height[i], kind, map.water_surface[i], map.flow[i], next.map(|n| map.cell_centre(n % dim, n / dim))
                    );
                }
                let Some(n) = next else { break };
                i = n;
            }
        }
        "edges" => {
            let half: i64 = args[4].parse().unwrap();
            println!("{}", longest_straight_edge(&terrain, x, z, half));
        }
        "line" => {
            let along_x = args[4] == "x";
            let len: i64 = args[5].parse().unwrap();
            for d in 0..len {
                let (px, pz) = if along_x { (x + d, z) } else { (x, z + d) };
                println!("{}", describe(&terrain, &map, &regions, px, pz));
            }
        }
        "grid" => {
            let half: i64 = args[4].parse().unwrap();
            let mut sampler = regions.sampler();
            for pz in z - half..z + half {
                let mut row = String::new();
                for px in x - half..x + half {
                    let (g, w) = (sampler.ground_top(px, pz), sampler.water_top(px, pz));
                    let kind = terrain.sample(px as f64, pz as f64).water_kind;
                    if w > g {
                        row.push_str(&format!("{}{:<3}", kind_char(kind), w - 400));
                    } else {
                        row.push_str(&format!(" {:<3}", g - 400));
                    }
                }
                println!("{row}");
            }
        }
        "png" => {
            let half: i64 = args[4].parse().unwrap();
            let out = &args[5];
            let scale: u32 = args.get(6).map_or(1, |s| s.parse().unwrap());
            let mut sampler = regions.sampler();
            let n = (2 * half) as u32;
            let (mut lo, mut hi) = (i64::MAX, i64::MIN);
            for pz in z - half..z + half {
                for px in x - half..x + half {
                    let g = sampler.ground_top(px, pz);
                    lo = lo.min(g);
                    hi = hi.max(g);
                }
            }
            let mut img = RgbImage::new(n * scale, n * scale);
            for pz in z - half..z + half {
                for px in x - half..x + half {
                    let (g, w) = (sampler.ground_top(px, pz), sampler.water_top(px, pz));
                    let t = ((g - lo) as f64 / (hi - lo).max(1) as f64).clamp(0.0, 1.0);
                    let shade = (60.0 + 170.0 * t) as u8;
                    let rgb = if w > g {
                        let depth = ((w - g) as f64 / 12.0).clamp(0.0, 1.0);
                        match terrain.sample(px as f64, pz as f64).water_kind {
                            WaterKind::River => [30, (120.0 - 60.0 * depth) as u8, 255],
                            _ => [
                                (40.0 * (1.0 - depth)) as u8,
                                (170.0 - 110.0 * depth) as u8,
                                (230.0 - 70.0 * depth) as u8,
                            ],
                        }
                    } else {
                        [shade, shade, shade]
                    };
                    for dy in 0..scale {
                        for dx in 0..scale {
                            img.put_pixel(
                                (px - (x - half)) as u32 * scale + dx,
                                (pz - (z - half)) as u32 * scale + dy,
                                Rgb(rgb),
                            );
                        }
                    }
                }
            }
            img.save(out).unwrap();
            println!("wrote {out}");
        }
        "slice" => {
            let along_x = args[4] == "x";
            let len: i64 = args[5].parse().unwrap();
            let (y0, y1): (i64, i64) = (args[6].parse().unwrap(), args[7].parse().unwrap());
            let mut blocks = Blocks::new(regions.clone(), passes_from(&args[8..]));
            for y in (y0..=y1).rev() {
                let mut row = format!("{y:4} ");
                for d in 0..len {
                    let (px, pz) = if along_x { (x + d, z) } else { (x, z + d) };
                    row.push(letter(blocks.at(px, y, pz)));
                }
                println!("{row}");
            }
        }
        "top" => {
            let half: i64 = args[4].parse().unwrap();
            let out = &args[5];
            let mut blocks = Blocks::new(regions.clone(), passes_from(&args[6..]));
            let mut sampler = regions.sampler();
            let n = (2 * half) as u32;
            let mut img = RgbImage::new(n, n);
            for pz in z - half..z + half {
                for px in x - half..x + half {
                    let top = sampler.ground_top(px, pz).max(sampler.water_top(px, pz)) + 24;
                    let rgb = blocks
                        .surface(px, pz, top, top - 96)
                        .map_or([0, 0, 0], |(_, b)| colour(b));
                    img.put_pixel((px - (x - half)) as u32, (pz - (z - half)) as u32, Rgb(rgb));
                }
            }
            img.save(out).unwrap();
            println!("wrote {out}");
        }
        "passes" => {
            let half: i64 = args[4].parse().unwrap();
            let mut sampler = regions.sampler();
            let mut all = Blocks::new(regions.clone(), ChunkPasses::ALL);
            let off = [
                (
                    "cliffs",
                    ChunkPasses {
                        cliffs: false,
                        ..ChunkPasses::ALL
                    },
                ),
                (
                    "trees",
                    ChunkPasses {
                        trees: false,
                        ..ChunkPasses::ALL
                    },
                ),
                (
                    "flora",
                    ChunkPasses {
                        flora: false,
                        ..ChunkPasses::ALL
                    },
                ),
            ];
            for (name, passes) in off {
                let mut without = Blocks::new(regions.clone(), passes);
                let (mut changed, mut columns) = (0u64, 0u64);
                let mut examples = Vec::new();
                for pz in z - half..z + half {
                    for px in x - half..x + half {
                        let top = sampler.ground_top(px, pz).max(sampler.water_top(px, pz)) + 40;
                        let mut column = false;
                        for y in top - 120..=top {
                            let (a, b) = (all.at(px, y, pz), without.at(px, y, pz));
                            if a != b {
                                changed += 1;
                                if !column && examples.len() < 5 {
                                    examples.push(format!(
                                        "({px},{y},{pz}) {} -> {}",
                                        letter(b),
                                        letter(a)
                                    ));
                                }
                                column = true;
                            }
                        }
                        columns += column as u64;
                    }
                }
                println!(
                    "{name}: {changed} blocks in {columns} columns; e.g. {}",
                    examples.join("  ")
                );
            }
        }
        other => {
            eprintln!("unknown command {other}");
            std::process::exit(2);
        }
    }
}
