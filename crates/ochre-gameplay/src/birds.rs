//! Birds by day: a murmuration of starlings wheeling round the camera, and now and then a
//! skein of geese in a V. They roost at night and shelter from the rain.
use std::f32::consts::TAU;

use bevy::{math::ops, prelude::*};

use crate::wings::{spawn_winged, wing_mesh, Flap, WingPlugin};
use ochre_base::{math::ease, WATER_LEVEL};
use ochre_terrain::world::World;
use ochre_world::{
    sky::{daylight, TimeOfDay},
    weather::SkyNow,
};

pub struct BirdPlugin;

impl Plugin for BirdPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<WingPlugin>() {
            app.add_plugins(WingPlugin);
        }
        app.init_resource::<TimeOfDay>()
            .init_resource::<Murmuration>()
            .add_systems(Startup, make_bird_assets)
            .add_systems(Update, (sense_bird_life, gather_flock, flock, fly_geese).chain());
    }
}

/// How much bird life a biome has: how many starlings are in the flock, how far round the camera it wheels and how high over the
/// ground or sea, in blocks, and how often, how big and how high a skein of geese passes.
#[derive(Clone, Copy, Debug, PartialEq)]
struct BirdLife {
    starlings: usize,
    range: f32,
    height: f32,
    skein_seconds: (f32, f32),
    skein_size: (u32, u32),
    skein_height: f32,
}

// Grassland's, and the base terrain's: the bird life before biomes had any say.
const BASE: BirdLife = BirdLife {
    starlings: 160,
    range: 55.0,
    height: 34.0,
    skein_seconds: (45.0, 110.0),
    skein_size: (7, 12),
    skein_height: 48.0,
};

impl Default for BirdLife {
    fn default() -> Self {
        BASE
    }
}

// What each biome's sky holds, by name. Open country has wide skies and passing geese, wet country fowl, and woods, jungle, desert and
// ice few of either: crowded canopy and bare ground hold fewer starlings, and geese cross them high or seldom. A glacier has no flock.
fn bird_life(biome: Option<&str>) -> BirdLife {
    let life = |starlings, range, height, skein_seconds, skein_size, skein_height| BirdLife {
        starlings,
        range,
        height,
        skein_seconds,
        skein_size,
        skein_height,
    };
    match biome {
        Some("forest") => life(70, 45.0, 38.0, (90.0, 220.0), (7, 12), 60.0),
        Some("jungle") => life(40, 40.0, 40.0, (200.0, 420.0), (6, 10), 65.0),
        Some("swamp") => life(110, 55.0, 30.0, (28.0, 70.0), (8, 13), 40.0),
        Some("savanna") => life(110, 75.0, 40.0, (110.0, 260.0), (7, 12), 55.0),
        Some("desert") => life(20, 85.0, 45.0, (300.0, 600.0), (5, 9), 80.0),
        Some("steppe") => life(60, 85.0, 42.0, (25.0, 60.0), (9, 14), 70.0),
        Some("taiga") => life(50, 50.0, 36.0, (50.0, 130.0), (7, 12), 55.0),
        Some("tundra") => life(25, 70.0, 32.0, (30.0, 75.0), (9, 14), 45.0),
        Some("glacier") => life(0, 60.0, 40.0, (400.0, 800.0), (5, 8), 80.0),
        _ => BASE,
    }
}

// How often the flock is brought toward its biome's size, in seconds, and by how many birds a time.
const ADJUST_SECONDS: f32 = 0.25;
const ADJUST_BIRDS: usize = 3;
// The starlings' speeds, in blocks a second, and how near they keep with and clear of others.
const SPEED: (f32, f32) = (9.0, 15.0);
const NEIGHBOURS: f32 = 7.0;
const PERSONAL_SPACE: f32 = 3.2;
// How hard each starling steers to keep with its neighbours' heading, toward their middle,
// away from those too near, and toward where the flock's heading.
const ALIGN: f32 = 1.4;
const COHERE: f32 = 0.25;
const SEPARATE: f32 = 9.0;
const HOME: f32 = 0.55;
// How long the flock takes to gather or leave, in seconds.
const FADE_SECONDS: f32 = 3.0;
// How much of the day it is, see `daylight`, that birds are about in.
const BIRD_DAY: f32 = 0.5;

