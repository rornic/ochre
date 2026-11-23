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
    frost::{frost_cover, FrostOn},
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

#[derive(Message)]
pub struct Footstep {
    pub surface: Surface,
    // how hard it lands, 1 for a walking step
    pub loudness: f32,
    // where it lands, played from there in stereo; none for the local player's own, heard evenly
    pub at: Option<Vec3>,
}

// Clips cut for each surface, as `sounds/footsteps/<surface>_<1..=VARIANTS>.ogg`.
const VARIANTS: usize = 4;
// Walking step volume before jitter, and the sprint multiplier. Clips sit at -28 LUFS (grass -34): steps play at -52.
const VOLUME: f32 = 0.06;
pub(crate) const SPRINT_LOUDER: f32 = 1.25;
const VOLUME_JITTER: (f32, f32) = (0.75, 1.0);
const PITCH_JITTER: (f32, f32) = (0.92, 1.08);
// Rustle clips `sounds/footsteps/foliage_<1..=RUSTLES>.ogg`, played over a step through flora, at a share of a step
const RUSTLES: usize = 3;
const RUSTLE_VOLUME: f32 = 0.02;
// The frost cover from which ground rings like ice underfoot: tundra caps at 0.5, its spawn area reads 0.33 to 0.42.
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

/// The clip variants last played, so none plays twice running.
#[derive(Default)]
struct Recent {
    step: usize,
    rustle: usize,
}

fn play_footsteps(
    mut commands: Commands,
    mut steps: MessageReader<Footstep>,
    clips: Res<Footsteps>,
    world: Res<World>,
    frost: Option<Res<FrostOn>>,
    player: Query<&Transform, With<Player>>,
    mut recent: Local<Recent>,
) {
    let frost_on = frost.is_some();
    for step in steps.read() {
        let feet = step
            .at
            .or_else(|| player.single().ok().map(|player| player.translation));
        let icy = frost_on
            && !matches!(step.surface, Surface::Snow | Surface::Ice)
            && feet.is_some_and(|feet| frost_cover(&world, feet) >= ICY_FROST);
        let surface = if icy { Surface::Ice } else { step.surface };
        // never the clip just played, which sounds like a stutter
        recent.step = (recent.step + 1 + rand::random_range(0..VARIANTS - 1)) % VARIANTS;
        let clip = clips.steps[surface as usize][recent.step].clone();
        play(&mut commands, clip, VOLUME * step.loudness, step.at);
        if feet.is_some_and(|feet| Surface::in_flora(&world, feet)) {
            recent.rustle = (recent.rustle + 1 + rand::random_range(0..RUSTLES - 1)) % RUSTLES;
            let clip = clips.rustles[recent.rustle].clone();
            play(&mut commands, clip, RUSTLE_VOLUME * step.loudness, step.at);
        }
    }
}

// One clip with its volume and pitch varied, from `at` if it has one.
fn play(commands: &mut Commands, clip: Handle<AudioSource>, volume: f32, at: Option<Vec3>) {
    let mut sound = commands.spawn((
        AudioPlayer(clip),
        PlaybackSettings {
            volume: Volume::Linear(volume * rand::random_range(VOLUME_JITTER.0..VOLUME_JITTER.1)),
            speed: rand::random_range(PITCH_JITTER.0..PITCH_JITTER.1),
            spatial: at.is_some(),
            ..PlaybackSettings::DESPAWN
        },
    ));
    if let Some(at) = at {
        sound.insert(Transform::from_translation(at));
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

    // The sounds a step makes, as whether each is spatial and where, with a fern at (5, 5, 5) and the player at `player`.
    fn sounds(at: Option<Vec3>, player: Vec3) -> Vec<(bool, Option<Vec3>)> {
        use bevy::{
            math::{I64Vec3, U16Vec3},
            prelude::App,
        };
        use ochre_terrain::chunk::{ChunkCoordinate, ChunkData};
        let mut world = World::with_terrain(1, 16, ochre_world::worldmap::test_terrain(1));
        let mut data = ChunkData::default();
        data.set_block_at(U16Vec3::new(5, 5, 5), BlockType::Fern);
        world.insert_chunk(ChunkCoordinate(I64Vec3::ZERO), data);
        let mut app = App::new();
        app.add_message::<Footstep>()
            .insert_resource(world)
            .insert_resource(Footsteps {
                steps: std::array::from_fn(|_| std::array::from_fn(|_| Handle::default())),
                rustles: std::array::from_fn(|_| Handle::default()),
            })
            .add_systems(Update, play_footsteps);
        app.world_mut()
            .spawn((Player::default(), Transform::from_translation(player)));
        app.world_mut().write_message(Footstep {
            surface: Surface::Rock,
            loudness: 1.0,
            at,
        });
        app.update();
        let world = app.world_mut();
        let mut query = world.query::<(&PlaybackSettings, Option<&Transform>, &AudioPlayer)>();
        query
            .iter(world)
            .map(|(settings, at, _)| (settings.spatial, at.map(|t| t.translation)))
            .collect()
    }

    const IN_FERN: Vec3 = Vec3::new(5.0, 4.9, 5.0);
    const BARE: Vec3 = Vec3::new(9.0, 4.9, 9.0);

    #[test]
    fn a_step_plays_from_where_it_lands() {
        let heard = sounds(Some(IN_FERN), BARE);
        assert_eq!(heard.len(), 2, "a step and a rustle: {heard:?}");
        assert!(
            heard.iter().all(|h| *h == (true, Some(IN_FERN))),
            "{heard:?}"
        );
        assert_eq!(sounds(None, BARE), vec![(false, None)]);
    }

    #[test]
    fn what_a_step_lands_in_decides_its_rustle_not_where_the_player_stands() {
        assert_eq!(
            sounds(Some(IN_FERN), BARE).len(),
            2,
            "a step through ferns beside a player on bare ground"
        );
        assert_eq!(
            sounds(Some(BARE), IN_FERN).len(),
            1,
            "a step on bare ground beside a player in ferns"
        );
        assert_eq!(
            sounds(None, IN_FERN).len(),
            2,
            "the player's own step through ferns"
        );
    }
}
