//! The weather where the camera is, from the world's `storms::Forecast`: darker skies and
//! clouds as a storm comes over, rain and its sound, quiet snow where it's cold, a stronger wind,
//! lightning and thunder in a thunderstorm's core, and a rainbow once the rain clears in sunshine.
use std::{
    f32::consts::{PI, TAU},
    time::Duration,
};

use bevy::{
    asset::RenderAssetUsages,
    audio::{AddAudioSource, ChannelCount, Decodable, SampleRate, Source, Volume},
    camera::{visibility::NoFrustumCulling, Exposure},
    light::{NotShadowCaster, NotShadowReceiver},
    math::ops,
    mesh::{Indices, PrimitiveTopology},
    pbr::{MaterialPipeline, MaterialPipelineKey},
    prelude::*,
    reflect::TypePath,
    render::{
        mesh::MeshVertexBufferLayoutRef,
        render_resource::{AsBindGroup, RenderPipelineDescriptor, SpecializedMeshPipelineError},
    },
    shader::ShaderRef,
};

use crate::{
    ambience::{AMBIENCE_TRIM, SAMPLE_RATE},
    clouds::CloudMaterial,
    sky::{
        camera_exposure, moon_illuminance, sun_direction, sun_illuminance, Moon, Sun, TimeOfDay,
        TICKS_PER_SECOND,
    },
    storms::{seconds, Forecast},
};
use bevy::math::DVec2;
use ochre_base::{block::BlockType, math::smoothstep};
use ochre_terrain::{curvature::WORLD_CURVE, world::World};

/// Rain, snow, storms and rainbows. `clear_sky` holds the sky clear: no storms, rain, snow, lightning or rainbow, and
/// only fair-weather cloud, so a recording that winds the clock on doesn't drive into a storm.
#[derive(Default)]
pub struct WeatherPlugin {
    pub clear_sky: bool,
}

impl Plugin for WeatherPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            MaterialPlugin::<RainMaterial>::default(),
            MaterialPlugin::<SnowMaterial>::default(),
            MaterialPlugin::<RainbowMaterial>::default(),
        ))
        .add_audio_source::<Thunder>()
        .init_resource::<TimeOfDay>()
        .insert_resource(Weather {
            calm: self.clear_sky,
            ..default()
        })
        .add_systems(Startup, spawn_weather)
        .add_systems(
            Update,
            (
                start_forecast,
                run_weather,
                (darken_sky, fall_rain, strike_lightning, show_rainbow, mix_rain),
            )
                .chain(),
        );
    }
}

/// How the weather is where the camera is, from the world's `Forecast`.
#[derive(Resource)]
pub struct Weather {
    /// How hard it's raining, 0..1: what falls where it's too warm for snow.
    pub rain: f32,
    /// How hard it's snowing, 0..1: what falls where grass lies under snow.
    pub snow: f32,
    /// The share of the sky clouded over, 0..1.
    pub cloud: f32,
    /// How much of a storm's dark cloud is overhead, 0..1.
    pub storm: f32,
    /// How much of a thunderstorm's core is overhead, 0..1.
    pub thunder: f32,
    /// How strong the rainbow after rain is, 0..1.
    pub rainbow: f32,
    /// The way storms drift, in blocks a game second, x and z.
    pub wind: Vec2,
    // seconds until the next lightning strike
    strike_in: f32,
    // whether the sky's held clear, see `WeatherPlugin`
    calm: bool,
}

impl Default for Weather {
    fn default() -> Self {
        Self {
            rain: 0.0,
            snow: 0.0,
            cloud: 0.0,
            storm: 0.0,
            thunder: 0.0,
            rainbow: 0.0,
            wind: Vec2::X,
            strike_in: 6.0,
            calm: false,
        }
    }
}

impl Weather {
    /// Whether the sky's held clear, see `WeatherPlugin::clear_sky`.
    pub fn calm(&self) -> bool {
        self.calm
    }

    /// How hard rain or snow is falling, 0..1.
    pub fn precipitation(&self) -> f32 {
        self.rain + self.snow
    }

    /// Whether rain or snow falls hard enough that what's out and about takes shelter.
    pub fn sheltering(&self) -> bool {
        self.precipitation() > SHELTER_RAIN
    }

    /// How much the storm overhead darkens the day, 0..1: its cloud before the rain or snow
    /// arrives, and what falls.
    pub fn gloom(&self) -> f32 {
        self.precipitation().max(GLOOM_BEFORE_RAIN * self.storm)
    }
}

// How hard rain falls, and snow, out of `sky`'s precipitation.
fn rain_and_snow(sky: &crate::storms::Sky) -> (f32, f32) {
    (sky.precipitation * (1.0 - sky.snow), sky.precipitation * sky.snow)
}

// How hard rain or snow falls, see `Weather::precipitation`, before birds and insects take shelter.
const SHELTER_RAIN: f32 = 0.2;
// How dark a storm's cloud makes the day before its rain arrives, as a share of the rain's gloom.
const GLOOM_BEFORE_RAIN: f32 = 0.8;
// A rainbow shows after rain: looking back this many game seconds at this many moments, how
// hard it rained at the most, and how little it rains now.
const RAINBOW_LOOK_BACK: f32 = 1200.0;
const RAINBOW_LOOKS: u32 = 8;

/// How strong a rainbow is when it rained `was` hard lately and rains `now` hard now, both 0..1.
fn rainbow_after(was: f32, now: f32) -> f32 {
    smoothstep(0.35, 0.6, was) * (1.0 - smoothstep(0.05, 0.2, now))
}

// The world's weather, once its world is loaded.
fn start_forecast(
    mut commands: Commands,
    world: Option<Res<World>>,
    forecast: Option<Res<Forecast>>,
) {
    if let (Some(world), None) = (world, forecast) {
        let forecast = Forecast::from_world(&world);
        info!(
            "weather: storms drift at {:.1} blocks a second toward ({:.2}, {:.2})",
            forecast.wind().length(),
            forecast.wind().normalize_or_zero().x,
            forecast.wind().normalize_or_zero().y
        );
        commands.insert_resource(forecast);
    }
}

