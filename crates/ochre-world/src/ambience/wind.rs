use std::{
    f32::consts::TAU,
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc,
    },
    time::Duration,
};

use bevy::{
    asset::{Asset, Assets},
    camera::Camera3d,
    math::Vec2,
    transform::components::GlobalTransform,
    audio::{AudioPlayer, ChannelCount, Decodable, PlaybackSettings, SampleRate, Source, Volume},
    ecs::{
        query::With,
        resource::Resource,
        system::{Commands, Query, Res, ResMut},
    },
    reflect::TypePath,
    time::Time,
};

use ochre_terrain::{curvature::Curvature, world::World};

use super::{smoothstep, Surroundings};

/// What the wind's sound follows, set as the camera moves and read on the audio thread.
#[derive(Resource, Clone, Default)]
pub struct WindDials {
    // how hard the wind blows where the camera is, 0..1
    strength: Dial,
    // how much of a breeze is blowing, 0..1, whatever its strength: set on the audio thread for the leaves to rustle
    breeze: Dial,
    // how much of the ground round the camera is steppe, 0..1, gliding: steppe wind never drops to still
    steppe: Dial,
    // the gust the shader has at the camera, 0..1, see `gust_at`, so the sound swells as the grass bends
    gust: Dial,
    // how sheltered the foliage is from the wind, 0..1, gliding with the camera; see `SHELTERED_SWAY`
    shelter: Dial,
}

impl WindDials {
    pub(super) fn breeze(&self) -> f32 {
        self.breeze.get()
    }
}

/// An f32 shared between threads through its bits, for sounds made as they play to read what they follow.
#[derive(Clone, Default)]
pub(crate) struct Dial(Arc<AtomicU32>);

impl Dial {
    pub(crate) fn get(&self) -> f32 {
        f32::from_bits(self.0.load(Ordering::Relaxed))
    }

    pub(crate) fn set(&self, strength: f32) {
        self.0.store(strength.to_bits(), Ordering::Relaxed);
    }
}

// How strong a breeze on the ground is, when one blows, see `SPELL_SECONDS`.
const GROUND_BREEZE: f32 = 0.1;
// How far above the ground or sea the wind reaches full strength, in blocks.
const FULL_WIND_ALTITUDE: f32 = 250.0;
// How fast the air rushes past a camera flying flat out, in blocks a second, adding to the wind.
const RUSH_SPEED: f32 = 80.0;
const RUSH_GAIN: f32 = 0.15;
// Ground wind in the heaviest rain: strong enough to keep blowing between spells, see `STEADY`.
const STORM_WIND: f32 = 0.6;
// Underwater the wind is only a murmur through the surface.
const UNDERWATER_WIND: f32 = 0.02;
// How long the wind takes to follow a change in where the camera is, in seconds.
const WIND_FOLLOW_SECONDS: f32 = 0.6;
// The least wind on the most exposed steppe, above where `STEADY` starts so it blows between spells.
const STEPPE_BREEZE: f32 = 0.5;
const STEPPE_EXPOSED: (f32, f32) = (0.3, 0.75);
// How much of its sway foliage loses in the most sheltered hollow, so grass there still bends a little.
const SHELTERED_SWAY: f32 = 0.7;
// Foliage is fully sheltered at or under the first exposure and fully in the wind from the second, level ground, up.
const SHELTER_EXPOSURE: (f32, f32) = (0.3, 0.5);
// The gust's greatest push either way, see `gust_at`.
const GUST_RANGE: f32 = 1.2;

/// The direction the wind blows, as the foliage shader takes it: a unit vector, the old fixed direction when unset.
pub fn wind_direction(prevailing: [f32; 2]) -> Vec2 {
    let wind = Vec2::from(prevailing);
    if wind.length_squared() < 0.0001 {
        Vec2::new(0.846, 0.538).normalize()
    } else {
        wind.normalize()
    }
}

/// How hard the wind pushes at `at` (x, z) at `time`, about -1.2..1.2; keep in step with `gust_at` in `foliage.wesl`.
pub fn gust_at(at: Vec2, time: f32, direction: Vec2) -> f32 {
    let along = at.dot(direction);
    0.7 * (time * 1.3 + along * 0.13).sin() + 0.5 * (time * 0.55 + along * TAU / 190.0).sin()
}

pub(crate) fn play_wind(mut commands: Commands, mut winds: ResMut<Assets<Wind>>, dials: Res<WindDials>) {
    let wind = winds.add(Wind {
        dials: dials.clone(),
    });
    commands.spawn((AudioPlayer(wind), PlaybackSettings::ONCE.with_volume(Volume::Linear(super::AMBIENCE_TRIM)),
    ));
}

