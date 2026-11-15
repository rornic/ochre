//! The weather over a whole world, from its seeded `Forecast`, without a window: how often each
//! biome sees rain, and maps of the storms drifting over it.
//!
//!   cargo run --release --example weather_map -- --seed 1 [--days 30] [--out target/weather_map]
//!   cargo run --release --example weather_map -- --seed 1 --snow
//!
//! `--snow` instead finds where and when it snows longest by day in the taiga, tundra and
//! glaciers, within the first `--days` (at most 12), and prints the command to start a game there.
//!
//! Prints, per biome over `--days` game days: the share of the time it rains, the share of days
//! with any rain, and how long runs of wet days and of dry days last on average. Writes `<out>/seed<N>-day<D>.png` for a few moments
//! over those days: biomes faded under the clouds (white), storm cloud (dark) and rain (blue), the
//! spawn in red and an arrow of the storms' drift.
use std::{path::Path, sync::Arc};

use bevy::math::DVec2;
use image::{Rgb, RgbImage};
use ochre_base::{cold::SNOWY_GRASS_TEMPERATURE, terrain_source::TerrainSource};
use ochre_world::{
    sky::{daylight, TimeOfDay, DAY_TICKS},
    storms::Forecast,
    worldmap::{character, climate::BiomeKind, io::load_or_generate, WorldConfig, WorldTerrain},
};

// The moments mapped, in days.
const MOMENTS: [f64; 5] = [0.5, 1.25, 2.5, 4.0, 5.75];
// Pixels across a map.
const PIXELS: usize = 512;

fn biome_colour(kind: BiomeKind) -> [f32; 3] {
    match kind {
        BiomeKind::Sea => [0.10, 0.20, 0.40],
        BiomeKind::Desert => [0.85, 0.75, 0.50],
        BiomeKind::Savanna => [0.75, 0.70, 0.35],
        BiomeKind::Steppe => [0.70, 0.68, 0.45],
        BiomeKind::Grassland => [0.45, 0.65, 0.30],
        BiomeKind::Forest => [0.20, 0.45, 0.20],
        BiomeKind::Jungle => [0.05, 0.35, 0.12],
        BiomeKind::Swamp => [0.25, 0.35, 0.25],
        BiomeKind::Taiga => [0.25, 0.40, 0.35],
        BiomeKind::Tundra => [0.60, 0.62, 0.55],
        BiomeKind::Glacier => [0.92, 0.95, 0.98],
        BiomeKind::Mountain => [0.50, 0.50, 0.50],
    }
}

