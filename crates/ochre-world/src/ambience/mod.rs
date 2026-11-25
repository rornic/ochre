//! What the world sounds like at the camera: wind, forest, cricket and wave loops, and the odd owl.
mod haze;
mod wind;

use std::f32::consts::TAU;

use bevy::{
    app::{App, Plugin, Startup, Update},
    asset::{AssetServer, Handle},
    audio::{
        AddAudioSource, AudioPlayer, AudioSink, AudioSinkPlayback, AudioSource, PlaybackSettings,
        SpatialAudioSink, SpatialListener, SpatialScale, Volume,
    },
    camera::Camera3d,
    ecs::{
        component::Component,
        entity::Entity,
        query::{With, Without},
        resource::Resource,
        schedule::{IntoScheduleConfigs, SystemSet},
        system::{Commands, Local, Query, Res, ResMut},
    },
    math::{ops, Vec3},
    time::Time,
    transform::components::{GlobalTransform, Transform},
};

use crate::atmosphere::{EasedFeel, Feel};
use crate::sky::{sun_direction, TimeOfDay};
pub(crate) use ochre_base::math::smoothstep;
use ochre_base::{block::BlockType, WATER_LEVEL};
use ochre_terrain::world::World;
use wind::Gust;
pub(crate) use wind::SAMPLE_RATE;
pub use wind::{wind_strength, Wind, WindDials};

/// Wind, loops and the odd owl, each as loud as the camera's `Surroundings` call for.
pub struct AmbiencePlugin;

impl Plugin for AmbiencePlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<SurroundingsPlugin>() {
            app.add_plugins(SurroundingsPlugin);
        }
        app.add_audio_source::<Wind>()
            .add_systems(Startup, (wind::play_wind, play_loops))
            .add_systems(
                Update,
                (
                    listen_from_camera,
                    (
                        wind::update_wind,
                        perch_loops.before(mix_loops),
                        mix_loops,
                        hoot,
                    )
                        .after(SenseSurroundings),
                ),
            );
    }
}

/// Keeps `Surroundings` current. Added by `AmbiencePlugin`, or by a game's own plugin that reads it.
pub struct SurroundingsPlugin;

impl Plugin for SurroundingsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TimeOfDay>()
            .init_resource::<WindDials>()
            .init_resource::<Surroundings>()
            .init_resource::<EasedFeel>()
            .add_systems(Update, sense_surroundings.in_set(SenseSurroundings))
            .add_systems(Update, ease_feel.after(SenseSurroundings))
            .add_systems(Update, haze::sense_haze.after(ease_feel));
    }
}

// Seconds for the feel to cover most of the way to the country the camera is in.
const FEEL_EASE: f32 = 2.0;

fn ease_feel(around: Res<Surroundings>, time: Res<Time>, mut eased: ResMut<EasedFeel>) {
    let step = 1.0 - (-time.delta_secs() / FEEL_EASE).exp();
    eased.0.ease_towards(around.feel, step);
}

/// The system that updates `Surroundings`, for systems that read it to run after.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct SenseSurroundings;

/// What's round the camera, as far as what it hears goes. The fractions are 0..1.
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct Surroundings {
    // how high the camera is over the ground or sea, in blocks
    pub altitude: f32,
    // how fast it's moving, in blocks a second
    pub speed: f32,
    pub underwater: bool,
    // how much of what's round it is sea; the rest is land
    pub water: f32,
    // how thick the forest round it is, and right round it, under the trees' crowns
    pub trees: f32,
    pub trees_overhead: f32,
    // 0 where it's cold enough to freeze, up to 1 where it's mild
    pub warmth: f32,
    // how much of each kind of country it is round the camera, see `atmosphere::Feel`
    pub feel: Feel,
    // 1 over desert
    pub desert: f32,
    // how much of what's round it is steppe, 0..1
    pub steppe: f32,
    // how far the ground stands over its surroundings, 0..1: 0.5 on level ground, up on crests, down in hollows
    pub exposure: f32,
    // the climate biome under the camera, None over the base terrain
    pub biome: Option<&'static str>,
    // 1 by day, 0 by night, in between at dawn and dusk
    pub day: f32,
    // how much of a breeze is blowing, see `WindDials`
    pub breeze: f32,
}