/// The wind's strength from the camera's height over the ground or sea and its flying speed.
pub fn wind_strength(altitude: f32, speed: f32) -> f32 {
    // slow to pick up off the ground, so even treetop height is calm
    let height = (altitude / FULL_WIND_ALTITUDE).clamp(0.0, 1.0).powf(1.5);
    let rush = (speed / RUSH_SPEED).clamp(0.0, 1.0) * RUSH_GAIN;
    (GROUND_BREEZE + (1.0 - GROUND_BREEZE) * height + rush).min(1.0)
}

// How much of steppe's wind reaches ground this exposed: none in a hollow, all on a crest.
fn exposed(exposure: f32) -> f32 {
    smoothstep(STEPPE_EXPOSED.0, STEPPE_EXPOSED.1, exposure)
}

// Foliage is fully sheltered in a hollow and not at all from level ground up, in any biome.
fn sheltered(exposure: f32) -> f32 {
    1.0 - smoothstep(SHELTER_EXPOSURE.0, SHELTER_EXPOSURE.1, exposure)
}

pub(crate) fn update_wind(
    time: Res<Time>,
    around: Res<Surroundings>,
    dials: Res<WindDials>,
    weather: Option<Res<crate::weather::Weather>>,
    world: Res<World>,
    camera: Query<&GlobalTransform, With<Camera3d>>,
    curvature: Option<ResMut<Curvature>>,
) {
    if let Ok(camera) = camera.single() {
        let eye = camera.translation();
        let direction = wind_direction(world.region_store.terrain().prevailing_wind());
        let push = gust_at(Vec2::new(eye.x, eye.z), time.elapsed_secs_wrapped(), direction);
        dials.gust.set((push / (2.0 * GUST_RANGE) + 0.5).clamp(0.0, 1.0));
    }
    // a storm blows before and without its rain
    let storm = weather.map_or(0.0, |weather| weather.gloom()) * STORM_WIND;
    let follow = 1.0 - (-time.delta_secs() / WIND_FOLLOW_SECONDS).exp();
    let steppe = dials.steppe.get();
    dials.steppe.set(steppe + (if around.underwater { 0.0 } else { around.steppe } - steppe) * follow);
    let exposed = exposed(around.exposure);
    let shelter = dials.shelter.get();
    dials.shelter.set(shelter + (sheltered(around.exposure) - shelter) * follow);
    if let Some(mut curvature) = curvature {
        curvature.sway[0] = 1.0 - SHELTERED_SWAY * dials.shelter.get();
    }
    let wanted = if around.underwater {
        UNDERWATER_WIND
    } else {
        wind_strength(around.altitude, around.speed)
            .max(storm)
            .max(dials.steppe.get() * STEPPE_BREEZE * exposed)
    };
    let current = dials.strength.get();
    dials.strength.set(current + (wanted - current) * follow);
}

/// Wind made from noise as it plays, so it never loops: a low roar that swells with gusts, and a whistle that rises.
#[derive(Asset, TypePath)]
pub struct Wind {
    dials: WindDials,
}

impl Decodable for Wind {
    type Decoder = WindDecoder;

    fn decoder(&self) -> WindDecoder {
        WindDecoder::new(self.dials.clone(), rand::random())
    }
}