// Prints where and when it snows longest by day in the cold biomes, with the command to start there.
fn find_snow(
    forecast: &Forecast,
    biome_at: &dyn Fn(f64, f64) -> BiomeKind,
    half: f64,
    seed: u32,
    days: u64,
) {
    // a sample every 0.005 of a day, about 108 seconds
    const STEP: u64 = DAY_TICKS / 200;
    const GRID: usize = 64;
    // how much colder than snowy grass a spot must be, about 100 blocks of height
    const SAFELY_COLD: f64 = 0.03;
    let cell = 2.0 * half / GRID as f64;
    let snowing = |x: f64, z: f64, tick: u64| {
        let sky = forecast.sky(x, z, tick);
        sky.precipitation > 0.5 && sky.snow > 0.9 && daylight(&TimeOfDay(tick)) > 0.9
    };
    // the longest run of snowing samples: its length, where, in which biome, and its first tick
    let mut best: Option<(u64, f64, f64, BiomeKind, u64)> = None;
    for k in (0..GRID * GRID).filter(|k| (k % GRID + k / GRID) % 2 == 0) {
        let x = -half + ((k % GRID) as f64 + 0.5) * cell;
        let z = -half + ((k / GRID) as f64 + 0.5) * cell;
        let biome = biome_at(x, z);
        if !matches!(
            biome,
            BiomeKind::Taiga | BiomeKind::Tundra | BiomeKind::Glacier
        ) {
            continue;
        }
        // well inside the snowy ground, so walking about doesn't cross into rain
        if forecast.climate_at(x, z).1 > SNOWY_GRASS_TEMPERATURE as f64 - SAFELY_COLD {
            continue;
        }
        let (mut run, mut from) = (0u64, 0u64);
        for tick in (0..days * DAY_TICKS).step_by(STEP as usize) {
            if !snowing(x, z, tick) {
                run = 0;
                continue;
            }
            if run == 0 {
                from = tick;
            }
            run += 1;
            if best.is_none_or(|(longest, ..)| run > longest) {
                best = Some((run, x, z, biome, from));
            }
        }
    }
    let Some((run, x, z, biome, from)) = best else {
        println!("seed {seed}: no daytime snowfall in the first {days} days");
        return;
    };
    let minutes = run * STEP / 20 / 60;
    let time = from as f64 / DAY_TICKS as f64;
    println!(
        "seed {seed}: snow falls for {minutes} game minutes by day over the {biome:?} at {x:.0}, {z:.0}, from day {time:.3}"
    );
    let sky = forecast.sky(x, z, from + run * STEP / 2);
    let (_, warmth) = forecast.climate_at(x, z);
    println!(
        "  warmth {warmth:.3}; mid-snowfall precipitation {:.2}, share that's snow {:.2}",
        sky.precipitation, sky.snow
    );
    println!(
        "  OCHRE_SEED={seed} OCHRE_SPAWN={x:.0},{z:.0} OCHRE_TIME={time:.3} cargo run --release --example sandbox"
    );
}

