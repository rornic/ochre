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
//   ... <seed> render <x> <z> <half> <out.png> [azimuth]       the meshes as the game builds them, sun-shaded in mean colours
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
use ochre_world::worldmap::valleys::Valley;
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
        BlockType::Rock => '#',
        BlockType::CompactedRock => '%',
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
        BlockType::Rock | BlockType::CompactedRock => [120, 120, 120],
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
        "x={x} z={z} biome={} ground={:.2} dry={:.2} base={:.2} water={:.2} {:?} | chunk ground_top={} water_top={} | map cell {:?} at {:.2} | temperature {:.2} humidity {:.2} exposure {:.2} water_closeness {:.2}",
        terrain
            .biomes()
            .name(terrain.biome(x as f64, z as f64))
            .unwrap_or("?"),
        p.height,
        terrain.dry_ground(x as f64, z as f64, 1.0),
        map.height_at(x as f64, z as f64),
        p.water_surface,
        p.water_kind,
        sampler.ground_top(x, z),
        sampler.water_top(x, z),
        WaterCell::from_u8(map.water_kind[cell]),
        map.water_surface[cell],
        terrain.climate(x as f64, z as f64).temperature,
        terrain.climate(x as f64, z as f64).humidity,
        terrain.exposure(x as f64, z as f64),
        terrain.water_closeness(x as f64, z as f64),
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
        eprintln!("usage: worldprobe <seed> info|line|grid|png|route|edges|valley <x> <z> ...");
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
        "sites" => {
            let half: i64 = args.get(4).map_or(4096, |s| s.parse().unwrap());
            println!("x z open_rays(of 16) relief_in_view");
            for site in ochre_world::worldmap::view::view_sites(&terrain, x, z, half) {
                println!(
                    "{} {} {} {:.0}  OCHRE_SEED={seed} OCHRE_SPAWN={},{} OCHRE_LOOK={:.2},{:.2},{:.2}",
                    site.x, site.z, site.open_rays, site.relief, site.x, site.z, site.look.0, site.look.1, site.look.2
                );
            }
        }
        "valley" => {
            // the valley at a point, then how much of the window round it is floor or wall, with spawn lines at walls
            let half: i64 = args.get(4).map_or(2048, |s| s.parse().unwrap());
            let valley = Valley::of(&map);
            let dim = map.dim() as f64;
            let weight = |x: f64, z: f64| {
                let (u, v) = map.to_cell(x, z);
                let cell = |t: f64| t.round().clamp(0.0, dim - 1.0) as usize;
                map.valley[map.index(cell(u), cell(v))] as f64 / 255.0
            };
            let cut = valley.at(x as f64, z as f64);
            println!(
                "at ({x}, {z}): weight {:.2} depth {:.0} soft {:.2} sharp {:.2} wall {:.2} exposure {:.2} ground {:.1}",
                weight(x as f64, z as f64),
                cut.depth,
                cut.soft,
                cut.sharp,
                terrain.valley_wall(x as f64, z as f64),
                terrain.exposure(x as f64, z as f64),
                terrain.height(x as f64, z as f64)
            );
            let (mut samples, mut floors, mut walls, mut floor_spots) = (0, 0, Vec::new(), Vec::new());
            for wz in (z - half..z + half).step_by(32) {
                for wx in (x - half..x + half).step_by(32) {
                    samples += 1;
                    let (fx, fz) = (wx as f64, wz as f64);
                    if weight(fx, fz) > 0.2 && valley.at(fx, fz).soft > 0.9 {
                        floors += 1;
                        if floor_spots.iter().all(|&(px, pz): &(i64, i64)| (px - wx).abs().max((pz - wz).abs()) > 1024) {
                            floor_spots.push((wx, wz));
                        }
                    }
                    if terrain.valley_wall(fx, fz) > 0.6 && walls.iter().all(|&(px, pz): &(i64, i64)| (px - wx).abs().max((pz - wz).abs()) > 256) {
                        walls.push((wx, wz));
                    }
                }
            }
            println!("{samples} samples: {floors} on a floor, {} wall spots", walls.len());
            for (wx, wz) in walls.iter().take(args.get(5).map_or(8, |s| s.parse().unwrap())) {
                println!("OCHRE_SEED={seed} OCHRE_SPAWN={wx},{wz},20 OCHRE_LOOK=0,-0.1,1");
            }
            println!("floor spots:");
            for (fx, fz) in floor_spots.iter().take(args.get(5).map_or(8, |s| s.parse().unwrap())) {
                println!("OCHRE_SEED={seed} OCHRE_SPAWN={fx},{fz},20 OCHRE_LOOK=0,-0.1,1");
            }
        }
        "falls" => {
            // every fall in the world, the tallest first: drop, level above, then a spawn line for it
            let mut falls = terrain.falls();
            falls.sort_by(|a, b| b.2.total_cmp(&a.2));
            let bins = [
                (2.5, 4.0),
                (4.0, 6.0),
                (6.0, 10.0),
                (10.0, 20.0),
                (20.0, f32::MAX),
            ];
            let counts: Vec<String> = bins
                .iter()
                .map(|&(lo, hi)| {
                    let n = falls.iter().filter(|f| (lo..hi).contains(&f.2)).count();
                    format!("{lo}..{hi}: {n}")
                })
                .collect();
            println!("{} falls ({})", falls.len(), counts.join(", "));
            for (fx, fz, drop, level) in falls
                .iter()
                .take(args.get(4).map_or(12, |s| s.parse().unwrap()))
            {
                println!(
                    "drop {drop:.1} from {level:.0}  OCHRE_SEED={seed} OCHRE_SPAWN={:.0},{:.0},12 OCHRE_LOOK={fx:.0},{level:.0},{fz:.0}",
                    fx - 20.0,
                    fz - 20.0
                );
            }
        }
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
        "render" => {
            let half: i64 = args[4].parse().unwrap();
            let azimuth: f32 = args.get(6).map_or(225.0, |a| a.parse().unwrap());
            render(&regions, x, z, half, azimuth, &args[5]);
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

// The chunks round (`x`, `z`) meshed as the game meshes them, drawn from above at `azimuth` degrees with an orthographic
// camera, each face its tile's mean colour times its vertex colour, lit by one sun. Plants and leaves are left out, as
// their cards would draw solid.
fn render(regions: &Arc<RegionStore>, x: i64, z: i64, half: i64, azimuth: f32, out: &str) {
    use bevy::math::Vec3;
    use bevy::render::mesh::{Indices, Mesh, VertexAttributeValues};
    use ochre_terrain::chunk::CHUNK_SIZE;
    use ochre_terrain::generate::generator::{generate_chunk, generate_chunk_mesh_with};
    let size = CHUNK_SIZE as i64;
    let shader = std::fs::read_to_string("assets/shaders/block_texture.wesl").unwrap();
    let means: Vec<Vec3> = shader
        .split("fn block_mean")
        .nth(1)
        .unwrap()
        .lines()
        .filter_map(|line| line.trim().strip_prefix("vec3<f32>(")?.split(')').next())
        .map(|args| {
            let v: Vec<f32> = args.split(',').map(|a| a.trim().parse().unwrap()).collect();
            Vec3::new(v[0], v[1], v[2])
        })
        .collect();
    let mut sampler = regions.sampler();
    let (mut low, mut high) = (i64::MAX, i64::MIN);
    for pz in (z - half..=z + half).step_by(4) {
        for px in (x - half..=x + half).step_by(4) {
            let top = sampler.ground_top(px, pz);
            low = low.min(top);
            high = high.max(top.max(sampler.water_top(px, pz)));
        }
    }
    let chunk_of = |v: i64| v.div_euclid(size);
    let (cx0, cx1, cz0, cz1) = (chunk_of(x - half), chunk_of(x + half), chunk_of(z - half), chunk_of(z + half));
    let (cy0, cy1) = (chunk_of(low - 4), chunk_of(high + 12));
    let mut chunks: HashMap<(i64, i64, i64), Arc<ChunkData>> = HashMap::new();
    let mut chunk_at = |c: (i64, i64, i64)| {
        chunks
            .entry(c)
            .or_insert_with(|| Arc::new(generate_chunk(regions.clone(), ChunkCoordinate(I64Vec3::new(c.0, c.1, c.2)))))
            .clone()
    };
    // (position, normal, colour) triangles in world blocks
    let mut triangles: Vec<[(Vec3, Vec3, Vec3); 3]> = Vec::new();
    let mut vertices = 0usize;
    // (position, normal, colour) triangles in world blocks, from `mesh` placed at `origin` and scaled by `scale`
    let mut add = |mesh: &Mesh, origin: Vec3, scale: f32, water: bool| {
        let (
            Some(VertexAttributeValues::Float32x3(positions)),
            Some(VertexAttributeValues::Float32x3(normals)),
            Some(VertexAttributeValues::Float32x4(colours)),
            Some(Indices::U32(indices)),
        ) = (
            mesh.attribute(Mesh::ATTRIBUTE_POSITION),
            mesh.attribute(Mesh::ATTRIBUTE_NORMAL),
            mesh.attribute(Mesh::ATTRIBUTE_COLOR),
            mesh.indices(),
        )
        else {
            return;
        };
        if !water {
            vertices += positions.len();
        }
        // OCHRE_RENDER_FLAT=1 shades each triangle by its own normal, as the game cuts sculpted rock's facets
        let flat = std::env::var("OCHRE_RENDER_FLAT").is_ok();
        for tri in indices.chunks(3) {
            let cell = colours[tri[0] as usize][3].round() as usize;
            let corners: [Vec3; 3] = std::array::from_fn(|k| Vec3::from(positions[tri[k] as usize]));
            let face = (corners[2] - corners[0]).cross(corners[1] - corners[0]).normalize_or_zero();
            // leaves and the plants after the steppe's top in the atlas
            if !water && (cell == 6 || cell >= 30) {
                continue;
            }
            triangles.push(std::array::from_fn(|k| {
                let i = tri[k] as usize;
                let n = normals[i];
                // the cube's faces along z store their normals reversed
                let normal = if flat { face } else { Vec3::new(n[0], n[1], -n[2]) };
                let c = colours[i];
                let colour = if water {
                    Vec3::new(0.16, 0.36, 0.55)
                } else {
                    means.get(cell).copied().unwrap_or(Vec3::ONE) * Vec3::new(c[0], c[1], c[2])
                };
                (Vec3::from(positions[i]) * scale + origin, normal, colour)
            }));
        }
    };
    // OCHRE_RENDER_LOD=<level> draws the coarse nodes of that level over the area instead of the chunks
    let lod: u8 = std::env::var("OCHRE_RENDER_LOD").ok().and_then(|l| l.parse().ok()).unwrap_or(0);
    // OCHRE_RENDER_HANDOVER=1 draws chunks in the node holding (`x`, `z`) instead, to look at the seam round it
    let centre_node = ochre_terrain::lod::LodKey::containing(ChunkCoordinate(I64Vec3::new(chunk_of(x), 0, chunk_of(z))), lod.max(1)).coord;
    let handover = lod > 0 && std::env::var("OCHRE_RENDER_HANDOVER").is_ok();
    let in_centre = |cx: i64, cz: i64| (cx >> lod, cz >> lod) == (centre_node.x, centre_node.z);
    if lod > 0 {
        let mut keys = std::collections::BTreeSet::new();
        for cx in cx0..=cx1 {
            for cz in cz0..=cz1 {
                for cy in cy0..=cy1 {
                    let key = ochre_terrain::lod::LodKey::containing(ChunkCoordinate(I64Vec3::new(cx, cy, cz)), lod);
                    keys.insert((key.coord.x, key.coord.y, key.coord.z));
                }
            }
        }
        for (x, y, z) in keys.into_iter().filter(|&(x, _, z)| !handover || (x, z) != (centre_node.x, centre_node.z)) {
            let key = ochre_terrain::lod::LodKey { level: lod, coord: I64Vec3::new(x, y, z) };
            let cell = key.cell_size() as f32;
            let origin = key.min_block().as_vec3() + Vec3::splat(0.5 * cell - 0.5);
            if let Some(meshes) = ochre_terrain::generate::generator::generate_lod_mesh(regions.clone(), key) {
                add(&meshes.opaque, origin, cell, false);
                if let Some(water) = &meshes.water {
                    add(water, origin, cell, true);
                }
            }
        }
    }
    for cx in cx0..=cx1 {
        for cz in (cz0..=cz1).filter(|&cz| lod == 0 || (handover && in_centre(cx, cz))) {
            for cy in cy0..=cy1 {
                let chunk = chunk_at((cx, cy, cz));
                let mut neighbours = vec![None; 26];
                for dx in -1..=1 {
                    for dy in -1..=1 {
                        for dz in -1..=1 {
                            let at = ((dx + 1) * 9 + (dy + 1) * 3 + (dz + 1)) as usize;
                            if at == 13 {
                                continue;
                            }
                            neighbours[if at < 13 { at } else { at - 1 }] =
                                Some(chunk_at((cx + dx as i64, cy + dy as i64, cz + dz as i64)));
                        }
                    }
                }
                let coord = ChunkCoordinate(I64Vec3::new(cx, cy, cz));
                let meshes = generate_chunk_mesh_with(chunk, neighbours, coord, regions, true);
                let origin = Vec3::new((cx * size) as f32, (cy * size) as f32, (cz * size) as f32);
                add(&meshes.opaque, origin, 1.0, false);
                if let Some(water) = &meshes.water {
                    add(water, origin, 1.0, true);
                }
            }
        }
    }
    let (w, h) = (1600u32, 1000u32);
    let centre = Vec3::new(x as f32, sampler.ground_top(x, z) as f32, z as f32);
    let (yaw, pitch) = (azimuth.to_radians(), 35f32.to_radians());
    let forward = Vec3::new(yaw.cos() * pitch.cos(), -pitch.sin(), yaw.sin() * pitch.cos());
    let right = forward.cross(Vec3::Y).normalize();
    let up = right.cross(forward);
    let scale = w as f32 / (2.5 * half as f32);
    let sun = Vec3::new(-0.45, 0.75, -0.3).normalize();
    let mut depth = vec![f32::INFINITY; (w * h) as usize];
    let mut img = RgbImage::from_pixel(w, h, Rgb([170, 196, 222]));
    for tri in &triangles {
        let screen = tri.map(|(p, _, _)| {
            let d = p - centre;
            (w as f32 / 2.0 + d.dot(right) * scale, h as f32 / 2.0 - d.dot(up) * scale, d.dot(forward))
        });
        let area = (screen[1].0 - screen[0].0) * (screen[2].1 - screen[0].1)
            - (screen[2].0 - screen[0].0) * (screen[1].1 - screen[0].1);
        if area.abs() < 1e-6 {
            continue;
        }
        let (min_x, max_x) = (
            screen.iter().map(|s| s.0).fold(f32::MAX, f32::min).floor().max(0.0) as u32,
            screen.iter().map(|s| s.0).fold(f32::MIN, f32::max).ceil().min(w as f32 - 1.0) as u32,
        );
        let (min_y, max_y) = (
            screen.iter().map(|s| s.1).fold(f32::MAX, f32::min).floor().max(0.0) as u32,
            screen.iter().map(|s| s.1).fold(f32::MIN, f32::max).ceil().min(h as f32 - 1.0) as u32,
        );
        if min_x > max_x || min_y > max_y {
            continue;
        }
        for py in min_y..=max_y {
            for px in min_x..=max_x {
                let (fx, fy) = (px as f32 + 0.5, py as f32 + 0.5);
                let edge = |a: usize, b: usize| {
                    (screen[b].0 - screen[a].0) * (fy - screen[a].1) - (screen[b].1 - screen[a].1) * (fx - screen[a].0)
                };
                let weights = [edge(1, 2) / area, edge(2, 0) / area, edge(0, 1) / area];
                if weights.iter().any(|&b| b < 0.0) {
                    continue;
                }
                let d = weights[0] * screen[0].2 + weights[1] * screen[1].2 + weights[2] * screen[2].2;
                let at = (py * w + px) as usize;
                if d >= depth[at] {
                    continue;
                }
                depth[at] = d;
                let normal = (tri[0].1 * weights[0] + tri[1].1 * weights[1] + tri[2].1 * weights[2]).normalize_or_zero();
                let colour = tri[0].2 * weights[0] + tri[1].2 * weights[1] + tri[2].2 * weights[2];
                let lit = colour * (0.35 + 0.9 * normal.dot(sun).max(0.0));
                // a rough sRGB, so mid colours aren't crushed
                let rgb = lit.map(|c| c.clamp(0.0, 1.0).powf(1.0 / 2.2) * 255.0);
                img.put_pixel(px, py, Rgb([rgb.x as u8, rgb.y as u8, rgb.z as u8]));
            }
        }
    }
    img.save(out).unwrap();
    println!("wrote {out}: {} triangles drawn, {vertices} opaque vertices", triangles.len());
    // by slope, each triangle's own face: walkable (normal.y over 0.4, as `GROUND_FACING`), steep (up to a wall), and a wall or overhang (normal.y under 0.1),
    // with the share of the ground area each holds; and the interior's open edges, which a sealed surface has none of, the plants and leaves left out
    {
        let (mut area, mut count) = ([0.0f32; 3], [0usize; 3]);
        let key = |p: Vec3| ((p.x * 16.0).round() as i64, (p.y * 16.0).round() as i64, (p.z * 16.0).round() as i64);
        type Point = (i64, i64, i64);
        let mut edges: HashMap<(Point, Point), u32> = HashMap::new();
        // water is a sheet open along its shore, so it is left out of both counts
        for tri in triangles.iter().filter(|t| t[0].2 != Vec3::new(0.16, 0.36, 0.55)) {
            let [a, b, c] = [tri[0].0, tri[1].0, tri[2].0];
            let cross = (c - a).cross(b - a);
            let up = cross.normalize_or_zero().y;
            let band = if up > 0.4 { 0 } else if up > 0.1 { 1 } else { 2 };
            area[band] += cross.length() / 2.0;
            count[band] += 1;
            for (p, q) in [(a, b), (b, c), (c, a)] {
                let (kp, kq) = (key(p), key(q));
                *edges.entry(if kp < kq { (kp, kq) } else { (kq, kp) }).or_default() += 1;
            }
        }
        let total: f32 = area.iter().sum::<f32>().max(1e-6);
        println!(
            "slope bands by area: walkable {:.1}%, steep {:.1}%, wall {:.1}% ({} / {} / {} triangles)",
            100.0 * area[0] / total, 100.0 * area[1] / total, 100.0 * area[2] / total, count[0], count[1], count[2]
        );
        // the area's sides and the chunk rows' ends are where the drawing stops, so edges there are the picture's, not the mesh's
        let (floor_y, ceiling_y) = ((cy0 * size) as f32 + 8.0, ((cy1 + 1) * size) as f32 - 8.0);
        let inside = |p: (i64, i64, i64)| {
            let y = p.1 as f32 / 16.0;
            (p.0 as f32 / 16.0 - x as f32).abs() < half as f32 * 0.8 && (p.2 as f32 / 16.0 - z as f32).abs() < half as f32 * 0.8 && y > floor_y && y < ceiling_y
        };
        // trees are drawn by their own mesher, open where they meet what they stand in, so an edge with a log or leaf block beside it is the tree's
        let mut near_tree = |p: Point, q: Point| {
            let mid = ((p.0 + q.0) as f32 / 32.0, (p.1 + q.1) as f32 / 32.0, (p.2 + q.2) as f32 / 32.0);
            // trunks sink into the ground, so a trunk's base sits up to two blocks under its first log, and the edge looks up for it
            [-0.3f32, 0.3].iter().any(|&o| {
                [-0.3f32, 0.3, 1.3, 2.3].iter().any(|&u| {
                    [-0.3f32, 0.3].iter().any(|&w| {
                        let b = I64Vec3::new((mid.0 + o).round() as i64, (mid.1 + u).round() as i64, (mid.2 + w).round() as i64);
                        let c = (b.x.div_euclid(size), b.y.div_euclid(size), b.z.div_euclid(size));
                        let block = chunk_at(c).get_block_at(b.rem_euclid(I64Vec3::splat(size)).as_u16vec3());
                        matches!(block, BlockType::Log | BlockType::Leaves)
                    })
                })
            })
        };
        let open_list: Vec<(Point, Point)> = edges.iter().filter(|(&(p, q), &n)| n == 1 && inside(p) && inside(q)).map(|(&e, _)| e).collect();
        let (mut treed, mut open_ground) = (0usize, Vec::new());
        for &(p, q) in &open_list {
            if near_tree(p, q) { treed += 1 } else { open_ground.push((p, q)) }
        }
        println!("  of them {treed} beside tree blocks, {} in the ground", open_ground.len());
        let open = open_ground.len();
        let on_plane = open_ground.iter().filter(|&&(p, q)| {
            let plane = |a: i64, b: i64| a == b && ((a as f32 / 16.0 + 0.5).rem_euclid(size as f32).min(size as f32 - (a as f32 / 16.0 + 0.5).rem_euclid(size as f32))) < 0.1;
            plane(p.0, q.0) || plane(p.1, q.1) || plane(p.2, q.2)
        }).count();
        println!("  of those {on_plane} lie in a chunk plane");
        println!("open edges inside the middle 80% of the area, off the rows the drawing starts and stops at: {open}");
        // where they cluster, in 32-block bins: a hole is a few edges in one place, a seam or a tree a long line of them
        let mut bins: HashMap<(i64, i64, i64), (u32, i64)> = HashMap::new();
        for &(p, q) in &open_ground {
            {
                let entry = bins.entry((p.0 / 512, p.1 / 512, p.2 / 512)).or_default();
                entry.0 += 1;
                entry.1 += (p.0 + q.0) / 2 / 16 * 1000 + (p.2 + q.2) / 2 / 16;
            }
        }
        let mut worst: Vec<_> = bins.into_iter().collect();
        worst.sort_by(|a, b| b.1 .0.cmp(&a.1 .0).then(a.0.cmp(&b.0)));
        // OCHRE_RENDER_EDGES=1 lists each open edge's ends in blocks, to see whether they fall on chunk planes
        if std::env::var("OCHRE_RENDER_EDGES").is_ok() {
            let mut list = open_ground.clone();
            list.sort();
            for (p, q) in list {
                let f = |k: (i64, i64, i64)| (k.0 as f32 / 16.0, k.1 as f32 / 16.0, k.2 as f32 / 16.0);
                println!("edge {:?} -> {:?}", f(p), f(q));
            }
        }
        for ((bx, by, bz), (n, _)) in worst.iter().take(8) {
            println!("  {n} open edges near x {} y {} z {} (32-block bin)", bx * 32, by * 32, bz * 32);
        }
    }
}