#[derive(Resource)]
struct BirdAssets {
    starling_wing: Handle<Mesh>,
    starling_body: Handle<Mesh>,
    starling: Handle<StandardMaterial>,
    goose_wing: Handle<Mesh>,
    goose_body: Handle<Mesh>,
    goose: Handle<StandardMaterial>,
    goose_wings: Handle<StandardMaterial>,
}

/// The flock, and how far it's gathered, 0..1.
#[derive(Resource, Default)]
struct Murmuration {
    gathered: f32,
    // whether it's the time and weather for birds, or they're leaving
    about: bool,
    // where it's heading round the camera, which wanders on so the flock never settles
    clock: f32,
    // what the biome under the camera holds, looked at every half second, and the seconds since
    life: BirdLife,
    sensed: f32,
    adjusted: f32,
}

#[derive(Component)]
struct Starling {
    velocity: Vec3,
}

#[derive(Component)]
struct Goose {
    velocity: Vec3,
    travelled: f32,
    phase: f32,
}

fn make_bird_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let matte = |materials: &mut Assets<StandardMaterial>, colour: Color| {
        materials.add(StandardMaterial {
            base_color: colour,
            perceptual_roughness: 0.95,
            double_sided: true,
            cull_mode: None,
            ..default()
        })
    };
    commands.insert_resource(BirdAssets {
        starling_wing: meshes.add(swept_wing(0.42, 0.16)),
        starling_body: meshes.add(Cuboid::new(0.1, 0.08, 0.32)),
        starling: matte(&mut materials, Color::srgb(0.05, 0.05, 0.06)),
        goose_wing: meshes.add(swept_wing(0.95, 0.32)),
        goose_body: meshes.add(Cuboid::new(0.22, 0.2, 0.9)),
        goose: matte(&mut materials, Color::srgb(0.32, 0.28, 0.24)),
        goose_wings: matte(&mut materials, Color::srgb(0.42, 0.38, 0.33)),
    });
}

// A swept-back wing `span` long and `chord` deep at the body.
fn swept_wing(span: f32, chord: f32) -> Mesh {
    wing_mesh([
        [0.0, 0.0, -chord * 0.5],
        [span * 0.55, 0.0, -chord * 0.15],
        [0.0, 0.0, chord * 0.5],
        [span, 0.0, chord * 0.55],
    ])
}

fn birds_about(sky: &SkyNow) -> bool {
    daylight(&sky.clock) >= BIRD_DAY && sky.weather.as_ref().is_none_or(|weather| !weather.sheltering())
}

// The biome under the camera says how much bird life there is, looked at every half second.
fn sense_bird_life(
    time: Res<Time>,
    world: Res<World>,
    camera: Query<&GlobalTransform, With<Camera3d>>,
    mut murmuration: ResMut<Murmuration>,
) {
    let Ok(camera) = camera.single() else {
        return;
    };
    murmuration.sensed += time.delta_secs();
    if murmuration.sensed >= 0.5 {
        murmuration.sensed = 0.0;
        let eye = camera.translation();
        let noise = world.region_store.terrain();
        let here = noise.sample(eye.x as f64, eye.z as f64);
        murmuration.life = bird_life(noise.biomes().name(here.biome));
    }
}