impl Default for Surroundings {
    fn default() -> Self {
        Self {
            altitude: 0.0,
            speed: 0.0,
            underwater: false,
            water: 0.0,
            trees: 0.0,
            trees_overhead: 0.0,
            warmth: 1.0,
            feel: Feel::default(),
            desert: 0.0,
            steppe: 0.0,
            exposure: 0.5,
            biome: None,
            day: 1.0,
            breeze: 0.0,
        }
    }
}

// Seconds between samples of the terrain round the camera; the loops glide to new volumes anyway.
const SENSE_SECONDS: f32 = 0.25;
// Rings of columns round the camera: radius in blocks, weight per column; the camera's column weighs as the inner
const RINGS: [(f32, f32); 2] = [(24.0, 2.0), (64.0, 1.0)];
const RING_COLUMNS: usize = 8;
const CENTRE_WEIGHT: f32 = 2.0;
// The tree density, see `Biome::tree_density`, that sounds like the depths of a forest.
const DENSE_FOREST: f64 = 0.4;
// Every ambient sound's share of its own level (-10 dB), so the whole bed sits under the music.
pub(crate) const AMBIENCE_TRIM: f32 = 0.32;
// The temperatures, see `NoiseGenerator::temperature`, between which the cold sets in.
const FREEZING: (f32, f32) = (0.15, 0.35);

/// When the camera was last sensed, and where it was, for its speed.
#[derive(Default)]
struct Sensed {
    since: f32,
    last: Option<Vec3>,
}

fn sense_surroundings(
    time: Res<Time>,
    world: Res<World>,
    clock: Res<TimeOfDay>,
    dials: Res<WindDials>,
    camera: Query<&GlobalTransform, With<Camera3d>>,
    mut around: ResMut<Surroundings>,
    mut sensed: Local<Sensed>,
) {
    let Ok(camera) = camera.single() else {
        return;
    };
    sensed.since += time.delta_secs();
    if sensed.last.is_some() && sensed.since < SENSE_SECONDS {
        return;
    }
    let eye = camera.translation();
    let speed = match sensed.last.replace(eye) {
        Some(last) if sensed.since > 0.0 => last.distance(eye) / sensed.since,
        _ => 0.0,
    };
    sensed.since = 0.0;

    let store = &world.region_store;
    let noise = store.terrain();
    let ground = store
        .get_height(eye.x.floor() as i64, eye.z.floor() as i64)
        .max(WATER_LEVEL as f64) as f32;
    let (mut water, mut trees, mut total, mut steppe) = (0.0, 0.0, 0.0, 0.0);
    let (mut overhead, mut overhead_total) = (0.0, 0.0);
    // the camera's own column and the inner ring count as overhead
    let mut sample = |x: f32, z: f32, weight: f32, is_overhead: bool| {
        let column = noise.sample(x as f64, z as f64);
        let mut cover = 0.0;
        if column.height < WATER_LEVEL as f64 {
            water += weight;
        } else {
            cover = (column.trees / DENSE_FOREST).min(1.0) as f32;
            trees += weight * cover;
            steppe += weight * (noise.biomes().name(column.biome) == Some("steppe")) as u8 as f32;
        }
        total += weight;
        if is_overhead {
            overhead += weight * cover;
            overhead_total += weight;
        }
    };
    sample(eye.x, eye.z, CENTRE_WEIGHT, true);
    for (ring, (radius, weight)) in RINGS.into_iter().enumerate() {
        for i in 0..RING_COLUMNS {
            let (sin, cos) = ops::sin_cos(i as f32 / RING_COLUMNS as f32 * TAU);
            sample(
                eye.x + cos * radius,
                eye.z + sin * radius,
                weight,
                ring == 0,
            );
        }
    }
    let here = noise.sample(eye.x as f64, eye.z as f64);
    let temperature = noise.temperature(eye.x as f64, eye.z as f64);
    let biome = noise.biomes().name(here.biome);

    *around = Surroundings {
        altitude: eye.y - ground,
        speed,
        underwater: world.get_block_at(eye.floor().as_i64vec3()) == BlockType::Water,
        water: water / total,
        trees: trees / total,
        trees_overhead: overhead / overhead_total,
        warmth: smoothstep(FREEZING.0, FREEZING.1, temperature),
        feel: Feel::at(noise, eye.x as f64, eye.z as f64),
        desert: (biome == Some("desert")) as u8 as f32,
        steppe: steppe / total,
        exposure: noise.exposure(eye.x as f64, eye.z as f64),
        biome,
        day: smoothstep(-0.05, 0.2, sun_direction(clock.days()).y),
        breeze: dials.breeze(),
    };
}