// The weather where the camera is. It keeps the game's time, so winding the clock on winds it
// on too and back winds it back. Lightning keeps real time, or a wound-on storm would strobe.
fn run_weather(
    clock: Res<TimeOfDay>,
    forecast: Option<Res<Forecast>>,
    camera: Query<&GlobalTransform, With<Camera3d>>,
    mut weather: ResMut<Weather>,
    // when the rain lately was last looked back at, and the most there was
    mut last_look: Local<Option<(u64, f32)>>,
) {
    // a sky held clear keeps the weather at nothing
    let (Some(forecast), Ok(camera), false) = (forecast, camera.single(), weather.calm) else {
        return;
    };
    let eye = camera.translation();
    let (x, z) = (eye.x as f64, eye.z as f64);
    let sky = forecast.sky(x, z, clock.0);
    // the rain lately changes slowly, so it's looked back at once a game second
    if last_look.is_none_or(|(tick, _)| clock.0.abs_diff(tick) >= TICKS_PER_SECOND) {
        let step = (RAINBOW_LOOK_BACK / RAINBOW_LOOKS as f32 * TICKS_PER_SECOND as f32) as u64;
        let was = (1..=RAINBOW_LOOKS)
            .map(|k| {
                rain_and_snow(&forecast.sky(x, z, clock.0.saturating_sub(k as u64 * step))).0
            })
            .fold(0.0f32, f32::max);
        *last_look = Some((clock.0, was));
    }
    let was = last_look.map_or(0.0, |(_, was)| was);
    (weather.rain, weather.snow) = rain_and_snow(&sky);
    weather.cloud = sky.cloud;
    weather.storm = sky.storm;
    weather.thunder = sky.thunder;
    weather.rainbow = rainbow_after(was, weather.rain);
    weather.wind = forecast.wind().as_vec2();
}

// How much of the sun's and moon's light gets through the storm's clouds, and how dark the
// clouds themselves get, at the heaviest rain.
const STORM_LIGHT: f32 = 0.3;
const STORM_CLOUD: f32 = 0.42;
// Lightning's flash, in stops brighter: held, then dying away, in seconds, with a fainter
// flicker after.
const FLASH_STOPS: f32 = 3.2;
const FLASH_HOLD_SECONDS: f32 = 0.08;
const FLASH_SECONDS: f32 = 0.5;
const FLICKER_STOPS: f32 = 2.2;

#[derive(Component)]
struct Flash {
    // seconds since the strike, and when the second, fainter flicker comes
    age: f32,
    flicker: f32,
}