// The flock gathers round the camera by day and leaves at dusk or in the rain.
fn gather_flock(
    mut commands: Commands,
    time: Res<Time>,
    sky: SkyNow,
    assets: Option<Res<BirdAssets>>,
    mut murmuration: ResMut<Murmuration>,
    camera: Query<&GlobalTransform, With<Camera3d>>,
    starlings: Query<(Entity, &Transform, &Starling)>,
) {
    let (Some(assets), Ok(camera)) = (assets, camera.single()) else {
        return;
    };
    let eye = camera.translation();
    let life = murmuration.life;
    let about = birds_about(&sky) && life.starlings > 0;
    murmuration.about = about;
    let step = time.delta_secs() / FADE_SECONDS;
    murmuration.gathered =
        (murmuration.gathered + if about { step } else { -step }).clamp(0.0, 1.0);
    let count = starlings.iter().count();
    if about && count == 0 {
        // they arrive together, from somewhere off to one side
        let angle = rand::random_range(0.0..TAU);
        let centre = eye
            + Vec3::new(ops::cos(angle), 0.0, ops::sin(angle)) * life.range
            + Vec3::Y * life.height;
        let heading = Vec3::new(-ops::sin(angle), 0.0, ops::cos(angle));
        for _ in 0..life.starlings {
            let at = centre
                + Vec3::new(
                    rand::random_range(-4.0..4.0),
                    rand::random_range(-2.0..2.0),
                    rand::random_range(-4.0..4.0),
                );
            spawn_starling(&mut commands, &assets, at, heading * SPEED.0);
        }
    } else if about && count > 0 {
        // crossing into another biome, the flock shrinks or grows toward its size a few birds at a time
        murmuration.adjusted += time.delta_secs();
        if murmuration.adjusted >= ADJUST_SECONDS {
            murmuration.adjusted = 0.0;
            if count > life.starlings {
                for (entity, _, _) in starlings.iter().take((count - life.starlings).min(ADJUST_BIRDS)) {
                    commands.entity(entity).despawn();
                }
            } else if let Some((_, transform, starling)) = starlings.iter().next().filter(|_| count < life.starlings) {
                for _ in 0..(life.starlings - count).min(ADJUST_BIRDS) {
                    let jitter = Vec3::new(rand::random_range(-3.0..3.0), rand::random_range(-1.5..1.5), rand::random_range(-3.0..3.0));
                    spawn_starling(&mut commands, &assets, transform.translation + jitter, starling.velocity);
                }
            }
        }
    } else if !about && murmuration.gathered <= 0.0 {
        for (starling, _, _) in &starlings {
            commands.entity(starling).despawn();
        }
    }
}

// One starling at `at`, flying with `velocity`: quick beats, now and then gliding with its wings out.
fn spawn_starling(commands: &mut Commands, assets: &BirdAssets, at: Vec3, velocity: Vec3) {
    spawn_winged(
        commands,
        Starling { velocity },
        at,
        (&assets.starling_body, &assets.starling),
        (&assets.starling_wing, &assets.starling),
        Flap {
            beat: rand::random_range(16.0..20.0),
            sweep: 0.8,
            lift: 0.1,
            glides: true,
        },
    );
}

/// Where the flock heads at `t` seconds, wandering round `eye` at about `range` blocks, far enough off to be seen whole.
pub(crate) fn flock_heading(eye: Vec3, t: f32, range: f32) -> Vec3 {
    let a = t * 0.11 + 0.8 * ops::sin(t * 0.05);
    let radius = range * (0.75 + 0.25 * ops::sin(t * 0.17));
    eye + Vec3::new(ops::cos(a) * radius, 0.0, ops::sin(a) * radius)
}

// Each starling heads with its neighbours, toward their middle and out of their way, while
// all of them steer after the flock's heading. Nothing leads, so it wheels and folds.
fn flock(
    time: Res<Time>,
    world: Res<World>,
    mut murmuration: ResMut<Murmuration>,
    camera: Query<&GlobalTransform, With<Camera3d>>,
    mut starlings: Query<(&mut Starling, &mut Transform)>,
) {
    let Ok(camera) = camera.single() else {
        return;
    };
    let dt = time.delta_secs().min(0.05);
    murmuration.clock += dt;
    let eye = camera.translation();
    let heading = flock_heading(eye, murmuration.clock, murmuration.life.range);
    let ground = world
        .region_store
        .get_height(heading.x.floor() as i64, heading.z.floor() as i64)
        .max(WATER_LEVEL as f64) as f32;
    let mut target = heading
        .with_y(ground.max(eye.y - 20.0) + murmuration.life.height + 6.0 * ops::sin(murmuration.clock * 0.3));
    // leaving, they make off together, up and away, fading as they go
    if !murmuration.about {
        target = eye + (heading - eye).with_y(0.0).normalize_or(Vec3::X) * 400.0 + Vec3::Y * 120.0;
    }

    let birds: Vec<(Vec3, Vec3)> = starlings
        .iter()
        .map(|(starling, transform)| (transform.translation, starling.velocity))
        .collect();
    let size = ease(murmuration.gathered);
    for (i, (mut starling, mut transform)) in starlings.iter_mut().enumerate() {
        let (here, velocity) = birds[i];
        let steer = steering(i, &birds, target);
        let mut velocity = velocity + steer * dt;
        let speed = velocity.length().clamp(SPEED.0, SPEED.1);
        velocity = velocity.normalize_or(Vec3::X) * speed;
        // never down into the land
        if here.y < ground + 8.0 {
            velocity.y = velocity.y.max(3.0);
        }
        starling.velocity = velocity;
        transform.translation = here + velocity * dt;
        transform.look_to(velocity, Vec3::Y);
        // banking into its turns
        let bank = steer.dot(transform.right().as_vec3()) * 0.04;
        transform.rotate_local_z(-bank.clamp(-0.9, 0.9));
        transform.scale = Vec3::splat(size);
    }
}