/// A recorded loop, and how loud it plays at most, as the files are all levelled alike.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Loop {
    Forest,
    Crickets,
    Waves,
    Underwater,
    Leaves,
}

impl Loop {
    const ALL: [Loop; 5] = [
        Loop::Forest,
        Loop::Crickets,
        Loop::Waves,
        Loop::Underwater,
        Loop::Leaves,
    ];

    fn file(self) -> &'static str {
        match self {
            Loop::Forest => "sounds/forest.ogg",
            Loop::Crickets => "sounds/crickets.ogg",
            Loop::Waves => "sounds/waves.ogg",
            Loop::Underwater => "sounds/underwater.ogg",
            Loop::Leaves => "sounds/leaves.ogg",
        }
    }

    // Each file's measured loudness and the loudness it plays at in full, in LUFS.
    fn loudness(self) -> (f32, f32) {
        match self {
            Loop::Forest => (-19.2, -30.0),
            Loop::Crickets => (-14.0, -31.0),
            Loop::Waves => (-19.7, -30.0),
            Loop::Underwater => (-18.4, -22.0),
            Loop::Leaves => (-20.2, -35.0),
        }
    }

    fn gain(self) -> f32 {
        let (measured, wanted) = self.loudness();
        10f32.powf((wanted - measured) / 20.0) * AMBIENCE_TRIM
    }

    /// Whether this loop calls from a perch in the world instead of all round the camera.
    fn perched(self) -> bool {
        matches!(self, Loop::Forest | Loop::Crickets)
    }

    /// Whether this loop comes and goes on its own, as birds and crickets do, instead of following its surroundings.
    fn coming_and_going(self) -> Option<Bursts> {
        match self {
            // birds strike up now and then, and fall quiet more than they sing
            Loop::Forest => Some(Bursts {
                seconds: (4.0, 14.0),
                quiet: (0.65, 0.92),
            }),
            // a cricket or two starts up, chirps a while and stops
            Loop::Crickets => Some(Bursts {
                seconds: (3.0, 10.0),
                quiet: (0.72, 0.95),
            }),
            // the leaves come and go with the wind's breezes instead, see `Loop::level`
            Loop::Waves | Loop::Underwater | Loop::Leaves => None,
        }
    }

    /// How loud this loop should be in these surroundings, 0..1, before its gain.
    pub fn level(self, around: &Surroundings) -> f32 {
        if around.underwater {
            return (self == Loop::Underwater) as u8 as f32;
        }
        // what's on the ground fades as the camera climbs away from it
        let near = |from, to| 1.0 - smoothstep(from, to, around.altitude);
        let night = 1.0 - around.day;
        // birds and crickets are heard only from a perch, which says where, see `perch_loops`
        match self {
            Loop::Forest => around.day * around.warmth * near(15.0, 90.0),
            Loop::Crickets => (1.0 - around.desert) * night * around.warmth * near(15.0, 90.0),
            // frozen sea doesn't break on the shore
            Loop::Waves => around.water * around.warmth * near(10.0, 120.0),
            Loop::Underwater => 0.0,
            // only under the trees' crowns, and only while a breeze stirs them
            Loop::Leaves => around.trees_overhead * around.breeze * near(8.0, 40.0),
        }
    }
}

/// How a loop comes and goes: it follows a value wandering 0..1, see `Gust`.
struct Bursts {
    // how long the value takes to wander to each new target, in seconds, at the least and most
    seconds: (f32, f32),
    // the value goes from silence (under the first) to full (over the second); higher means silent more often
    quiet: (f32, f32),
}

/// Where a loop that comes and goes is in its comings and goings, see `Loop::coming_and_going`.
#[derive(Component, Default)]
struct Activity(Gust);

// How long the loops take to follow the surroundings, in seconds.
const LOOP_FOLLOW_SECONDS: f32 = 1.5;