pub(crate) const SAMPLE_RATE: u32 = 44_100;
const CHANNELS: u16 = 2;
// How often the decoder rereads the strength and moves its gusts on, in frames.
const CONTROL_FRAMES: u32 = 64;
// How long the decoder takes to follow the strength, in seconds, so steps in it don't click.
const STRENGTH_GLIDE_SECONDS: f32 = 0.3;
// Wind comes in spells: seconds one takes to pick up or die away, least and most; low down it drops to nothing.
const SPELL_SECONDS: (f32, f32) = (12.0, 40.0);
// A spell's wandering value, 0..1: still air under the first, a full breeze over the second; calm a third of the time
const CALM: (f32, f32) = (0.35, 0.8);
// Wind this strong, from high up or flying fast, blows between spells: none of it at the first, all at the second.
const STEADY: (f32, f32) = (0.35, 0.8);
// The roar's low-pass cutoff in calm air and the strongest gusts, in Hz; much higher hisses like a detuned radio.
const ROAR_CUTOFF: (f32, f32) = (200.0, 600.0);
// One-pole stages in the roar's low-pass, each cutting another 6 dB an octave past the cutoff.
const ROAR_POLES: usize = 3;
// The whistle's pitch in calm air and in the strongest gusts, in Hz.
const WHISTLE_PITCH: (f32, f32) = (260.0, 820.0);
// Narrow, so the whistle is a note rather than a band of hiss.
const WHISTLE_Q: f32 = 14.0;
// Cuts the whistle's input, so it only rings with the rumble's low end too.
const WHISTLE_INPUT_CUTOFF: f32 = 1200.0;
// Quieter than the roar, and only once the wind picks up, see `WindDecoder::control`.
const WHISTLE_GAIN: f32 = 0.2;
// Grass rustling in the wind is hiss above this cutoff, in Hz, at this share of the wind's level on steppe.
const RUSTLE_CUTOFF: f32 = 2400.0;
const RUSTLE_GAIN: f32 = 0.18;
const OUTPUT_GAIN: f32 = 0.5;
// How much loudness a full gale loses, so a gale sits under the music and loops; a breeze is unchanged.
const GALE_EASE: f32 = 0.85;
// Puts a ground breeze at about -38 LUFS and a high gale at about -35 once eased by `GALE_EASE`.
const ROAR_MAKEUP: f32 = 0.34;

pub struct WindDecoder {
    dials: WindDials,
    rng: u64,
    frame: u32,
    channel: u16,
    // the strength as heard, gliding after `strength`
    level: f32,
    spell: Gust,
    flutter: Gust,
    // per channel, so the two ears hear different air
    roar: [[OnePole; ROAR_POLES]; 2],
    whistle_input: [OnePole; 2],
    whistle: [Bandpass; 2],
    // low-passes the grass's hiss is the white noise minus
    rustle: [OnePole; 2],
    rustle_gain: f32,
    // how much of each the control step last asked for; the roar's also offsets its low-pass, so cutoff changes only tone
    roar_gain: f32,
    whistle_gain: f32,
}

impl WindDecoder {
    fn new(dials: WindDials, seed: u64) -> Self {
        let level = dials.strength.get();
        let mut decoder = Self {
            dials,
            rng: seed | 1,
            frame: 0,
            channel: 0,
            level,
            spell: Gust::default(),
            flutter: Gust::default(),
            roar: [[OnePole::default(); ROAR_POLES]; 2],
            whistle_input: [OnePole::default(); 2],
            whistle: [Bandpass::default(); 2],
            rustle: [OnePole::default(); 2],
            rustle_gain: 0.0,
            roar_gain: 0.0,
            whistle_gain: 0.0,
        };
        for filter in &mut decoder.whistle_input {
            filter.tune(WHISTLE_INPUT_CUTOFF);
        }
        for filter in &mut decoder.rustle {
            filter.tune(RUSTLE_CUTOFF);
        }
        decoder.control();
        decoder
    }

    // xorshift: plenty for noise, and cheap enough to run every sample
    fn random(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        (self.rng >> 40) as f32 / (1u64 << 24) as f32
    }

    fn noise(&mut self) -> f32 {
        self.random() * 2.0 - 1.0
    }

    // Moves the gusts on and retunes the filters, every `CONTROL_FRAMES`.
    fn control(&mut self) {
        let step = CONTROL_FRAMES as f32 / SAMPLE_RATE as f32;
        let glide = 1.0 - (-step / STRENGTH_GLIDE_SECONDS).exp();
        self.level += (self.dials.strength.get() - self.level) * glide;
        let (a, b) = (self.random(), self.random());
        let spell = self.spell.advance(step, SPELL_SECONDS, a, b);
        let (a, b) = (self.random(), self.random());
        let flutter = self.flutter.advance(step, (0.2, 0.8), a, b);

        let up = smoothstep(CALM.0, CALM.1, spell);
        let steady = smoothstep(STEADY.0, STEADY.1, self.level);
        let blowing = up + (1.0 - up) * steady;
        let gusting = 0.45 + 0.55 * self.dials.gust.get();
        self.dials.breeze.set(blowing * gusting);
        // gusts swing a calm day's wind far less than a gale's
        let blow = (self.level * blowing * gusting * (0.9 + 0.1 * flutter)).clamp(0.0, 1.0);
        let cutoff = lerp(ROAR_CUTOFF, blow);
        let pitch = lerp(WHISTLE_PITCH, blow * (0.85 + 0.15 * flutter));
        for channel in 0..2 {
            for pole in &mut self.roar[channel] {
                pole.tune(cutoff);
            }
            // the ears' whistles a little apart, so it doesn't sit in the middle of the head
            self.whistle[channel].tune(pitch * (1.0 + 0.04 * channel as f32), WHISTLE_Q);
        }
        // noise through a low-pass keeps about the square root of its cutoff's share of power
        let makeup = (SAMPLE_RATE as f32 / 2.0 / cutoff).sqrt();
        let ease = 1.0 - GALE_EASE * blow;
        self.roar_gain = blow.powf(0.95) * makeup * ROAR_MAKEUP * ease;
        self.whistle_gain = WHISTLE_GAIN * blow * blow * ease;
        // the grass rustles in the gusts, and with the flutter
        self.rustle_gain = RUSTLE_GAIN * self.dials.steppe.get() * blow * (0.4 + 0.6 * flutter);
    }
}