impl Flash {
    // How many stops brighter it lights everything now.
    fn stops(&self) -> f32 {
        let dying = |since: f32| {
            let t = ((since - FLASH_HOLD_SECONDS) / FLASH_SECONDS).clamp(0.0, 1.0);
            if since < 0.0 {
                0.0
            } else {
                (1.0 - t).powf(2.5)
            }
        };
        (FLASH_STOPS * dying(self.age)).max(FLICKER_STOPS * dying(self.age - self.flicker))
    }
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn darken_sky(
    weather: Res<Weather>,
    time: Res<Time>,
    mut lights: Query<(&mut DirectionalLight, Has<Sun>), Or<(With<Sun>, With<Moon>)>>,
    clouds: Option<ResMut<Assets<CloudMaterial>>>,
    mut cameras: Query<&mut Exposure, With<Camera3d>>,
    mut flashes: Query<(Entity, &mut Flash)>,
    mut commands: Commands,
    mut last_shade: Local<f32>,
) {
    let shade = 1.0 - (1.0 - STORM_LIGHT) * weather.gloom();
    for (mut light, is_sun) in &mut lights {
        light.illuminance = shade
            * if is_sun {
                sun_illuminance()
            } else {
                moon_illuminance()
            };
    }
    let cloud_shade = 1.0 - (1.0 - STORM_CLOUD) * weather.gloom();
    if (cloud_shade - *last_shade).abs() > 0.002 {
        *last_shade = cloud_shade;
        if let Some(mut clouds) = clouds {
            for (_, cloud) in clouds.iter_mut() {
                cloud.set_shade(cloud_shade);
            }
        }
    }

    // lightning lights everything the moment it strikes, holds, dies away and flickers once
    let mut stops = 0.0f32;
    for (entity, mut flash) in &mut flashes {
        flash.age += time.delta_secs();
        stops = stops.max(flash.stops());
        if flash.age > flash.flicker + FLASH_HOLD_SECONDS + FLASH_SECONDS {
            commands.entity(entity).despawn();
        }
    }
    for mut exposure in &mut cameras {
        exposure.ev100 = camera_exposure().ev100 - stops;
    }
}

// What every effect's material shares: the one uniform it takes.
trait Effect: Asset {
    fn params(&self) -> Vec4;
    fn set_params(&mut self, params: Vec4);
}

// Sets an effect's uniform, only when it changes, as writing marks it for the GPU again.
fn set_params<M: Effect>(materials: &mut Assets<M>, handle: &Handle<M>, params: Vec4) {
    if materials
        .get(handle)
        .is_some_and(|material| material.params() != params)
    {
        if let Some(mut material) = materials.get_mut(handle) {
            material.set_params(params);
        }
    }
}

// A see-through effect drawn from both sides by its own shaders, which casts nothing and
// takes one uniform: what it needs to know this frame.
macro_rules! effect_material {
    ($(#[$doc:meta])* $material:ident, vertex: $vertex:expr, fragment: $fragment:literal) => {
        $(#[$doc])*
        #[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
        pub struct $material {
            #[uniform(0)]
            params: Vec4,
        }

        impl Effect for $material {
            fn params(&self) -> Vec4 {
                self.params
            }

            fn set_params(&mut self, params: Vec4) {
                self.params = params;
            }
        }

        impl Material for $material {
            fn vertex_shader() -> ShaderRef {
                $vertex
            }

            fn fragment_shader() -> ShaderRef {
                $fragment.into()
            }

            fn alpha_mode(&self) -> AlphaMode {
                AlphaMode::Premultiplied
            }

            fn enable_prepass() -> bool {
                false
            }

            fn enable_shadows() -> bool {
                false
            }

            fn specialize(
                _pipeline: &MaterialPipeline,
                descriptor: &mut RenderPipelineDescriptor,
                _layout: &MeshVertexBufferLayoutRef,
                _key: MaterialPipelineKey<Self>,
            ) -> Result<(), SpecializedMeshPipelineError> {
                descriptor.primitive.cull_mode = None;
                Ok(())
            }
        }
    };
}

effect_material!(
    /// The streaks of rain, falling in `rain.wesl`.
    RainMaterial,
    vertex: "shaders/rain.wesl".into(),
    fragment: "shaders/rain.wesl"
);
effect_material!(
    /// The flakes of snow, falling in `snow.wesl`.
    SnowMaterial,
    vertex: "shaders/snow.wesl".into(),
    fragment: "shaders/snow.wesl"
);
effect_material!(
    /// The rainbow's band, coloured in `rainbow.wesl`.
    RainbowMaterial,
    vertex: ShaderRef::Default,
    fragment: "shaders/rainbow.wesl"
);

// The box of rain round the camera, in blocks, matching `BOX` in `rain.wesl`, and how many
// streaks fill it in the heaviest rain.
const RAIN_BOX: Vec3 = Vec3::new(48.0, 32.0, 48.0);
const STREAKS: usize = 14_000;
// How hard the rain slants with the storm's wind, in blocks a second, in light rain and in the heaviest.
const RAIN_SLANT: (f32, f32) = (2.0, 4.5);
// The box of snow round the camera, in blocks, matching `BOX` in `snow.wesl`, and how many
// flakes fill it in the heaviest snow: a smaller box than the rain's, as flakes far off are
// lost in the white anyway.
const SNOW_BOX: Vec3 = Vec3::new(40.0, 24.0, 40.0);
const FLAKES: usize = 18_000;
// How fast snow drifts with the storm's wind, in blocks a second, in light snow and in the
// heaviest: about as fast as it falls (`FALL` in `snow.wesl`), so it slants well over.
const SNOW_DRIFT: (f32, f32) = (0.8, 2.4);
// How much of the daylight the flakes lose in the heaviest snow, and the drops in the heaviest rain.
const SNOW_GLOOM: f32 = 0.35;
const RAIN_GLOOM: f32 = 0.5;

#[derive(Resource)]
struct WeatherHandles {
    rain: Handle<RainMaterial>,
    snow: Handle<SnowMaterial>,
    rainbow: Handle<RainbowMaterial>,
    bolt: Handle<StandardMaterial>,
}

#[derive(Component)]
struct Rainbow;

/// The streaks of rain round the camera, hidden while it's dry.
#[derive(Component)]
struct RainCurtain;

/// The flakes of snow round the camera, hidden while none falls.
#[derive(Component)]
struct SnowCurtain;

// Shows or hides an effect, leaving it be when it's already so, as change detection would
// otherwise re-extract it every frame.
fn show_if(visibility: &mut Mut<Visibility>, shown: bool) {
    visibility.set_if_neq(if shown {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    });
}

fn spawn_weather(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut rains: ResMut<Assets<RainMaterial>>,
    mut snows: ResMut<Assets<SnowMaterial>>,
    mut rainbows: ResMut<Assets<RainbowMaterial>>,
    mut standard: ResMut<Assets<StandardMaterial>>,
    assets: Res<AssetServer>,
) {
    let rain = rains.add(RainMaterial { params: Vec4::ZERO });
    let snow = snows.add(SnowMaterial { params: Vec4::ZERO });
    let rainbow = rainbows.add(RainbowMaterial { params: Vec4::ZERO });
    commands.spawn((
        RainCurtain,
        Mesh3d(meshes.add(scatter_mesh(STREAKS, RAIN_BOX))),
        MeshMaterial3d(rain.clone()),
        Transform::default(),
        NoFrustumCulling,
        NotShadowCaster,
        NotShadowReceiver,
    ));
    commands.spawn((
        SnowCurtain,
        Mesh3d(meshes.add(scatter_mesh(FLAKES, SNOW_BOX))),
        MeshMaterial3d(snow.clone()),
        Transform::default(),
        NoFrustumCulling,
        NotShadowCaster,
        NotShadowReceiver,
    ));
    commands.spawn((
        Rainbow,
        Mesh3d(meshes.add(rainbow_mesh())),
        MeshMaterial3d(rainbow.clone()),
        Transform::default(),
        NoFrustumCulling,
        NotShadowCaster,
        NotShadowReceiver,
    ));
    let bolt = standard.add(StandardMaterial {
        base_color: Color::BLACK,
        emissive: LinearRgba::rgb(4.0, 4.4, 6.0),
        emissive_exposure_weight: 0.0,
        fog_enabled: false,
        cull_mode: None,
        ..default()
    });
    commands.insert_resource(WeatherHandles {
        rain,
        snow,
        rainbow,
        bolt,
    });
    commands.spawn((
        RainLoop,
        AudioPlayer::<AudioSource>(assets.load("sounds/rain.ogg")),
        PlaybackSettings::LOOP.with_volume(Volume::SILENT),
    ));
}

/// The loop of rain on the garden, `sounds/rain.ogg`, as loud as the rain is heavy.
#[derive(Component)]
struct RainLoop;

// The rain loop's loudness as recorded and as wanted at full rain, in LUFS, as `ambience::Loop::loudness` does for the others: its drips stand 25 dB over its bed, so it can't be levelled much louder than it is without clipping them.
const RAIN_LOUDNESS: (f32, f32) = (-31.3, -31.5);
// How long the rain's sound takes to follow the rain, in seconds, and the volume below which a loop is paused.
const RAIN_FOLLOW_SECONDS: f32 = 1.5;
const RAIN_SILENT: f32 = 1e-3;

/// The volume the rain loop plays at when it rains `rain` hard, 0..1.
fn rain_volume(rain: f32) -> f32 {
    let gain = 10f32.powf((RAIN_LOUDNESS.1 - RAIN_LOUDNESS.0) / 20.0);
    // square-rooted, as loudness goes by power
    rain.clamp(0.0, 1.0).sqrt() * gain
}

fn mix_rain(time: Res<Time>, weather: Res<Weather>, mut rain: Query<&mut AudioSink, With<RainLoop>>) {
    let wanted = rain_volume(weather.rain) * AMBIENCE_TRIM;
    let follow = 1.0 - (-time.delta_secs() / RAIN_FOLLOW_SECONDS).exp();
    for mut sink in &mut rain {
        let volume = sink.volume().to_linear();
        let volume = volume + (wanted - volume) * follow;
        sink.set_volume(Volume::Linear(volume));
        // a loop no one can hear is paused, so it isn't decoded for nothing
        let silent = volume < RAIN_SILENT && wanted < RAIN_SILENT;
        if silent && !sink.is_paused() {
            sink.pause();
        } else if !silent && sink.is_paused() {
            sink.play();
        }
    }
}

// `count` streaks or flakes scattered through a box of `size`, each four corners at one spot,
// told apart by their uv.
fn scatter_mesh(count: usize, size: Vec3) -> Mesh {
    let mut positions = Vec::with_capacity(count * 4);
    let mut uvs = Vec::with_capacity(count * 4);
    let mut indices = Vec::with_capacity(count * 6);
    for i in 0..count {
        let spot = Vec3::new(
            rand::random_range(-0.5..0.5),
            rand::random_range(-0.5..0.5),
            rand::random_range(-0.5..0.5),
        ) * size;
        let start = (i * 4) as u32;
        for uv in [[-1.0, 0.0], [1.0, 0.0], [-1.0, 1.0], [1.0, 1.0]] {
            positions.push(spot.to_array());
            uvs.push(uv);
        }
        indices.extend([0, 2, 1, 1, 2, 3].map(|k| start + k));
    }
    let count = positions.len();
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 1.0, 0.0]; count])
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
    // `rain.wesl` and `snow.wesl` pass their own values along in the colour
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, vec![[1.0f32; 4]; count])
    .with_inserted_indices(Indices::U32(indices))
}