fn play_loops(mut commands: Commands, assets: Res<AssetServer>) {
    commands.insert_resource(Owl(assets.load("sounds/owl.ogg")));
    for kind in Loop::ALL {
        let mut entity = commands.spawn((
            kind,
            Activity::default(),
            AudioPlayer::<AudioSource>(assets.load(kind.file())),
            PlaybackSettings {
                spatial: kind.perched(),
                // explicit, so a game's own `DefaultSpatialScale` doesn't change how far they carry
                spatial_scale: kind.perched().then(|| SpatialScale::new(PERCH_SCALE)),
                ..PlaybackSettings::LOOP.with_volume(Volume::SILENT)
            },
        ));
        if kind.perched() {
            entity.insert((Transform::default(), Perched::default()));
        }
    }
}

/// An entity a bird or cricket may call from, for a game to put on its bushes and the like.
#[derive(Component, Default, Debug, Clone, Copy)]
pub struct Perch;

/// Where a perched loop calls from, see `perch_loops`.
#[derive(Component, Default)]
struct Perched {
    at: Option<Vec3>,
    // whether it's been heard since it settled there
    heard: bool,
    // seconds until it may look for a perch again
    retry: f32,
}

impl Perched {
    /// Whether a silent loop should fly to a new perch: once it's been heard, so the next call comes from elsewhere, or once it's left behind.
    fn wants_moving(&self, eye: Vec3) -> bool {
        self.heard || self.at.is_none_or(|at| at.distance(eye) > PERCH_REACH)
    }
}

// How far off a loop's perch may be, in blocks.
const PERCH_REACH: f32 = 40.0;
// How far out the treetops looked for each time are, in blocks, and how many columns.
const CANOPY_DISTANCE: (f32, f32) = (6.0, 36.0);
const CANOPY_COLUMNS: usize = 12;
// How far over the ground a column is looked up for leaves, in blocks.
const CANOPY_HEIGHT: (i64, i64) = (2, 24);
// Seconds between looks for a perch, as finding one takes a few hundred block lookups.
const PERCH_RETRY: f32 = 2.0;
// The stereo falloff's block (inverse square past 1/scale blocks), as the owl's: full within about 17 blocks.
const PERCH_SCALE: f32 = OWL_SCALE;

/// Moves each silent perched loop to a `Perch` or a treetop near the camera, so it's heard from there next.
fn perch_loops(
    time: Res<Time>,
    world: Res<World>,
    camera: Query<&GlobalTransform, With<Camera3d>>,
    perches: Query<&GlobalTransform, With<Perch>>,
    mut loops: Query<(&mut Perched, &mut Transform, &SpatialAudioSink)>,
) {
    let Ok(camera) = camera.single() else {
        return;
    };
    let eye = camera.translation();
    for (mut perched, mut transform, sink) in &mut loops {
        perched.retry -= time.delta_secs();
        if sink.volume().to_linear() >= SILENT || perched.retry > 0.0 || !perched.wants_moving(eye)
        {
            continue;
        }
        perched.retry = PERCH_RETRY;
        let mut candidates: Vec<Vec3> = perches
            .iter()
            .map(GlobalTransform::translation)
            .filter(|at| at.distance(eye) <= PERCH_REACH)
            .collect();
        candidates.extend((0..CANOPY_COLUMNS).filter_map(|_| treetop(&world, eye)));
        let at =
            (!candidates.is_empty()).then(|| candidates[rand::random_range(0..candidates.len())]);
        if let Some(at) = at {
            transform.translation = at;
        }
        *perched = Perched {
            at,
            heard: false,
            ..*perched
        };
    }
}

/// The leaves of a tree in a random column some way from `eye`, if there's one standing there.
fn treetop(world: &World, eye: Vec3) -> Option<Vec3> {
    let angle = rand::random_range(0.0..TAU);
    let distance = rand::random_range(CANOPY_DISTANCE.0..CANOPY_DISTANCE.1);
    let x = (eye.x + ops::cos(angle) * distance).floor() as i64;
    let z = (eye.z + ops::sin(angle) * distance).floor() as i64;
    let ground = world.region_store.get_height(x, z).floor() as i64;
    (ground + CANOPY_HEIGHT.0..=ground + CANOPY_HEIGHT.1)
        .map(|y| bevy::math::I64Vec3::new(x, y, z))
        .find(|&block| world.get_block_at(block) == BlockType::Leaves)
        .map(|block| block.as_vec3() + 0.5)
}

