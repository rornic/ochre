//! `FlythroughPlugin::flight` flies the keys in a text file, for recording the screen with its sound, see `Flight`.
use std::time::{Duration, Instant};

use bevy::{
    app::AppExit,
    prelude::*,
    render::view::screenshot::{save_to_disk, Screenshot},
    time::TimeUpdateStrategy,
    transform::TransformSystems,
};

use crate::player::{Player, EYE_HEIGHT};
use ochre_terrain::generate::trees::trees_near;
use ochre_terrain::world::World;
use ochre_world::sky::TimeOfDay;

const FPS: u32 = 60;
// Blocks the camera keeps over any block right round it.
const BLOCK_CLEARANCE: f32 = 4.0;
// Blocks a flight keeps over the top of any tree near its path.
const TREE_CLEARANCE: f32 = 6.0;
// Seconds either side the lift over the ground looks for the highest it needs, and eases over, so it rises and settles in long curves.
const LIFT_REACH: f32 = 2.5;

/// Flies a recorded flight, if one is set. Does nothing otherwise.
#[derive(Default)]
pub struct FlythroughPlugin {
    pub flight: Option<FlightConfig>,
}

/// A flight read from a file of keys, see `Flight`.
pub struct FlightConfig {
    pub file: String,
    /// Seconds spent holding the first key while the world round it loads, `FLIGHT_WARMUP` if unset.
    pub warmup: Option<f32>,
    /// With a directory the flight is rendered a frame at a time on a fixed 60 fps clock, a frame saved for each, however
    /// slowly the machine draws, for a picture to put the sound of a real-time run to.
    pub frames: Option<String>,
}

impl Plugin for FlythroughPlugin {
    fn build(&self, app: &mut App) {
        if let Some(FlightConfig { file, warmup, frames }) = &self.flight {
            let keys = std::fs::read_to_string(file)
                .map_err(|e| e.to_string())
                .and_then(|text| parse_keys(&text))
                .unwrap_or_else(|e| panic!("flight {file}: {e}"));
            let warmup = warmup.unwrap_or(FLIGHT_WARMUP);
            let frames = frames.clone();
            if let Some(dir) = &frames {
                std::fs::create_dir_all(dir).expect("flight frame directory");
            }
            app.insert_resource(Flight {
                keys,
                warmup,
                frames,
                frame: None,
                drain: 0,
                started: None,
                flying: None,
                height: Vec::new(),
            })
            .add_systems(PostUpdate, fly_keys.before(TransformSystems::Propagate));
        }
    }
}

/// A flight read from `OCHRE_FLIGHT`: one key a line, `seconds x z height yaw pitch day`, with `#` comments. The camera is `height`
/// blocks over the ground at `x`, `z`, looks `yaw` degrees round from north (90 is east, and it may run past 360 to keep turning) and
/// `pitch` degrees up, with the time of day at `day` (0 is midnight, 0.25 sunrise, 0.5 noon), all eased through the keys. It waits
/// `OCHRE_FLIGHT_WARMUP` seconds first, holding the first key, so the world round it loads, then logs `flythrough: flying` and runs in
/// real time, so the sound and the clouds go as they do in play. With `OCHRE_FLIGHT_FRAMES=<dir>` it renders on a fixed 60 fps clock
/// instead and saves `frame_NNNNN.png` for every frame there. Spawn at the first key's `x z` with `OCHRE_SPAWN`.
#[derive(Resource)]
struct Flight {
    keys: Vec<Key>,
    warmup: f32,
    // where the frames go, in the frame-by-frame mode
    frames: Option<String>,
    // the frame being rendered, once the flight has begun in that mode
    frame: Option<u32>,
    // frames run since the last one was saved, which gives its screenshot time to be written before the game exits
    drain: u32,
    // when the player first existed, which the warmup counts from
    started: Option<Instant>,
    // when the flight began
    flying: Option<Instant>,
    // the camera's height at each sample along the flight, worked out up front
    height: Vec<f32>,
}

// seconds, x, z, blocks over the ground, yaw and pitch in degrees, time of day
type Key = [f32; 7];