// A unit ring in its xy plane, its band from 0.95 to 1 of the way out, uv.x across the band.
fn rainbow_mesh() -> Mesh {
    const SEGMENTS: usize = 128;
    const INNER: f32 = 0.95;
    let mut positions = Vec::new();
    let mut uvs = Vec::new();
    let mut indices = Vec::new();
    for i in 0..=SEGMENTS {
        let angle = i as f32 / SEGMENTS as f32 * TAU;
        let (sin, cos) = ops::sin_cos(angle);
        positions.push([cos * INNER, sin * INNER, 0.0]);
        positions.push([cos, sin, 0.0]);
        uvs.push([0.0, 0.0]);
        uvs.push([1.0, 0.0]);
        if i < SEGMENTS {
            let k = (i * 2) as u32;
            indices.extend([k, k + 1, k + 2, k + 2, k + 1, k + 3]);
        }
    }
    let count = positions.len();
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 0.0, 1.0]; count])
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
    .with_inserted_indices(Indices::U32(indices))
}

/// How bright the daylight is, 0..1, for things that aren't lit by the world's shaders.
fn unlit_light(clock: &TimeOfDay) -> f32 {
    ((sun_direction(clock.days()).y + 0.1) / 0.4).clamp(0.05, 1.0)
}

/// How fast and which way rain or snow falling `amount` hard (0..1) is blown across the ground
/// by a storm drifting along `wind`, in blocks a second, from `light` in the lightest to `heavy`
/// in the heaviest.
fn blown(wind: Vec2, amount: f32, (light, heavy): (f32, f32)) -> Vec2 {
    wind.normalize_or(Vec2::X) * (light + (heavy - light) * amount.clamp(0.0, 1.0))
}

// The rain's streaks and the snow's flakes round the camera, as hard as each falls.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn fall_rain(
    weather: Res<Weather>,
    clock: Res<TimeOfDay>,
    world: Res<World>,
    handles: Option<Res<WeatherHandles>>,
    camera: Query<&GlobalTransform, With<Camera3d>>,
    mut rains: ResMut<Assets<RainMaterial>>,
    mut snows: ResMut<Assets<SnowMaterial>>,
    mut curtains: Query<
        (&mut Visibility, Has<SnowCurtain>),
        Or<(With<RainCurtain>, With<SnowCurtain>)>,
    >,
) {
    let (Some(handles), Ok(camera)) = (handles, camera.single()) else {
        return;
    };
    let eye = camera.translation();
    // none underwater, nor once high above the clouds' base
    let dry = world.get_block_at(eye.floor().as_i64vec3()) == BlockType::Water
        || eye.y > crate::clouds::CLOUD_BASE;
    let (rain, snow) = if dry {
        (0.0, 0.0)
    } else {
        (weather.rain, weather.snow)
    };
    for (mut visibility, is_snow) in &mut curtains {
        show_if(&mut visibility, if is_snow { snow } else { rain } > 0.0);
    }
    let daylight = unlit_light(&clock);
    let slant = blown(weather.wind, rain, RAIN_SLANT);
    let light = daylight * (1.0 - RAIN_GLOOM * weather.rain);
    set_params(
        &mut rains,
        &handles.rain,
        Vec4::new(rain, light, slant.x, slant.y),
    );
    let drift = blown(weather.wind, snow, SNOW_DRIFT);
    let light = daylight * (1.0 - SNOW_GLOOM * weather.snow);
    set_params(
        &mut snows,
        &handles.snow,
        Vec4::new(snow, light, drift.x, drift.y),
    );
}

// Lightning strikes this often in the heaviest rain, at the least and most, in seconds, from
// this far off, in blocks (a metre each), and its thunder rolls in at the speed of sound.
const STRIKE_SECONDS: (f32, f32) = (7.0, 22.0);
const STRIKE_DISTANCE: (f32, f32) = (30.0, 520.0);
const THUNDER_SPEED: f32 = 343.0;
// Now and then a strike lands close, within this many blocks; the rest are farther off.
const CLOSE_STRIKE_CHANCE: f64 = 0.25;
const CLOSE_STRIKE_WITHIN: f32 = 110.0;
// How long a bolt shows, in seconds.
const BOLT_SECONDS: f32 = 0.3;
// Out of a thunderstorm's core, lightning is seen in thunderstorms this far off, in blocks, as
// often as this, in seconds, and heard only from the nearer ones.
const FAR_STRIKE_REACH: f32 = 28_000.0;
const FAR_STRIKE_SECONDS: (f32, f32) = (4.0, 14.0);
const FAR_THUNDER_HEARD: f32 = 12_000.0;

#[derive(Component)]
struct Bolt {
    age: f32,
}

#[derive(Component)]
struct ThunderDue {
    in_seconds: f32,
    distance: f32,
}

// Where lightning strikes in a thunderstorm's core round `camera`: mostly somewhere ahead, where
// it's seen, and now and then close by. With how far off that is.
fn near_strike(camera: &GlobalTransform) -> (Vec3, f32) {
    let facing = camera.forward().with_y(0.0).normalize_or(Vec3::Z);
    let angle = ops::atan2(facing.z, facing.x) + rand::random_range(-0.55..0.55);
    let distance = if rand::random_bool(CLOSE_STRIKE_CHANCE) {
        rand::random_range(STRIKE_DISTANCE.0..CLOSE_STRIKE_WITHIN)
    } else {
        rand::random_range(CLOSE_STRIKE_WITHIN..STRIKE_DISTANCE.1)
    };
    let foot = camera.translation() + Vec3::new(ops::cos(angle), 0.0, ops::sin(angle)) * distance;
    (foot, distance)
}

// Where lightning strikes in a thunderstorm seen from afar, `reach` blocks round `eye` at most,
// at `t` game seconds: somewhere in one's core, with how far off that is. None while none's in sight.
fn far_strike(forecast: &Forecast, eye: Vec3, t: f64, reach: f32) -> Option<(Vec3, f32)> {
    let here = DVec2::new(eye.x as f64, eye.z as f64);
    let cores = forecast.thunderstorms_near(here, reach as f64, t);
    if cores.is_empty() {
        return None;
    }
    let core = cores[rand::random_range(0..cores.len())].as_vec2();
    let scatter = Vec2::new(rand::random_range(-1.0..1.0), rand::random_range(-1.0..1.0));
    let spot = core + scatter * 1_500.0;
    let foot = Vec3::new(spot.x, 0.0, spot.y);
    let distance = foot.with_y(eye.y).distance(eye);
    (distance > STRIKE_DISTANCE.1 && distance < reach).then_some((foot, distance))
}