/// How hard bird `i` steers, in blocks a second squared, given every bird's place and velocity.
pub(crate) fn steering(i: usize, birds: &[(Vec3, Vec3)], target: Vec3) -> Vec3 {
    let (here, velocity) = birds[i];
    let (mut heading, mut middle, mut away, mut count) = (Vec3::ZERO, Vec3::ZERO, Vec3::ZERO, 0.0);
    for (j, &(there, their_velocity)) in birds.iter().enumerate() {
        if j == i {
            continue;
        }
        let between = there - here;
        let distance_squared = between.length_squared();
        if distance_squared < NEIGHBOURS * NEIGHBOURS {
            heading += their_velocity;
            middle += there;
            count += 1.0;
            if distance_squared < PERSONAL_SPACE * PERSONAL_SPACE {
                away -= between / distance_squared.max(0.01);
            }
        }
    }
    let mut steer = (target - here).normalize_or_zero() * SPEED.1 * HOME;
    if count > 0.0 {
        steer += (heading / count - velocity) * ALIGN;
        steer += (middle / count - here) * COHERE;
    }
    steer + away * SEPARATE
}

// How soon the first skein flies over, in seconds, and how fast and far geese fly, in blocks; how often, how big and how high a skein
// is comes from the biome, see `BirdLife`.
const FIRST_SKEIN_IN: f32 = 12.0;
const GOOSE_SPEED: f32 = 11.0;
const SKEIN_REACH: f32 = 230.0;

// What a skein is drawn and sized from.
#[derive(bevy::ecs::system::SystemParam)]
struct SkeinParts<'w> {
    assets: Option<Res<'w, BirdAssets>>,
    murmuration: Res<'w, Murmuration>,
}