fn main() {
    let mut seed = 1u32;
    let mut days = 30u64;
    let mut out = "target/weather_map".to_string();
    let mut snow = false;
    let mut args = std::env::args().skip(1);
    while let Some(flag) = args.next() {
        if flag == "--snow" {
            snow = true;
            continue;
        }
        let value = args
            .next()
            .unwrap_or_else(|| panic!("missing value for {flag}"));
        match flag.as_str() {
            "--seed" => seed = value.parse().expect("--seed must be a u32"),
            "--days" => days = value.parse().expect("--days must be a whole number"),
            "--out" => out = value,
            _ => panic!("unknown flag {flag}"),
        }
    }
    std::fs::create_dir_all(&out).expect("can't make the output directory");
    let dir = std::env::var("OCHRE_WORLD_DIR").unwrap_or_else(|_| "worlds".into());
    let config = character::world_config(seed, WorldConfig::default());
    let map = Arc::new(load_or_generate(Path::new(&dir), seed, config));
    let half = map.config.half_size() as f64;
    let terrain: Arc<dyn TerrainSource> = Arc::new(WorldTerrain::new(map.clone()));
    let (sx, sz, _) = terrain.find_spawn();
    let forecast = Forecast::from_terrain(terrain);
    let wind = forecast.wind();
    println!(
        "seed {seed}: storms drift at {:.1} blocks a second toward ({:.2}, {:.2}); spawn at {sx}, {sz}",
        wind.length(),
        wind.normalize().x,
        wind.normalize().y
    );
    let biome_at = |x: f64, z: f64| {
        let (u, v) = map.to_cell(x, z);
        let max = (map.dim() - 1) as f64;
        let k = map.index(
            u.round().clamp(0.0, max) as usize,
            v.round().clamp(0.0, max) as usize,
        );
        BiomeKind::from_u8(map.biome[k])
    };

    if snow {
        find_snow(&forecast, &biome_at, half, seed, days.min(12));
        return;
    }

    // how often it rains in each biome, and whether it comes in spells: a day is wet if it rains
    // at all, sampled every quarter of a game hour
    const GRID: usize = 32;
    const SAMPLES_A_DAY: u64 = 24;
    // no cloud to speak of below this share of the sky
    const CLEAR_BELOW: f32 = 0.01;
    let cell = 2.0 * half / GRID as f64;
    let corner = DVec2::splat(-half);
    let mut wet_days = vec![vec![false; days as usize]; GRID * GRID];
    let mut cloudy_days = vec![vec![false; days as usize]; GRID * GRID];
    let mut stats = vec![(0usize, 0usize, 0usize); BiomeKind::ALL.len()];
    // per biome, the samples with rain or snow falling, and the ones where it's mostly snow
    let mut falling = vec![(0usize, 0usize); BiomeKind::ALL.len()];
    let biomes: Vec<BiomeKind> = (0..GRID * GRID)
        .map(|k| {
            let p = corner + (DVec2::new((k % GRID) as f64, (k / GRID) as f64) + 0.5) * cell;
            biome_at(p.x, p.y)
        })
        .collect();
    let start = std::time::Instant::now();
    for step in 0..days * SAMPLES_A_DAY {
        let tick = step * DAY_TICKS / SAMPLES_A_DAY;
        for (k, sky) in forecast.map(corner, GRID, cell, tick).iter().enumerate() {
            let raining = sky.precipitation > 0.1;
            let clear = sky.cloud < CLEAR_BELOW;
            let entry = &mut stats[biomes[k] as usize];
            entry.0 += 1;
            entry.1 += raining as usize;
            entry.2 += clear as usize;
            if raining {
                let entry = &mut falling[biomes[k] as usize];
                entry.0 += 1;
                entry.1 += (sky.snow > 0.5) as usize;
            }
            let day = (step / SAMPLES_A_DAY) as usize;
            wet_days[k][day] |= raining;
            cloudy_days[k][day] |= !clear;
        }
    }
    println!(
        "\n{:<10} {:>6} {:>8} {:>9} {:>9} {:>9} {:>7} {:>10}   ({days} days, {:.1?})",
        "biome",
        "cells",
        "raining",
        "wet days",
        "wet run",
        "dry run",
        "clear",
        "clear days",
        start.elapsed()
    );
    for kind in BiomeKind::ALL {
        let cells: Vec<usize> = (0..GRID * GRID).filter(|&k| biomes[k] == kind).collect();
        if cells.is_empty() {
            continue;
        }
        let (samples, raining, clear) = stats[kind as usize];
        let clear_days: usize = cells
            .iter()
            .map(|&k| cloudy_days[k].iter().filter(|&&cloudy| !cloudy).count())
            .sum();
        // runs of wet days and of dry ones, in days, averaged over the biome's cells
        let (mut wet, mut wet_runs, mut dry_runs) = (0usize, Vec::new(), Vec::new());
        for &k in &cells {
            let days = &wet_days[k];
            wet += days.iter().filter(|&&d| d).count();
            let mut run = 1;
            for i in 1..=days.len() {
                if i < days.len() && days[i] == days[i - 1] {
                    run += 1;
                } else {
                    if days[i - 1] {
                        &mut wet_runs
                    } else {
                        &mut dry_runs
                    }
                    .push(run as f64);
                    run = 1;
                }
            }
        }
        let mean = |runs: &[f64]| runs.iter().sum::<f64>() / runs.len().max(1) as f64;
        println!(
            "{:<10} {:>6} {:>7.1}% {:>8.0}% {:>8.1}d {:>8.1}d {:>6.0}% {:>9.0}%",
            format!("{kind:?}"),
            cells.len(),
            100.0 * raining as f64 / samples as f64,
            100.0 * wet as f64 / (cells.len() * days as usize) as f64,
            mean(&wet_runs),
            mean(&dry_runs),
            100.0 * clear as f64 / samples as f64,
            100.0 * clear_days as f64 / (cells.len() * days as usize) as f64,
        );
    }

    println!("\n{:<10} {:>7} {:>16}", "biome", "warmth", "falls as snow");
    for kind in BiomeKind::ALL {
        let cells: Vec<usize> = (0..GRID * GRID).filter(|&k| biomes[k] == kind).collect();
        let (precipitating, snowing) = falling[kind as usize];
        if cells.is_empty() || precipitating == 0 {
            continue;
        }
        let warmth = cells
            .iter()
            .map(|&k| {
                let p = corner + (DVec2::new((k % GRID) as f64, (k / GRID) as f64) + 0.5) * cell;
                forecast.climate_at(p.x, p.y).1
            })
            .sum::<f64>()
            / cells.len() as f64;
        println!(
            "{:<10} {:>7.2} {:>15.0}%",
            format!("{kind:?}"),
            warmth,
            100.0 * snowing as f64 / precipitating as f64
        );
    }

    // what it costs the game: the clouds' 64 by 64 grid round the camera, worked out every half
    // second, and the camera's own weather each frame (nine asks, with the rainbow's look back)
    let around = DVec2::new(sx as f64, sz as f64) - DVec2::splat(32.0 * 1_100.0);
    let _ = forecast.map(around, 64, 1_100.0, 0);
    let start = std::time::Instant::now();
    for k in 0..50u64 {
        let _ = forecast.map(
            around,
            64,
            1_100.0,
            (0.4 * DAY_TICKS as f64) as u64 + k * 200,
        );
    }
    let grid = start.elapsed() / 50;
    let start = std::time::Instant::now();
    for k in 0..900u64 {
        let _ = forecast.sky(
            sx as f64,
            sz as f64,
            (0.6 * DAY_TICKS as f64) as u64 + k * 20,
        );
    }
    let ask = start.elapsed() / 900;
    println!(
        "\ncost: the clouds' grid {grid:.2?}, one ask {ask:.2?} (nine a frame {:.2?})",
        ask * 9
    );

    // maps of the weather at a few moments
    let cell = 2.0 * half / PIXELS as f64;
    let ground: Vec<[f32; 3]> = (0..PIXELS * PIXELS)
        .map(|k| {
            let p = corner + (DVec2::new((k % PIXELS) as f64, (k / PIXELS) as f64) + 0.5) * cell;
            biome_colour(biome_at(p.x, p.y))
        })
        .collect();
    for day in MOMENTS {
        let tick = (day * DAY_TICKS as f64) as u64;
        let skies = forecast.map(corner, PIXELS, cell, tick);
        let mut image = RgbImage::new(PIXELS as u32, PIXELS as u32);
        for (k, sky) in skies.iter().enumerate() {
            let mut c = ground[k];
            for (channel, value) in c.iter_mut().enumerate() {
                // clouds over it, storm cloud darkening them, then rain in blue
                let cloud = 0.92 * (1.0 - 0.75 * sky.storm);
                *value += (cloud - *value) * 0.55 * sky.cloud;
                let blue = [0.15, 0.35, 0.95][channel];
                *value += (blue - *value) * 0.8 * sky.precipitation;
            }
            let rgb = c.map(|v| (v.clamp(0.0, 1.0) * 255.0) as u8);
            image.put_pixel((k % PIXELS) as u32, (k / PIXELS) as u32, Rgb(rgb));
        }
        let to_pixel = |x: f64, z: f64| ((x + half) / cell, (z + half) / cell);
        let mut dot = |x: f64, z: f64, colour: [u8; 3], r: i32| {
            let (px, pz) = to_pixel(x, z);
            for dz in -r..=r {
                for dx in -r..=r {
                    let (x, y) = (px as i32 + dx, pz as i32 + dz);
                    if (0..PIXELS as i32).contains(&x) && (0..PIXELS as i32).contains(&y) {
                        image.put_pixel(x as u32, y as u32, Rgb(colour));
                    }
                }
            }
        };
        dot(sx as f64, sz as f64, [230, 20, 20], 3);
        // the drift, an arrow from the corner
        let from = DVec2::splat(-half + 3_000.0);
        for i in 0..40 {
            let p = from + wind.normalize() * (i as f64 * 150.0);
            dot(p.x, p.y, [255, 255, 255], if i > 35 { 2 } else { 0 });
        }
        let file = format!("{out}/seed{seed}-day{day:.2}.png");
        image.save(&file).expect("can't save the map");
        println!("day {day:.2}: {file}");
    }
}