#[allow(clippy::too_many_arguments)]
fn strike_lightning(
    mut commands: Commands,
    time: Res<Time>,
    clock: Res<TimeOfDay>,
    mut weather: ResMut<Weather>,
    world: Res<World>,
    forecast: Option<Res<Forecast>>,
    handles: Option<Res<WeatherHandles>>,
    camera: Query<(&GlobalTransform, &Projection), With<Camera3d>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut thunders: ResMut<Assets<Thunder>>,
    mut bolts: Query<(Entity, &mut Bolt, &mut Visibility)>,
    mut due: Query<(Entity, &mut ThunderDue)>,
) {
    let dt = time.delta_secs();
    for (entity, mut bolt, mut visibility) in &mut bolts {
        bolt.age += dt;
        // it flickers as it goes
        *visibility = if bolt.age < BOLT_SECONDS && (bolt.age * 30.0) as u32 % 3 != 1 {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
        if bolt.age > BOLT_SECONDS {
            commands.entity(entity).despawn();
        }
    }
    for (entity, mut thunder) in &mut due {
        thunder.in_seconds -= dt;
        if thunder.in_seconds <= 0.0 {
            commands.entity(entity).despawn();
            // far off, only a quiet rumble
            let volume = (STRIKE_DISTANCE.1 / thunder.distance).sqrt().min(1.0) * AMBIENCE_TRIM;
            commands.spawn((
                AudioPlayer(thunders.add(Thunder {
                    distance: thunder.distance,
                    seed: rand::random(),
                })),
                PlaybackSettings::DESPAWN.with_volume(Volume::Linear(volume)),
            ));
        }
    }

    let (Some(handles), Ok((camera, projection))) = (handles, camera.single()) else {
        return;
    };
    // in a thunderstorm's core, or watching one from afar
    let near = weather.thunder >= 0.4;
    if !near && (forecast.is_none() || weather.calm) {
        return;
    }
    weather.strike_in -= dt;
    if weather.strike_in > 0.0 {
        return;
    }
    let (soonest, latest) = if near {
        STRIKE_SECONDS
    } else {
        FAR_STRIKE_SECONDS
    };
    weather.strike_in = rand::random_range(soonest..latest);

    let eye = camera.translation();
    let strike = match forecast {
        Some(forecast) if !near => {
            // drawn short of the far plane, or it's cut off
            let far = match projection {
                Projection::Perspective(perspective) => perspective.far * 0.9,
                _ => FAR_STRIKE_REACH,
            };
            far_strike(&forecast, eye, seconds(clock.0), FAR_STRIKE_REACH.min(far))
        }
        _ => Some(near_strike(camera)),
    };
    let Some((foot, distance)) = strike else {
        return;
    };
    // from the noise, as the region there may not be worked out, and working it out here
    // would stall the frame the bolt strikes on
    let ground = world
        .region_store
        .terrain()
        .sample(foot.x.floor() as f64, foot.z.floor() as f64)
        .height
        .max(ochre_base::WATER_LEVEL as f64) as f32;
    // far off, drawn dropped with the land and clouds round the world's curve
    let drop = WORLD_CURVE.drop_at(distance, eye.y);
    let foot = foot.with_y(ground - drop);
    let top = foot.with_y(crate::clouds::CLOUD_BASE - drop);
    commands.spawn((
        Bolt { age: 0.0 },
        Mesh3d(meshes.add(bolt_mesh(top, foot, eye, distance))),
        MeshMaterial3d(handles.bolt.clone()),
        Transform::default(),
        Visibility::Visible,
        NoFrustumCulling,
        NotShadowCaster,
        NotShadowReceiver,
    ));
    // only a near strike lights everything up
    if near {
        commands.spawn(Flash {
            age: 0.0,
            flicker: rand::random_range(0.12..0.22),
        });
    }
    if distance < FAR_THUNDER_HEARD {
        commands.spawn(ThunderDue {
            in_seconds: distance / THUNDER_SPEED,
            distance,
        });
    }
}

// A forked bolt from `top` down to `foot`, as quads turned to face `eye`: a jagged trunk and a
// branch or two off it. Close up it's drawn thinner and wanders less, so it isn't a fat zigzag
// filling the view; kilometres off, wider, so it's still a few pixels across.
fn bolt_mesh(top: Vec3, foot: Vec3, eye: Vec3, distance: f32) -> Mesh {
    let scale = (distance / 160.0).clamp(0.4, 1.0).max(distance / 450.0);
    let mut positions = Vec::new();
    let mut indices: Vec<u32> = Vec::new();
    let mut segment = |a: Vec3, b: Vec3, width: f32| {
        let along = (b - a).normalize_or(Vec3::Y);
        let across = along
            .cross((eye - a).normalize_or(Vec3::Z))
            .normalize_or(Vec3::X)
            * width;
        let k = positions.len() as u32;
        positions.extend([a - across, a + across, b - across, b + across].map(|p| p.to_array()));
        indices.extend([0, 2, 1, 1, 2, 3].map(|i| k + i));
    };
    let jagged = |from: Vec3, to: Vec3, steps: usize, wander: f32| {
        let mut points = vec![from];
        for i in 1..steps {
            let t = i as f32 / steps as f32;
            let jitter = Vec3::new(
                rand::random_range(-1.0..1.0),
                0.0,
                rand::random_range(-1.0..1.0),
            ) * wander;
            points.push(from.lerp(to, t) + jitter);
        }
        points.push(to);
        points
    };
    let trunk = jagged(top, foot, 18, 14.0 * scale);
    for pair in trunk.windows(2) {
        segment(pair[0], pair[1], 1.4 * scale);
    }
    for _ in 0..rand::random_range(1..4) {
        let from = trunk[rand::random_range(3..trunk.len() - 4)];
        let reach = Vec3::new(
            rand::random_range(-60.0..60.0),
            -rand::random_range(40.0..110.0),
            rand::random_range(-60.0..60.0),
        ) * scale.max(1.0).sqrt();
        let branch = jagged(from, from + reach, 7, 8.0 * scale);
        for pair in branch.windows(2) {
            segment(pair[0], pair[1], 0.7 * scale);
        }
    }
    let count = positions.len();
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 1.0, 0.0]; count])
    .with_inserted_indices(Indices::U32(indices))
}

// How far off the rainbow hangs, in blocks: near enough to be in front of the far hills'
// fog, and how wide it is from the point opposite the sun, as real ones are.
const RAINBOW_DISTANCE: f32 = 700.0;
const RAINBOW_ANGLE: f32 = 42.0 * PI / 180.0;