#[allow(clippy::type_complexity)]
fn mix_loops(
    time: Res<Time>,
    around: Res<Surroundings>,
    weather: Option<Res<crate::weather::Weather>>,
    mut loops: Query<(&Loop, &mut Activity, &mut AudioSink)>,
    // a spatial sound has a `SpatialAudioSink`, never an `AudioSink`
    mut perched: Query<
        (&Loop, &mut Activity, &mut SpatialAudioSink, &mut Perched),
        Without<AudioSink>,
    >,
) {
    // birds and crickets fall silent in the rain, and before a storm's rain, or without it
    let rain = weather.map_or(0.0, |weather| weather.gloom());
    let follow = 1.0 - (-time.delta_secs() / LOOP_FOLLOW_SECONDS).exp();
    let wanted = |kind: Loop, activity: &mut Activity| {
        let active = kind.coming_and_going().map_or(1.0, |bursts| {
            let (a, b) = (rand::random(), rand::random());
            let wander = activity.0.advance(time.delta_secs(), bursts.seconds, a, b);
            smoothstep(bursts.quiet.0, bursts.quiet.1, wander)
        });
        let sheltering = match kind {
            Loop::Forest | Loop::Crickets => 1.0 - rain,
            _ => 1.0,
        };
        // square-rooted, as loudness goes by power: a third the thickness is about 5 dB quieter, not 10
        kind.level(&around).sqrt() * kind.gain() * active * sheltering
    };
    for (kind, mut activity, mut sink) in &mut loops {
        glide(&mut *sink, wanted(*kind, &mut activity), follow);
    }
    for (kind, mut activity, mut sink, mut perched) in &mut perched {
        // without a perch there's nowhere to hear it from
        let to = wanted(*kind, &mut activity) * perched.at.is_some() as u8 as f32;
        perched.heard |= glide(&mut *sink, to, follow);
    }
}

/// Moves a loop's volume a `follow` share of the way to `wanted`, and returns whether it can be heard.
fn glide(sink: &mut impl AudioSinkPlayback, wanted: f32, follow: f32) -> bool {
    let current = sink.volume().to_linear();
    let volume = current + (wanted - current) * follow;
    sink.set_volume(Volume::Linear(volume));
    // a loop no one can hear is paused, so it isn't decoded for nothing
    let silent = volume < SILENT && wanted < SILENT;
    if silent && !sink.is_paused() {
        sink.pause();
    } else if !silent && sink.is_paused() {
        sink.play();
    }
    volume >= SILENT
}

// How quiet a loop is, as a linear volume, before it's paused, see `mix_loops`.
const SILENT: f32 = 1e-3;

/// Hears from the camera, for sounds placed in the world, see `hoot`.
fn listen_from_camera(
    mut commands: Commands,
    cameras: Query<Entity, (With<Camera3d>, Without<SpatialListener>)>,
) {
    for camera in &cameras {
        commands.entity(camera).insert(SpatialListener::default());
    }
}

// How long between owls' hoots in a forest at night, in seconds, at the least and most.
const HOOT_SECONDS: (f32, f32) = (12.0, 40.0);
// How far off an owl hoots, in blocks, at the nearest and farthest.
const OWL_DISTANCE: (f32, f32) = (20.0, 45.0);
// A block's distance for the stereo falloff (inverse of distance), so owls a few dozen blocks off are faint but clear
const OWL_SCALE: f32 = 0.06;
// Brings a hoot a few dozen blocks off to about the crickets' loudness.
const OWL_VOLUME: f32 = 0.2 * AMBIENCE_TRIM;

/// How likely an owl is to hoot here, 0..1: as loud as the forest would be at night, and only in the forest.
fn hoot_chance(around: &Surroundings) -> f32 {
    let night = Surroundings {
        day: 1.0 - around.day,
        ..*around
    };
    around.trees * Loop::Forest.level(&night)
}

#[derive(Resource)]
struct Owl(Handle<AudioSource>);