impl Iterator for WindDecoder {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        if self.channel == 0 {
            if self.frame.is_multiple_of(CONTROL_FRAMES) {
                self.control();
            }
            self.frame = self.frame.wrapping_add(1);
        }
        let channel = self.channel as usize;
        self.channel = (self.channel + 1) % CHANNELS;

        let white = self.noise();
        let roar = self.roar[channel]
            .iter_mut()
            .fold(white, |signal, pole| pole.process(signal));
        let whistle_input = self.whistle_input[channel].process(white);
        let whistle = self.whistle[channel].process(whistle_input);
        let hiss = white - self.rustle[channel].process(white);
        let sample = roar * self.roar_gain + whistle * self.whistle_gain + hiss * self.rustle_gain;
        Some((sample * OUTPUT_GAIN).tanh())
    }
}

impl Source for WindDecoder {
    fn current_span_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> ChannelCount {
        ChannelCount::new(CHANNELS).unwrap()
    }

    fn sample_rate(&self) -> SampleRate {
        SampleRate::new(SAMPLE_RATE).unwrap()
    }

    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

fn lerp((from, to): (f32, f32), t: f32) -> f32 {
    from + (to - from) * t
}

/// A smoothly wandering value, 0..1, easing from one random target to the next over a random time in the given range.
#[derive(Default)]
pub(super) struct Gust {
    from: f32,
    to: f32,
    progress: f32,
    seconds: f32,
}

impl Gust {
    // `a` and `b` are fresh random numbers, 0..1, used only when a new target is picked.
    pub(super) fn advance(
        &mut self,
        step: f32,
        (shortest, longest): (f32, f32),
        a: f32,
        b: f32,
    ) -> f32 {
        if self.progress >= 1.0 || self.seconds == 0.0 {
            self.from = self.to;
            self.to = a;
            self.seconds = shortest + (longest - shortest) * b;
            self.progress = 0.0;
        }
        self.progress = (self.progress + step / self.seconds).min(1.0);
        let eased = ochre_base::math::ease(self.progress);
        self.from + (self.to - self.from) * eased
    }
}

#[derive(Default, Clone, Copy)]
struct OnePole {
    coefficient: f32,
    state: f32,
}

impl OnePole {
    fn tune(&mut self, cutoff: f32) {
        self.coefficient = 1.0 - (-TAU * cutoff / SAMPLE_RATE as f32).exp();
    }

    fn process(&mut self, input: f32) -> f32 {
        self.state += (input - self.state) * self.coefficient;
        self.state
    }
}

/// A state variable filter's band-pass output.
#[derive(Default, Clone, Copy)]
struct Bandpass {
    f: f32,
    damping: f32,
    low: f32,
    band: f32,
}

impl Bandpass {
    fn tune(&mut self, centre: f32, q: f32) {
        self.f = 2.0 * (std::f32::consts::PI * centre / SAMPLE_RATE as f32).sin();
        self.damping = 1.0 / q;
    }