fn show_rainbow(
    weather: Res<Weather>,
    clock: Res<TimeOfDay>,
    handles: Option<Res<WeatherHandles>>,
    camera: Query<&GlobalTransform, With<Camera3d>>,
    mut bows: Query<(&mut Transform, &mut Visibility), With<Rainbow>>,
    mut materials: ResMut<Assets<RainbowMaterial>>,
) {
    let (Some(handles), Ok(camera)) = (handles, camera.single()) else {
        return;
    };
    let eye = camera.translation();
    let away = -sun_direction(clock.days());
    // only by day, with the sun low enough that some of the bow clears the horizon: once
    // it's higher than the bow is wide, all of it's below
    let sun_height = -away.y;
    let strength = weather.rainbow
        * smoothstep(-0.02, 0.08, sun_height)
        * (1.0 - smoothstep(0.6, 0.68, sun_height));
    if let Ok((mut transform, mut visibility)) = bows.single_mut() {
        show_if(&mut visibility, strength > 0.0);
        if strength > 0.0 {
            *transform = Transform::from_translation(eye + away * RAINBOW_DISTANCE)
                .looking_to(away, Vec3::Y)
                .with_scale(Vec3::splat(RAINBOW_DISTANCE * ops::tan(RAINBOW_ANGLE)));
        }
    }
    let params = Vec4::new(strength, unlit_light(&clock), 0.0, 0.0);
    if materials
        .get(&handles.rainbow)
        .is_some_and(|material| material.params != params)
    {
        if let Some(mut material) = materials.get_mut(&handles.rainbow) {
            material.params = params;
        }
    }
}

// xorshift: plenty for noise
fn next_random(state: &mut u64) -> f32 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    (*state >> 40) as f32 / (1u64 << 24) as f32 * 2.0 - 1.0
}

/// One clap of thunder from `distance` blocks off: close by, a rapid run of sharp cracks over a
/// deep boom; farther off, only a softer rumble. Then a long roll that swells again as the far
/// parts of the bolt are heard, echoes, and dies away.
#[derive(Asset, TypePath)]
pub(crate) struct Thunder {
    distance: f32,
    seed: u64,
}

// How long a clap lasts, in seconds, and its loudness from the nearest strikes.
const THUNDER_SECONDS: f32 = 7.0;
const THUNDER_GAIN: f32 = 0.46;
// Strikes within the first distance are heard at full force, fading by the log out to the second.
const THUNDER_HEARD: (f32, f32) = (120.0, 520.0);
// The roll's gain from the farthest strikes and the nearest.
const ROLL_GAIN: (f32, f32) = (0.30, 1.16);
// How far back, in frames, the two echoes come from, ~120 ms and ~270 ms; the first heard in
// the other ear.
const ECHO_TAPS: (usize, usize) = (5_200, 11_800);
const ECHO_FRAMES: usize = ECHO_TAPS.1 + 1;

impl Decodable for Thunder {
    type Decoder = ThunderDecoder;

    fn decoder(&self) -> ThunderDecoder {
        // by the log of the distance, as loudness falls off
        let near = 1.0
            - (ops::ln(self.distance / THUNDER_HEARD.0)
                / ops::ln(THUNDER_HEARD.1 / THUNDER_HEARD.0))
            .clamp(0.0, 1.0);
        let mut rng = (self.seed << 1) | 1;
        let mut uniform = || next_random(&mut rng) * 0.5 + 0.5;

        // the roll swells again round 0.7, 1.5, 2.5 and 3.7 seconds in, each later one broader,
        // quieter and wider apart in the ears
        let swells: [Swell; 4] = [
            Swell {
                at: 0.72 + uniform() * 0.20,
                rise: 0.15 + uniform() * 0.06,
                decay: 0.75 + uniform() * 0.25,
                size: 0.65 + uniform() * 0.28,
                pan: (uniform() * 2.0 - 1.0) * 0.55,
            },
            Swell {
                at: 1.48 + uniform() * 0.32,
                rise: 0.18 + uniform() * 0.08,
                decay: 0.90 + uniform() * 0.30,
                size: 0.70 + uniform() * 0.32,
                pan: (uniform() * 2.0 - 1.0) * 0.75,
            },
            Swell {
                at: 2.45 + uniform() * 0.42,
                rise: 0.22 + uniform() * 0.10,
                decay: 1.05 + uniform() * 0.35,
                size: 0.50 + uniform() * 0.28,
                pan: (uniform() * 2.0 - 1.0) * 0.85,
            },
            Swell {
                at: 3.65 + uniform() * 0.50,
                rise: 0.28 + uniform() * 0.12,
                decay: 1.20 + uniform() * 0.40,
                size: 0.35 + uniform() * 0.22,
                pan: (uniform() * 2.0 - 1.0) * 0.85,
            },
        ];

        // the crack: five shocks in the first third of a second, each longer and lower than the
        // last, leaning left and right in turn
        let shocks: [Shock; 5] = [
            Shock {
                at: 0.0,
                duration: 0.0028,
                size: 4.6,
                pan: -0.15,
            },
            Shock {
                at: 0.026 + uniform() * 0.004,
                duration: 0.0042,
                size: 5.2,
                pan: 0.25,
            },
            Shock {
                at: 0.058 + uniform() * 0.008,
                duration: 0.0060,
                size: 4.8,
                pan: -0.20,
            },
            Shock {
                at: 0.105 + uniform() * 0.012,
                duration: 0.0095,
                size: 6.2,
                pan: 0.30,
            },
            Shock {
                at: 0.265 + uniform() * 0.020,
                duration: 0.0140,
                size: 5.4,
                pan: -0.35,
            },
        ];

        ThunderDecoder {
            rng,
            frame: 0,
            channel: 0,
            near,
            swells,
            shocks,
            low: [[0.0; 4]; 2],
            heave: [[0.0; 4]; 2],
            echo: Box::new([[0.0; 2]; ECHO_FRAMES]),
            echo_low: [0.0; 2],
            dc: [0.0; 2],
            air: [[0.0; 2]; 2],
        }
    }
}

// One swell in the roll: when it peaks and how long it takes to, how fast it dies, all in
// seconds, how loud, and which way it leans, -1 left to 1 right.
#[derive(Clone, Copy)]
struct Swell {
    at: f32,
    rise: f32,
    decay: f32,
    size: f32,
    pan: f32,
}

// One shock in the crack: a single cycle of a sine that dies away as it goes, sharp up then
// down. When it comes and how long it lasts, in seconds, how loud, and which way it leans.
#[derive(Clone, Copy)]
struct Shock {
    at: f32,
    duration: f32,
    size: f32,
    pan: f32,
}