// Seconds of warmup, the heights worked out a sample every this many seconds, and how long the last key is held before the game exits.
const FLIGHT_WARMUP: f32 = 25.0;
const HEIGHT_STEP: f32 = 1.0 / 30.0;
const FLIGHT_TAIL: f32 = 1.0;
// Frames the game runs on after the last one is saved, as screenshots are written on other threads.
const DRAIN_FRAMES: u32 = 120;

fn parse_keys(text: &str) -> Result<Vec<Key>, String> {
    let mut keys: Vec<Key> = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let numbers = line
            .split_whitespace()
            .map(str::parse::<f32>)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("line {}: {e}", n + 1))?;
        let key: Key = numbers
            .try_into()
            .map_err(|_| format!("line {}: want seconds x z height yaw pitch day", n + 1))?;
        if keys.last().is_some_and(|last| key[0] <= last[0]) {
            return Err(format!("line {}: seconds must increase", n + 1));
        }
        keys.push(key);
    }
    if keys.len() < 2 {
        return Err("want at least two keys".into());
    }
    Ok(keys)
}

// The keys at `t` seconds: each value on a Hermite curve through its keys, with monotone tangents so it never overshoots a key and
// swings back, and still at the ends.
fn sample_keys(keys: &[Key], t: f32) -> Key {
    let last = keys.len() - 1;
    let i = (0..last).find(|&i| t < keys[i + 1][0]).unwrap_or(last - 1);
    std::array::from_fn(|k| {
        let value = |i: usize| keys[i][k];
        let tangent = |i: usize| {
            if i == 0 || i == last {
                return 0.0;
            }
            let before = (value(i) - value(i - 1)) / (keys[i][0] - keys[i - 1][0]);
            let after = (value(i + 1) - value(i)) / (keys[i + 1][0] - keys[i][0]);
            if before * after <= 0.0 {
                0.0
            } else {
                2.0 * before * after / (before + after)
            }
        };
        let span = keys[i + 1][0] - keys[i][0];
        let s = ((t - keys[i][0]) / span).clamp(0.0, 1.0);
        let (s2, s3) = (s * s, s * s * s);
        value(i) * (2.0 * s3 - 3.0 * s2 + 1.0)
            + tangent(i) * span * (s3 - 2.0 * s2 + s)
            + value(i + 1) * (-2.0 * s3 + 3.0 * s2)
            + tangent(i + 1) * span * (s3 - s2)
    })
}

// Which way the camera looks for a compass `yaw` and `pitch`, in degrees: yaw 0 is north (-z) and 90 is east (+x).
pub(crate) fn facing(yaw: f32, pitch: f32) -> Vec3 {
    let (yaw, pitch) = (yaw.to_radians(), pitch.to_radians());
    Vec3::new(yaw.sin() * pitch.cos(), pitch.sin(), -yaw.cos() * pitch.cos())
}