fn hoot(
    mut commands: Commands,
    time: Res<Time>,
    around: Res<Surroundings>,
    camera: Query<&GlobalTransform, With<Camera3d>>,
    owl: Res<Owl>,
    mut wait: Local<Option<f32>>,
) {
    let chance = hoot_chance(&around);
    let left = wait.get_or_insert_with(|| rand::random_range(HOOT_SECONDS.0..HOOT_SECONDS.1));
    // owls only call where the forest's loud enough at night, and wait longer in thin woods
    *left -= time.delta_secs() * chance;
    if *left > 0.0 || chance < 0.25 {
        return;
    }
    *wait = None;
    let Ok(camera) = camera.single() else {
        return;
    };
    let angle = rand::random_range(0.0..TAU);
    let distance = rand::random_range(OWL_DISTANCE.0..OWL_DISTANCE.1);
    let offset = Vec3::new(ops::cos(angle), 0.0, ops::sin(angle)) * distance
        + Vec3::Y * rand::random_range(4.0..12.0);
    commands.spawn((
        AudioPlayer(owl.0.clone()),
        PlaybackSettings {
            spatial: true,
            spatial_scale: Some(SpatialScale::new(OWL_SCALE)),
            volume: Volume::Linear(OWL_VOLUME),
            speed: rand::random_range(0.95..1.05),
            ..PlaybackSettings::DESPAWN
        },
        Transform::from_translation(camera.translation() + offset),
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn levels(around: Surroundings) -> Vec<(Loop, f32)> {
        Loop::ALL
            .into_iter()
            .map(|kind| (kind, kind.level(&around)))
            .filter(|(_, level)| *level > 0.01)
            .collect()
    }

    #[test]
    fn test_forest_by_day_and_crickets_by_night() {
        let forest = Surroundings {
            trees: 0.9,
            ..Default::default()
        };
        assert_eq!(
            vec![Loop::Forest],
            levels(forest).into_iter().map(|l| l.0).collect::<Vec<_>>()
        );
        let night = Surroundings { day: 0.0, ..forest };
        assert_eq!(
            vec![Loop::Crickets],
            levels(night).into_iter().map(|l| l.0).collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_leaves_rustle_only_under_trees_in_a_breeze() {
        let still = Surroundings {
            trees: 0.9,
            trees_overhead: 0.9,
            ..Default::default()
        };
        assert_eq!(0.0, Loop::Leaves.level(&still));
        let breeze = Surroundings {
            breeze: 1.0,
            ..still
        };
        assert!(Loop::Leaves.level(&breeze) > 0.8);
        // forest down the slope doesn't rustle over a bare peak
        let peak = Surroundings {
            trees_overhead: 0.0,
            ..breeze
        };
        assert_eq!(0.0, Loop::Leaves.level(&peak));
    }

    #[test]
    fn test_birds_sing_from_perches_but_owls_stay_in_forests() {
        // the Forest loop's perches decide where it's heard, so open country can have birds
        let open = Surroundings::default();
        assert!(Loop::Forest.level(&open) > 0.9);
        let night = |trees| Surroundings {
            trees,
            day: 0.0,
            ..open
        };
        assert_eq!(0.0, hoot_chance(&night(0.0)));
        assert!(hoot_chance(&night(0.9)) > 0.8);
    }

    #[test]
    fn test_a_perched_loop_moves_once_heard_or_left_behind() {
        let eye = Vec3::ZERO;
        let near = Perched {
            at: Some(Vec3::new(10.0, 5.0, 0.0)),
            ..Default::default()
        };
        assert!(!near.wants_moving(eye));
        assert!(Perched {
            heard: true,
            ..near
        }
        .wants_moving(eye));
        assert!(Perched::default().wants_moving(eye));
        let far = Perched {
            at: Some(Vec3::new(PERCH_REACH + 1.0, 0.0, 0.0)),
            ..Default::default()
        };
        assert!(far.wants_moving(eye));
    }

    #[test]
    fn test_underwater_hears_only_the_water() {
        let around = Surroundings {
            underwater: true,
            water: 1.0,
            trees: 0.5,
            ..Default::default()
        };
        assert_eq!(vec![(Loop::Underwater, 1.0)], levels(around));
    }

    #[test]
    fn test_the_ground_falls_quiet_high_up_and_the_sea_freezes_silent() {
        let coast = Surroundings {
            water: 0.5,
            trees: 0.5,
            ..Default::default()
        };
        assert!(Loop::Waves.level(&coast) > 0.4);
        let high = Surroundings {
            altitude: 200.0,
            ..coast
        };
        assert!(levels(high).is_empty());
        let frozen = Surroundings {
            warmth: 0.0,
            ..coast
        };
        assert_eq!(0.0, Loop::Waves.level(&frozen));
    }
}
