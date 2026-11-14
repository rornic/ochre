//! The sound of the player's feet, one clip set per ground, played as the head bob dips.

use bevy::{
    app::{App, Plugin, Startup, Update},
    asset::{AssetServer, Handle},
    audio::{AudioPlayer, AudioSource, PlaybackSettings, Volume},
    ecs::{
        message::{Message, MessageReader},
        query::With,
        resource::Resource,
        system::{Commands, Local, Query, Res},
    },
    math::Vec3,
    transform::components::Transform,
};
use ochre_base::block::BlockType;
use ochre_terrain::{
    frost::{frost_cover, FrostSwitch},
    world::World,
};

use crate::player::Player;

#[derive(Clone, Copy)]
pub enum Surface {
    Rock,
    Grass,
    Wood,
    Ice,
    Snow,
    Sand,
}

impl Surface {
    const ALL: [Surface; 6] = [
        Surface::Rock,
        Surface::Grass,
        Surface::Wood,
        Surface::Ice,
        Surface::Snow,
        Surface::Sand,
    ];

    fn name(self) -> &'static str {
        match self {
            Surface::Rock => "rock",
            Surface::Grass => "grass",
            Surface::Wood => "wood",
            Surface::Ice => "ice",
            Surface::Snow => "snow",
            Surface::Sand => "sand",
        }
    }

    // How loud a step on this ground is, as a share of the common level: the snow clips are cut much louder than the rest.
    fn gain(self) -> f32 {
        match self {
            Surface::Snow => SNOW_GAIN,
            _ => 1.0,
        }
    }

    // What a block sounds like underfoot, none for the ones nothing stands on.
    fn of(block: BlockType) -> Option<Surface> {
        match block {
            BlockType::Grass | BlockType::Dirt | BlockType::Clay | BlockType::Leaves => {
                Some(Surface::Grass)
            }
            BlockType::Sand => Some(Surface::Sand),
            BlockType::Snow => Some(Surface::Snow),
            BlockType::Ice => Some(Surface::Ice),
            BlockType::Log => Some(Surface::Wood),
            _ if block.is_solid() => Some(Surface::Rock),
            _ => None,
        }
    }

    /// Whether `feet` are pushing through grass or other ground flora.
    fn in_flora(world: &World, feet: Vec3) -> bool {
        world
            .get_block_at((feet + Vec3::Y * 0.1).round().as_i64vec3())
            .is_flora()
    }

    /// What the ground under `feet` sounds like, none when the feet are in water.
    pub fn under(world: &World, feet: Vec3) -> Option<Surface> {
        let at = |offset: f32| world.get_block_at((feet + Vec3::Y * offset).round().as_i64vec3());
        if at(0.1) == BlockType::Water {
            return None;
        }
        Surface::of(at(-0.6))
    }
}

/// A foot comes down on `surface`.
#[derive(Message)]
pub struct Footstep {
    pub surface: Surface,
    // how hard it lands, 1 for a walking step
    pub loudness: f32,
}

// Clips cut for each surface, as `sounds/footsteps/<surface>_<1..=VARIANTS>.ogg`.
const VARIANTS: usize = 4;
// Volume of a walking step before its jitter, and how much louder a sprinting one is.
const VOLUME: f32 = 0.15;
pub const SPRINT_LOUDER: f32 = 1.25;
const VOLUME_JITTER: (f32, f32) = (0.75, 1.0);
const PITCH_JITTER: (f32, f32) = (0.92, 1.08);
// Rustle clips as `sounds/footsteps/foliage_<1..=RUSTLES>.ogg`, played over a step through flora, and their volume as a share of a step's.
const RUSTLES: usize = 3;
const RUSTLE_VOLUME: f32 = 0.08;
// Snow steps as a share of the others' volume, about -10 dB.
const SNOW_GAIN: f32 = 0.3;
// The frost cover, see `frost_cover`, from which ground rings like ice underfoot: tundra caps at 0.5, and its spawn area reads 0.33 to 0.42.
const ICY_FROST: f32 = 0.35;

#[derive(Resource)]
struct Footsteps {
    steps: [[Handle<AudioSource>; VARIANTS]; Surface::ALL.len()],
    rustles: [Handle<AudioSource>; RUSTLES],
}

pub struct FootstepPlugin;

impl Plugin for FootstepPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<Footstep>()
            .add_systems(Startup, load_footsteps)
            .add_systems(Update, play_footsteps);
    }
}

fn load_footsteps(mut commands: Commands, assets: Res<AssetServer>) {
    commands.insert_resource(Footsteps {
        steps: Surface::ALL.map(|surface| {
            std::array::from_fn(|i| {
                assets.load(format!("sounds/footsteps/{}_{}.ogg", surface.name(), i + 1))
            })
        }),
        rustles: std::array::from_fn(|i| {
            assets.load(format!("sounds/footsteps/foliage_{}.ogg", i + 1))
        }),
    });
}

#[allow(clippy::too_many_arguments)]
fn play_footsteps(
    mut commands: Commands,
    mut steps: MessageReader<Footstep>,
    clips: Res<Footsteps>,
    world: Res<World>,
    frost: Option<Res<FrostSwitch>>,
    player: Query<&Transform, With<Player>>,
    mut last: Local<usize>,
    mut last_rustle: Local<usize>,
) {
    let frost_on = frost.is_some_and(|frost| frost.0);
    for step in steps.read() {
        let feet = player.single().ok().map(|player| player.translation);
        let icy = frost_on
            && !matches!(step.surface, Surface::Snow | Surface::Ice)
            && feet.is_some_and(|feet| frost_cover(&world, feet) >= ICY_FROST);
        let surface = if icy { Surface::Ice } else { step.surface };
        // never the clip just played, which sounds like a stutter
        *last = (*last + 1 + rand::random_range(0..VARIANTS - 1)) % VARIANTS;
        commands.spawn((
            AudioPlayer(clips.steps[surface as usize][*last].clone()),
            PlaybackSettings {
                volume: Volume::Linear(
                    VOLUME
                        * surface.gain()
                        * step.loudness
                        * rand::random_range(VOLUME_JITTER.0..VOLUME_JITTER.1),
                ),
                speed: rand::random_range(PITCH_JITTER.0..PITCH_JITTER.1),
                ..PlaybackSettings::DESPAWN
            },
        ));
        if feet.is_some_and(|feet| Surface::in_flora(&world, feet)) {
            *last_rustle = (*last_rustle + 1 + rand::random_range(0..RUSTLES - 1)) % RUSTLES;
            commands.spawn((
                AudioPlayer(clips.rustles[*last_rustle].clone()),
                PlaybackSettings {
                    volume: Volume::Linear(
                        RUSTLE_VOLUME
                            * step.loudness
                            * rand::random_range(VOLUME_JITTER.0..VOLUME_JITTER.1),
                    ),
                    speed: rand::random_range(PITCH_JITTER.0..PITCH_JITTER.1),
                    ..PlaybackSettings::DESPAWN
                },
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_ground_has_a_surface_and_nothing_else_does() {
        assert!(matches!(
            Surface::of(BlockType::Basalt),
            Some(Surface::Rock)
        ));
        assert!(matches!(Surface::of(BlockType::Snow), Some(Surface::Snow)));
        assert!(Surface::of(BlockType::Water).is_none());
        assert!(Surface::of(BlockType::Air).is_none());
        assert!(Surface::of(BlockType::Fern).is_none());
    }
}