fn fly_keys(
    mut commands: Commands,
    mut state: ResMut<Flight>,
    world: Res<World>,
    clock: Option<ResMut<TimeOfDay>>,
    mut exit: MessageWriter<AppExit>,
    mut player: Query<&mut Transform, With<Player>>,
    mut camera: Query<&mut Transform, (With<Camera3d>, Without<Player>)>,
) {
    let (Ok(mut body), Ok(mut eye)) = (player.single_mut(), camera.single_mut()) else {
        return;
    };
    let started = *state.started.get_or_insert_with(Instant::now);
    if state.height.is_empty() {
        let end = state.keys[state.keys.len() - 1][0];
        let mut ground = world.region_store.sampler();
        let need: Vec<f32> = (0..=(end / HEIGHT_STEP) as usize + 1)
            .map(|i| {
                let key = sample_keys(&state.keys, i as f32 * HEIGHT_STEP);
                let p = Vec3::new(key[1], 0.0, key[2]);
                (ground_height(&world, p) + key[3]).max(tree_top(&world, &mut ground, p) + TREE_CLEARANCE)
            })
            .collect();
        state.height = smooth_lift(&need, (LIFT_REACH / HEIGHT_STEP) as usize);
    }
    if state.flying.is_none() && started.elapsed().as_secs_f32() >= state.warmup {
        state.flying = Some(Instant::now());
        if state.frames.is_some() {
            commands.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(1.0 / FPS as f64)));
            state.frame = Some(0);
        }
        info!("flythrough: flying");
    }
    let t = match state.frame {
        Some(frame) => frame as f32 / FPS as f32,
        None => state.flying.map_or(0.0, |at| at.elapsed().as_secs_f32()),
    };
    let end = state.keys[state.keys.len() - 1][0];
    // the frames end with the last key, the real-time run a moment after it so the sound has its tail
    let finish = if state.frames.is_some() { end } else { end + FLIGHT_TAIL };
    if t > finish {
        state.drain += 1;
        if state.frames.is_none() || state.drain > DRAIN_FRAMES {
            exit.write(AppExit::Success);
        }
        return;
    }
    let key = sample_keys(&state.keys, t.min(end));
    let at_sample = (t.min(end) / HEIGHT_STEP).min((state.height.len() - 1) as f32);
    let (a, b) = (at_sample.floor() as usize, (at_sample.ceil() as usize).min(state.height.len() - 1));
    let height = state.height[a] + (state.height[b] - state.height[a]) * (at_sample - a as f32);
    let mut at = Vec3::new(key[1], height, key[2]);
    // and never in a block the ground's height missed, a tree or an overhang
    at.y = at.y.max(block_height(&world, at));
    let look = facing(key[4], key[5]);
    body.translation = at - Vec3::Y * EYE_HEIGHT;
    body.rotation = Quat::from_rotation_y(f32::atan2(-look.x, -look.z));
    eye.translation = Vec3::Y * EYE_HEIGHT;
    eye.rotation = Quat::from_rotation_x(look.y.clamp(-1.0, 1.0).asin());
    if let Some(mut clock) = clock {
        *clock = TimeOfDay::from_days(key[6]);
    }
    if let (Some(dir), Some(frame)) = (&state.frames, state.frame) {
        let file = format!("{dir}/frame_{frame:05}.png");
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(file));
        state.frame = Some(frame + 1);
    }
}

// The highest ground or water round `p`'s column, from the terrain itself, as the blocks far along the path aren't loaded yet.
pub(crate) fn ground_height(world: &World, p: Vec3) -> f32 {
    let terrain = world.region_store.terrain();
    let mut top = f32::MIN;
    for dx in [-8.0, 0.0, 8.0] {
        for dz in [-8.0, 0.0, 8.0] {
            let sample = terrain.sample((p.x + dx) as f64, (p.z + dz) as f64);
            top = top.max((sample.height as f32).max(sample.water_surface));
        }
    }
    top
}

// The top of the highest tree that could reach the column at `p`, as the terrain would grow them.
fn tree_top(world: &World, ground: &mut ochre_terrain::generate::region_store::HeightSampler, p: Vec3) -> f32 {
    let at = bevy::math::I64Vec2::new(p.x.round() as i64, p.z.round() as i64);
    trees_near(world.region_store.terrain(), ground, at, at).iter().map(|tree| tree.crown_top() as f32).fold(f32::MIN, f32::max)
}

// How high the camera must be over any solid block right round `p`, trees included.
fn block_height(world: &World, p: Vec3) -> f32 {
    let mut floor = f32::MIN;
    for dx in -1..=1 {
        for dz in -1..=1 {
            let (x, z) = (p.x.round() as i64 + dx, p.z.round() as i64 + dz);
            let top = (p.y.round() as i64 - 4..=p.y.round() as i64 + 16)
                .rev()
                .find(|&y| {
                    world
                        .get_block_at(bevy::math::I64Vec3::new(x, y, z))
                        .is_solid()
                });
            if let Some(y) = top {
                floor = floor.max(y as f32 + 0.5 + BLOCK_CLEARANCE);
            }
        }
    }
    floor
}