    fn process(&mut self, input: f32) -> f32 {
        self.low += self.f * self.band;
        let high = input - self.low - self.damping * self.band;
        self.band += self.f * high;
        self.band
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wind_grows_with_altitude_and_speed_and_never_goes_silent() {
        assert_eq!(GROUND_BREEZE, wind_strength(0.0, 0.0));
        assert_eq!(GROUND_BREEZE, wind_strength(-20.0, 0.0));
        assert!(wind_strength(50.0, 0.0) > wind_strength(10.0, 0.0));
        assert_eq!(1.0, wind_strength(FULL_WIND_ALTITUDE * 2.0, 0.0));
        assert!(wind_strength(0.0, RUSH_SPEED) > wind_strength(0.0, 0.0));
        assert_eq!(1.0, wind_strength(FULL_WIND_ALTITUDE, RUSH_SPEED));
    }

    // The loudness of each second of two minutes of wind this strong.
    fn loudness(strength: f32) -> Vec<f32> {
        let shared = WindDials::default();
        shared.strength.set(strength);
        shared.gust.set(0.5);
        let second = SAMPLE_RATE as usize * CHANNELS as usize;
        let samples: Vec<f32> = WindDecoder::new(shared, 7).take(second * 120).collect();
        assert!(samples.iter().all(|s| s.is_finite() && s.abs() <= 1.0));
        samples
            .chunks(second)
            .map(|s| (s.iter().map(|s| s * s).sum::<f32>() / s.len() as f32).sqrt())
            .collect()
    }

    #[test]
    fn test_a_breeze_comes_and_goes_but_a_gale_keeps_blowing() {
        let breeze = loudness(GROUND_BREEZE);
        let loudest = breeze.iter().cloned().fold(0.0, f32::max);
        assert!(loudest > 0.005, "loudest {loudest}");
        assert!(breeze.iter().any(|&l| l < loudest * 0.05), "{breeze:?}");
        let gale = loudness(1.0);
        let quietest = gale.iter().cloned().fold(1.0, f32::min);
        assert!(quietest > loudest, "quietest {quietest}, breeze {loudest}");
    }

    // How much of the two minutes' sound is above 2 kHz-ish hiss: the energy of its differences, left ear only.
    fn hiss(strength: f32, steppe: f32) -> f32 {
        let shared = WindDials::default();
        shared.strength.set(strength);
        shared.gust.set(0.5);
        shared.steppe.set(steppe);
        let samples: Vec<f32> = WindDecoder::new(shared, 7).take(SAMPLE_RATE as usize * 2 * 60).step_by(2).collect();
        samples.windows(2).map(|w| (w[1] - w[0]).powi(2)).sum::<f32>() / samples.len() as f32
    }

    #[test]
    fn grass_rustles_on_steppe_and_not_elsewhere() {
        let (steppe, grassland) = (hiss(STEPPE_BREEZE, 1.0), hiss(STEPPE_BREEZE, 0.0));
        assert!(steppe > grassland * 1.25, "steppe {steppe}, grassland {grassland}");
    }

    #[test]
    fn steppe_wind_never_drops_to_still() {
        // a ground breeze comes and goes, and the steppe's floor is over where the wind stops dropping out between spells
        assert!(STEPPE_BREEZE > STEADY.0 && STEPPE_BREEZE > GROUND_BREEZE);
        let steady = loudness(STEPPE_BREEZE);
        let quiet = steady.iter().cloned().fold(1.0, f32::min);
        assert!(quiet > 0.001, "quietest second {quiet}");
    }

    #[test]
    fn the_gust_stays_in_range_and_depends_on_where_and_when() {
        let direction = wind_direction([0.0, 0.0]);
        assert!((direction.length() - 1.0).abs() < 1e-5);
        let east = wind_direction([3.0, 0.0]);
        assert_eq!(Vec2::X, east);
        let (mut low, mut high) = (f32::MAX, f32::MIN);
        for t in 0..400 {
            for p in 0..40 {
                let push = gust_at(Vec2::new(p as f32 * 23.0, p as f32 * 7.0), t as f32 * 0.37, east);
                (low, high) = (low.min(push), high.max(push));
            }
        }
        assert!(low >= -GUST_RANGE && high <= GUST_RANGE && low < -1.0 && high > 1.0, "{low} {high}");
        assert_ne!(gust_at(Vec2::ZERO, 1.0, east), gust_at(Vec2::new(60.0, 0.0), 1.0, east));
        assert_eq!(gust_at(Vec2::new(0.0, 80.0), 2.0, east), gust_at(Vec2::new(0.0, -300.0), 2.0, east), "no change across the wind");
    }

    #[test]
    fn the_steppe_wind_is_gone_in_a_hollow_and_full_on_a_crest() {
        assert_eq!(0.0, exposed(0.1));
        assert!(exposed(0.5) > 0.2 && exposed(0.5) < 0.6);
        assert_eq!(1.0, exposed(0.9));
    }

    #[test]
    fn foliage_is_sheltered_in_a_hollow_and_not_on_level_ground() {
        assert_eq!(1.0, sheltered(0.2));
        assert!(sheltered(0.4) > 0.2 && sheltered(0.4) < 0.8);
        assert_eq!(0.0, sheltered(0.5));
        assert_eq!(0.0, sheltered(0.9));
    }
}