pub struct ThunderDecoder {
    rng: u64,
    frame: u32,
    channel: usize,
    // 0 for the farthest strikes, 1 for the nearest
    near: f32,
    swells: [Swell; 4],
    shocks: [Shock; 5],
    // per ear: the roll's three low-pass stages, then one finding the rumble below ~25 Hz to
    // take away
    low: [[f32; 4]; 2],
    // per ear: two slow wanderings that make the roll heave and tumble, each two low-passes,
    // round 2 Hz and 7 Hz
    heave: [[f32; 4]; 2],
    // what's been heard, for the echoes, and a low-pass dulling them
    echo: Box<[[f32; 2]; ECHO_FRAMES]>,
    echo_low: [f32; 2],
    // per ear: what drifts below ~25 Hz, to take away
    dc: [f32; 2],
    // per ear: two low-passes, the air dulling the sound as it travels
    air: [[f32; 2]; 2],
}

impl Iterator for ThunderDecoder {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        let frame = self.frame as usize;
        let t = self.frame as f32 / SAMPLE_RATE as f32;
        if t >= THUNDER_SECONDS {
            return None;
        }
        let c = self.channel;
        if c == 1 {
            self.frame += 1;
        }
        self.channel = 1 - self.channel;
        let near = self.near;
        let ear = if c == 0 { -1.0 } else { 1.0 };
        let white = next_random(&mut self.rng);

        // the crack, only from nearer strikes
        let crack_amount = near * near;
        let mut crack = 0.0f32;
        if crack_amount > 0.0 && t < 0.6 {
            for shock in &self.shocks {
                let since = t - shock.at;
                if (0.0..shock.duration).contains(&since) {
                    let phase = since / shock.duration;
                    crack += ops::sin(phase * TAU)
                        * (1.0 - phase)
                        * shock.size
                        * (1.0 + 0.25 * shock.pan * ear);
                }
            }
        }
        let crack = crack * crack_amount;

        // how much the roll swells here, and how much of that is a fresh swell cresting
        let (mut swelling, mut cresting) = (0.0f32, 0.0f32);
        for swell in &self.swells {
            let since = t - swell.at + swell.rise;
            if since > 0.0 {
                let u = (since / swell.rise).min(1.0);
                let onset = u * u * (3.0 - 2.0 * u);
                let after = (since - swell.rise).max(0.0);
                let size = swell.size * onset * ops::exp(-after / swell.decay);
                swelling += size * (1.0 + 0.35 * swell.pan * ear);
                cresting += size * ops::exp(-after / 0.35);
            }
        }

        // the roll heaving and tumbling
        let [h1, h2, g1, g2] = &mut self.heave[c];
        *h1 += (white - *h1) * 0.00032;
        *h2 += (*h1 - *h2) * 0.00032;
        *g1 += (white - *g1) * 0.00095;
        *g2 += (*g1 - *g2) * 0.00095;
        let heave = (*h2 * 120.0).clamp(-0.55, 1.45);
        let tumble = (*g2 * 70.0).clamp(-0.50, 1.40);
        let ragged = (0.85 + 0.48 * heave + 0.38 * tumble.max(-0.20)).clamp(0.55, 2.20);

        // the roll: spiky noise through three low-passes, round 150-500 Hz, brighter near, early
        // on and as it heaves or swells, with the lowest rumble taken out
        let crest = (0.35 * heave.max(0.0) + 0.25 * tumble.max(0.0) + 0.50 * cresting).min(1.4);
        let cut = (0.021 + (0.015 + 0.022 * near) * ops::exp(-t / 3.2) + 0.017 * crest)
            .clamp(0.019, 0.072);
        let mut x = (white * 0.60 + white * white * white * 0.75) * ragged;
        let [l1, l2, l3, rumble] = &mut self.low[c];
        for stage in [l1, l2, l3] {
            *stage += (x - *stage) * cut;
            x = *stage;
        }
        *rumble += (x - *rumble) * 0.0035;
        let x = (x - *rumble) * ops::sqrt(0.026 / cut);
        let roll_envelope = {
            // near, it's on top of you at once; far, it fades in, slower at first
            let attack = (t / (0.42 * (1.0 - near).max(0.002))).min(1.0);
            let attack = if near < 0.3 { attack * attack } else { attack };
            let decay = ops::exp(-t / (2.4 + 0.7 * near));
            attack * decay * (0.60 + 0.65 * swelling)
        };
        let roll_gain = ROLL_GAIN.0 * ops::powf(ROLL_GAIN.1 / ROLL_GAIN.0, near);
        let mut sample = x * 17.0 * roll_envelope * roll_gain + crack;

        // two echoes off the land and clouds, dulled, the nearer the louder, each feeding the
        // next
        let tap = |delay: usize| (frame + ECHO_FRAMES - delay) % ECHO_FRAMES;
        let heard =
            self.echo[tap(ECHO_TAPS.0)][1 - c] * 0.58 + self.echo[tap(ECHO_TAPS.1)][c] * 0.42;
        self.echo_low[c] += (heard - self.echo_low[c]) * 0.038;
        let echo = self.echo_low[c];
        self.echo[frame % ECHO_FRAMES][c] = crack * 0.40 + sample * 0.20 + echo * 0.12;
        sample += echo * (0.35 + 0.30 * near);

        self.dc[c] += (sample - self.dc[c]) * 0.0035;
        let sample = (sample - self.dc[c]) * THUNDER_GAIN;
        // fades out at the very end so it doesn't click, and saturates rather than clipping
        let tail = ((THUNDER_SECONDS - t) / 0.5).min(1.0);
        let sample = (sample * tail).tanh();

        // the air dulls it: round 2.5 kHz from the nearest strikes at first, closing to round
        // 270 Hz within half a second
        let cut = (0.038 + (0.020 + 0.240 * near) * ops::exp(-t / 0.14)).clamp(0.036, 0.300);
        let [a1, a2] = &mut self.air[c];
        *a1 += (sample - *a1) * cut;
        *a2 += (*a1 - *a2) * cut;
        Some((*a2 * 1.05).clamp(-1.0, 1.0))
    }
}

impl Source for ThunderDecoder {
    fn current_span_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> ChannelCount {
        ChannelCount::new(2).unwrap()
    }

    fn sample_rate(&self) -> SampleRate {
        SampleRate::new(SAMPLE_RATE).unwrap()
    }