// The lift over each frame: never under what it `need`s, the highest need within `reach` frames either side, eased twice over that span so it curves in and out.
fn smooth_lift(need: &[f32], reach: usize) -> Vec<f32> {
    let window = |i: usize| i.saturating_sub(reach)..(i + reach + 1).min(need.len());
    let mut lift: Vec<f32> = (0..need.len())
        .map(|i| need[window(i)].iter().copied().fold(0.0, f32::max))
        .collect();
    // each frame of two means over half the span still only draws on frames whose highest covers its own need
    let half = reach / 2;
    for _ in 0..2 {
        lift = (0..lift.len())
            .map(|i| {
                let span = i.saturating_sub(half)..(i + half + 1).min(lift.len());
                lift[span.clone()].iter().sum::<f32>() / span.len() as f32
            })
            .collect();
    }
    lift
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_a_flight_file_reads_keys_and_says_what_is_wrong_with_a_bad_one() {
        let keys = parse_keys("# a flight\n0 10 20 5 0 0 0.1\n\n4 30 20 5 90 10 0.2 # east\n").unwrap();
        assert_eq!(keys, vec![[0.0, 10.0, 20.0, 5.0, 0.0, 0.0, 0.1], [4.0, 30.0, 20.0, 5.0, 90.0, 10.0, 0.2]]);
        assert!(parse_keys("0 1 2 3 4 5").unwrap_err().contains("line 1"));
        assert!(parse_keys("0 1 2 3 4 5 x").unwrap_err().contains("line 1"));
        assert!(parse_keys("0 1 2 3 4 5 6\n0 1 2 3 4 5 6").unwrap_err().contains("line 2"));
        assert!(parse_keys("0 1 2 3 4 5 6").unwrap_err().contains("two keys"));
    }

    #[test]
    fn test_a_flight_passes_through_its_keys_and_never_overshoots_between() {
        let keys = vec![
            [0.0, 0.0, 0.0, 5.0, 0.0, 0.0, 0.10],
            [4.0, 80.0, -40.0, 6.0, 90.0, 20.0, 0.15],
            [9.0, 90.0, -50.0, 3.0, 360.0, -10.0, 0.22],
        ];
        for key in &keys {
            let at = sample_keys(&keys, key[0]);
            assert!(at.iter().zip(key).all(|(a, b)| (a - b).abs() < 1e-3), "{at:?} {key:?}");
        }
        for pair in keys.windows(2) {
            for step in 0..=40 {
                let t = pair[0][0] + (pair[1][0] - pair[0][0]) * step as f32 / 40.0;
                let at = sample_keys(&keys, t);
                for k in 1..7 {
                    let (lo, hi) = (pair[0][k].min(pair[1][k]), pair[0][k].max(pair[1][k]));
                    assert!((lo - 1e-3..=hi + 1e-3).contains(&at[k]), "value {k} at {t}");
                }
            }
        }
        // the time of day only ever runs forward through a flight whose keys do
        let mut last = 0.0;
        for step in 0..=90 {
            let day = sample_keys(&keys, step as f32 / 10.0)[6];
            assert!(day >= last - 1e-6);
            last = day;
        }
    }

    #[test]
    fn test_yaw_is_a_compass_bearing_with_north_down_the_negative_z_axis() {
        let close = |a: Vec3, b: Vec3| a.distance(b) < 1e-5;
        assert!(close(facing(0.0, 0.0), Vec3::NEG_Z));
        assert!(close(facing(90.0, 0.0), Vec3::X));
        assert!(close(facing(180.0, 0.0), Vec3::Z));
        assert!(close(facing(270.0, 0.0), Vec3::NEG_X));
        assert!(close(facing(0.0, 90.0), Vec3::Y));
        assert!(close(facing(360.0, 30.0), facing(0.0, 30.0)));
    }

    #[test]
    fn test_lift_clears_every_need_and_changes_gently() {
        // flat, then a sharp spike and a jagged ridge, as rough ground under a straight path
        let need: Vec<f32> = (0..600)
            .map(|i| match i {
                100 => 80.0,
                300..=400 => 40.0 + 30.0 * ((i % 7) as f32 / 7.0),
                _ => -20.0,
            })
            .collect();
        let lift = smooth_lift(&need, 150);
        assert!(need.iter().zip(&lift).all(|(n, l)| l >= n && *l >= 0.0));
        let steepest = lift
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0, f32::max);
        assert!(steepest < 1.5, "{steepest} blocks in a frame");
    }
}