fn fly_geese(
    mut commands: Commands,
    time: Res<Time>,
    sky: SkyNow,
    parts: SkeinParts,
    camera: Query<&GlobalTransform, With<Camera3d>>,
    mut geese: Query<(Entity, &mut Goose, &mut Transform)>,
    mut next: Local<Option<f32>>,
) {
    let SkeinParts { assets, murmuration } = parts;
    let (Some(assets), Ok(camera)) = (assets, camera.single()) else {
        return;
    };
    let dt = time.delta_secs();
    let t = time.elapsed_secs();
    for (entity, mut goose, mut transform) in &mut geese {
        goose.travelled += GOOSE_SPEED * dt;
        transform.translation += goose.velocity * dt;
        // a gentle rise and fall, each in its own time
        transform.translation.y += ops::sin(t * 1.1 + goose.phase) * 0.15 * dt;
        let grown = (goose.travelled / 20.0).min(1.0);
        let gone = ((2.0 * SKEIN_REACH - goose.travelled) / 20.0).clamp(0.0, 1.0);
        transform.scale = Vec3::splat(grown.min(gone));
        transform.look_to(goose.velocity, Vec3::Y);
        if goose.travelled > 2.0 * SKEIN_REACH {
            commands.entity(entity).despawn();
        }
    }

    let due = next.get_or_insert(t + FIRST_SKEIN_IN);
    if t < *due {
        return;
    }
    let life = murmuration.life;
    *due = t + rand::random_range(life.skein_seconds.0..life.skein_seconds.1);
    if !birds_about(&sky) {
        return;
    }
    // across the camera's view, passing a little to one side of it
    let eye = camera.translation();
    let ahead = camera.forward().with_y(0.0).normalize_or(Vec3::Z);
    let across =
        Vec3::new(-ahead.z, 0.0, ahead.x) * if rand::random::<bool>() { 1.0 } else { -1.0 };
    let course = (across + ahead * rand::random_range(-0.4..0.4)).normalize();
    let passing = eye + ahead * rand::random_range(40.0..90.0) + Vec3::Y * life.skein_height;
    let start = passing - course * SKEIN_REACH;
    let side = Vec3::new(-course.z, 0.0, course.x);
    let count: u32 = rand::random_range(life.skein_size.0..life.skein_size.1);
    for k in 0..count {
        // the leader at the point, the rest down either arm of the V
        let rank = k.div_ceil(2) as f32;
        let arm = if k % 2 == 0 { 1.0 } else { -1.0 };
        let at = start - course * rank * 2.4 + side * arm * rank * 2.0 + Vec3::Y * rank * 0.3;
        spawn_winged(
            &mut commands,
            Goose {
                velocity: course * GOOSE_SPEED,
                travelled: 0.0,
                phase: rand::random_range(0.0..TAU),
            },
            at,
            (&assets.goose_body, &assets.goose),
            (&assets.goose_wing, &assets.goose_wings),
            // slow, deep strokes
            Flap {
                beat: rand::random_range(5.5..6.5),
                sweep: 0.55,
                lift: 0.1,
                glides: true,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BIOMES: [&str; 10] = ["grassland", "forest", "jungle", "swamp", "savanna", "desert", "steppe", "taiga", "tundra", "glacier"];

    #[test]
    fn grassland_and_the_base_terrain_keep_the_bird_life_there_always_was() {
        assert_eq!(BASE, bird_life(None));
        assert_eq!(BASE, bird_life(Some("grassland")));
        assert_eq!((160, 55.0, 34.0), (BASE.starlings, BASE.range, BASE.height));
        assert_eq!(((45.0, 110.0), (7, 12), 48.0), (BASE.skein_seconds, BASE.skein_size, BASE.skein_height));
    }

    #[test]
    fn every_biome_has_sane_bird_life() {
        for name in BIOMES {
            let life = bird_life(Some(name));
            assert!(life.range >= 30.0 && life.height >= 25.0, "{name}");
            assert!(life.skein_seconds.0 > 0.0 && life.skein_seconds.0 < life.skein_seconds.1, "{name}");
            assert!(life.skein_size.0 >= 3 && life.skein_size.0 < life.skein_size.1, "{name}");
            assert!(life.skein_height > life.height, "{name}: geese fly over the flock");
        }
        assert_eq!(BASE, bird_life(Some("a biome nobody has named")));
    }

    #[test]
    fn open_country_has_wide_skies_and_busy_geese_and_woods_jungle_desert_and_ice_have_few_birds() {
        let mean = |life: BirdLife| (life.skein_seconds.0 + life.skein_seconds.1) / 2.0;
        let (steppe, tundra, swamp) = (bird_life(Some("steppe")), bird_life(Some("tundra")), bird_life(Some("swamp")));
        for open in [steppe, tundra, swamp] {
            assert!(mean(open) < 0.7 * mean(BASE), "geese are busy over open and wet country");
        }
        assert!(steppe.range > BASE.range && steppe.skein_size.1 > BASE.skein_size.1, "steppe has a wide sky and big skeins");
        for shut in ["forest", "jungle", "desert"] {
            let life = bird_life(Some(shut));
            assert!(life.starlings * 2 < BASE.starlings && mean(life) > 1.5 * mean(BASE), "{shut}");
        }
        assert_eq!(0, bird_life(Some("glacier")).starlings);
    }

    #[test]
    fn test_crowded_birds_push_apart_and_lone_ones_head_home() {
        let target = Vec3::new(100.0, 0.0, 0.0);
        let lone = [(Vec3::ZERO, Vec3::X * 10.0)];
        assert!(steering(0, &lone, target).x > 0.0);
        let crowded = [
            (Vec3::ZERO, Vec3::Z * 10.0),
            (Vec3::new(0.0, 0.0, 0.5), Vec3::Z * 10.0),
        ];
        // the one behind is pushed back, away from the one just ahead
        assert!(steering(0, &crowded, Vec3::ZERO).z < 0.0);
    }

    #[test]
    fn test_neighbours_turn_a_bird_to_their_heading() {
        let mut birds = vec![(Vec3::ZERO, Vec3::X * 10.0)];
        for k in 0..6 {
            birds.push((Vec3::new(k as f32 * 0.4 - 1.0, 2.0, 0.0), Vec3::Z * 10.0));
        }
        let steer = steering(0, &birds, Vec3::ZERO);
        assert!(steer.z > 0.0 && steer.x < 0.0, "{steer}");
    }
}