    fn total_duration(&self) -> Option<Duration> {
        Some(Duration::from_secs_f32(THUNDER_SECONDS))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The most of any weather (rain, snow, storm cloud, thunder or rainbow) at the camera over six
    // game days of a stormy jungle's, as `run_weather` has it, with the sky held clear or not.
    fn most_weather(calm: bool) -> f32 {
        use bevy::ecs::system::RunSystemOnce;
        let mut world = bevy::ecs::world::World::new();
        world.insert_resource(Forecast::new(4, [1.0, 0.0], |_, _| {
            ochre_base::biome::Climate {
                temperature: 0.8,
                humidity: 0.95,
            }
        }));
        world.insert_resource(Weather {
            calm,
            ..Weather::default()
        });
        world.insert_resource(TimeOfDay(0));
        world.spawn((Camera3d::default(), GlobalTransform::default()));
        let mut most = 0.0f32;
        for step in 0..6 * 48 {
            world.resource_mut::<TimeOfDay>().0 = step * crate::sky::DAY_TICKS / 48;
            world.run_system_once(run_weather).unwrap();
            let weather = world.resource::<Weather>();
            most = most
                .max(weather.precipitation())
                .max(weather.storm)
                .max(weather.thunder)
                .max(weather.rainbow);
        }
        most
    }

    #[test]
    fn test_a_calm_sky_stays_clear_through_days_of_weather() {
        assert_eq!(0.0, most_weather(true));
        // and the same weather, not calm, has stormed by then
        let most = most_weather(false);
        assert!(most > 0.5, "{most}");
    }

    #[test]
    fn test_the_rain_is_silent_without_rain_and_grows_with_it_to_the_target() {
        assert_eq!(rain_volume(0.0), 0.0);
        assert_eq!(rain_volume(-1.0), 0.0);
        let mut last = 0.0;
        for i in 1..=20 {
            let now = rain_volume(i as f32 / 20.0);
            assert!(now > last);
            last = now;
        }
        // full rain is the wanted loudness, and more rain than there is changes nothing
        let gain = 10f32.powf((RAIN_LOUDNESS.1 - RAIN_LOUDNESS.0) / 20.0);
        assert!((rain_volume(1.0) - gain).abs() < 1e-6);
        assert_eq!(rain_volume(3.0), rain_volume(1.0));
        // a light drizzle is heard but well under a storm
        assert!(rain_volume(0.05) > RAIN_SILENT && rain_volume(0.05) < 0.5 * rain_volume(1.0));
    }

    #[test]
    fn test_snow_falls_silently() {
        let snowing = Weather {
            snow: 1.0,
            ..default()
        };
        assert_eq!(rain_volume(snowing.rain), 0.0);
        assert!(snowing.sheltering());
    }

    #[test]
    fn test_rain_and_snow_blow_along_the_wind_harder_as_they_fall_harder() {
        let wind = Vec2::new(3.0, -4.0);
        for range in [RAIN_SLANT, SNOW_DRIFT] {
            let light = blown(wind, 0.0, range);
            let heavy = blown(wind, 1.0, range);
            assert!((light.normalize() - wind.normalize()).length() < 1e-5);
            assert!((light.length() - range.0).abs() < 1e-5);
            assert!((heavy.length() - range.1).abs() < 1e-5);
            assert_eq!(blown(wind, 5.0, range), heavy);
        }
        // with no wind it still blows somewhere, and snow drifts gentler than rain slants
        assert!(blown(Vec2::ZERO, 0.5, SNOW_DRIFT).length() > 0.0);
        assert!(SNOW_DRIFT.1 < RAIN_SLANT.1);
    }

    #[test]
    fn test_streaks_and_flakes_are_scattered_through_their_box() {
        let mesh = scatter_mesh(500, SNOW_BOX);
        let Some(bevy::mesh::VertexAttributeValues::Float32x3(positions)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("no positions");
        };
        assert_eq!(positions.len(), 500 * 4);
        assert_eq!(mesh.indices().map(|indices| indices.len()), Some(500 * 6));
        assert!(positions
            .iter()
            .all(|p| (Vec3::from_array(*p) / SNOW_BOX).abs().max_element() <= 0.5));
        // the four corners of each are at one spot
        assert!(positions
            .chunks(4)
            .all(|corners| corners.iter().all(|p| *p == corners[0])));
    }

    fn rms(samples: &[f32]) -> f32 {
        (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
    }

    #[test]
    fn test_thunder_ends_and_near_claps_are_louder() {
        let clap = |distance| -> Vec<f32> { Thunder { distance, seed: 9 }.decoder().collect() };
        let (near, far) = (clap(STRIKE_DISTANCE.0), clap(STRIKE_DISTANCE.1));
        assert_eq!((THUNDER_SECONDS * 44_100.0) as usize * 2, near.len());
        assert!(near.iter().all(|s| s.is_finite() && s.abs() <= 1.0));
        assert!(rms(&near) > rms(&far) * 1.5, "{} {}", rms(&near), rms(&far));
    }

    #[test]
    fn test_near_thunder_opens_with_a_bright_crack() {
        let opening = |distance| -> Vec<f32> {
            Thunder { distance, seed: 9 }
                .decoder()
                .take(44_100 / 20 * 2)
                .collect()
        };
        // how much it changes from one frame to the next, in the left ear: the brighter, the more
        let brightness = |s: &[f32]| {
            rms(&s
                .windows(3)
                .map(|w| w[2] - w[0])
                .step_by(2)
                .collect::<Vec<_>>())
        };
        let (near, far) = (opening(STRIKE_DISTANCE.0), opening(STRIKE_DISTANCE.1));
        assert!(
            rms(&near) > rms(&far) * 10.0,
            "{} {}",
            rms(&near),
            rms(&far)
        );
        assert!(
            brightness(&near) > brightness(&far) * 100.0,
            "{} {}",
            brightness(&near),
            brightness(&far)
        );
        // and it's loud from the very start
        assert!(rms(&near[..88]) > 0.1, "{}", rms(&near[..88]));
    }

    #[test]
    fn test_a_rainbow_follows_the_rain_once_it_clears() {
        // none without rain, nor while it still pours
        assert_eq!(0.0, rainbow_after(0.0, 0.0));
        assert_eq!(0.0, rainbow_after(1.0, 0.9));
        // a full one once a downpour has cleared
        assert_eq!(1.0, rainbow_after(0.9, 0.0));
        // and it fades in as the rain thins
        assert!(rainbow_after(0.9, 0.1) > 0.0 && rainbow_after(0.9, 0.1) < 1.0);
    }

    #[test]
    fn test_a_storm_darkens_the_day_before_its_rain() {
        let mut weather = Weather {
            storm: 1.0,
            ..default()
        };
        assert!(weather.gloom() > 0.5);
        weather.rain = 1.0;
        assert_eq!(1.0, weather.gloom());
        assert!(!Weather::default().sheltering() && weather.sheltering());
    }
}
