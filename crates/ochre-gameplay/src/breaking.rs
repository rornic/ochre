use bevy::{
    light::{NotShadowCaster, NotShadowReceiver},
    log::info,
    prelude::*,
};

use crate::player::{cursor_grabbed, grab_cursor};
use bevy::math::I64Vec3;
use ochre_base::block::BlockType;
use ochre_terrain::{
    raycast::{raycast, RayHit},
    world::World,
};

/// How far from the eye the player can reach a block, in blocks.
pub const REACH: f32 = 7.0;
const BREAK_BUTTON: MouseButton = MouseButton::Left;
// How far out from the block the outline stands, so it isn't lost in the block's faces, and how
// thick its edges are, in blocks.
const OUTLINE_GAP: f32 = 0.004;
const OUTLINE_WIDTH: f32 = 0.012;
// How far a dig reaches from where it lands, in blocks: loose ground digs wide like a spade, rock in smaller bites.
const SOFT_BRUSH: f32 = 1.5;
const ROCK_BRUSH: f32 = 1.0;
// How far into the ground from the face it lands on a dig is centred, in blocks.
const BRUSH_DEPTH: f32 = 0.3;
// How much each block's distance from the dig is jittered, in blocks, so a hole isn't a perfect ball.
const BRUSH_ROUGHNESS: f32 = 0.35;
// Seconds between digs while the button is held.
const DIG_REPEAT: f32 = 0.25;

/// The player looks at blocks, outlined, and breaks them with a click.
pub struct BreakingPlugin;

impl Plugin for BreakingPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LookedAt>()
            .init_resource::<DigTimer>()
            .add_systems(Startup, spawn_outline)
            // what's broken is looked past at once, and outlined the same frame
            .add_systems(
                Update,
                (
                    // the click that grabs the cursor doesn't break a block too
                    break_looked_at.run_if(cursor_grabbed).before(grab_cursor),
                    look_at_block,
                    show_outline,
                )
                    .chain(),
            );
    }
}

/// The block the player's looking at in reach, and what it is.
#[derive(Resource, Default, Debug)]
pub(crate) struct LookedAt(pub Option<(RayHit, BlockType)>);

/// The first block from `eye` along `forward`, past air and water, within `REACH`.
pub(crate) fn block_looked_at(world: &World, eye: Vec3, forward: Vec3) -> Option<(RayHit, BlockType)> {
    raycast(eye, forward, REACH, |block| {
        world.get_block_at(block).is_targetable()
    })
    .map(|hit| (hit, world.get_block_at(hit.block)))
}

fn look_at_block(
    world: Option<Res<World>>,
    camera: Query<&GlobalTransform, With<Camera3d>>,
    mut looked_at: ResMut<LookedAt>,
) {
    let (Some(world), Ok(camera)) = (world, camera.single()) else {
        looked_at.0 = None;
        return;
    };
    looked_at.0 = block_looked_at(&world, camera.translation(), *camera.forward());
}

// Seconds until a held dig digs again.
#[derive(Resource, Default)]
struct DigTimer(f32);

// What the brush digs: the ground, which the sculpted surface wraps, and not trees.
fn ground(block: BlockType) -> bool {
    block.is_solid() && !matches!(block, BlockType::Leaves | BlockType::Log)
}

fn loose(block: BlockType) -> bool {
    matches!(
        block,
        BlockType::Grass | BlockType::Sand | BlockType::Dirt | BlockType::Snow | BlockType::Gravel | BlockType::Clay
    )
}

// The blocks a dig at `centre` takes: those `digs` allows within a roughened `radius`, and always the one it landed on.
pub(crate) fn brush(centre: Vec3, radius: f32, landed: I64Vec3, digs: impl Fn(I64Vec3) -> bool) -> Vec<I64Vec3> {
    let reach = (radius + BRUSH_ROUGHNESS).ceil() as i64;
    let middle = centre.round().as_i64vec3();
    let jitter = |b: I64Vec3| {
        let mut v = (b.x as u32).wrapping_mul(0x9E37_79B1) ^ (b.y as u32).wrapping_mul(0x85EB_CA77) ^ (b.z as u32).wrapping_mul(0xC2B2_AE3D);
        v ^= v >> 15;
        v = v.wrapping_mul(0x2C1B_3C6D);
        v ^= v >> 12;
        ((v & 0xFFFF) as f32 / 32767.5 - 1.0) * BRUSH_ROUGHNESS
    };
    let mut dug: Vec<I64Vec3> = (-reach..=reach)
        .flat_map(|x| (-reach..=reach).flat_map(move |y| (-reach..=reach).map(move |z| middle + I64Vec3::new(x, y, z))))
        .filter(|&b| b == landed || b.as_vec3().distance(centre) + jitter(b) <= radius)
        .filter(|&b| digs(b))
        .collect();
    if !dug.contains(&landed) && digs(landed) {
        dug.push(landed);
    }
    dug
}

fn break_looked_at(
    time: Res<Time>,
    buttons: Res<ButtonInput<MouseButton>>,
    looked_at: Res<LookedAt>,
    camera: Query<&GlobalTransform, With<Camera3d>>,
    mut timer: ResMut<DigTimer>,
    world: Option<ResMut<World>>,
) {
    let (Some(mut world), Some((hit, kind))) = (world, looked_at.0) else {
        return;
    };
    // a click digs at once, and holding on digs again every `DIG_REPEAT`
    if buttons.just_pressed(BREAK_BUTTON) {
        timer.0 = DIG_REPEAT;
    } else if buttons.pressed(BREAK_BUTTON) {
        timer.0 -= time.delta_secs();
        if timer.0 > 0.0 {
            return;
        }
        timer.0 += DIG_REPEAT;
    } else {
        return;
    }
    // trees and plants come away a block at a time, the ground in a brush round where the dig lands
    if !ground(kind) {
        if let Some(broken) = world.break_block(hit.block) {
            info!("broke {} at {}", broken.name(), hit.block);
        }
        return;
    }
    let Ok(eye) = camera.single() else {
        return;
    };
    let landed = eye.translation() + *eye.forward() * hit.distance;
    let centre = landed - hit.face.as_vec3() * BRUSH_DEPTH;
    let radius = if loose(kind) { SOFT_BRUSH } else { ROCK_BRUSH };
    let dug = brush(centre, radius, hit.block, |b| {
        let block = world.get_block_at(b);
        ground(block) && block.is_breakable()
    });
    for &b in &dug {
        world.break_block(b);
        // a plant on dug ground goes with it
        if world.get_block_at(b + I64Vec3::Y).is_flora() {
            world.break_block(b + I64Vec3::Y);
        }
    }
    info!("dug {} blocks of {} round {}", dug.len(), kind.name(), hit.block);
}

#[derive(Component)]
struct BlockOutline;

// The edges of a block, as thin bars, centred on the origin.
fn outline_mesh() -> Mesh {
    let half = 0.5 + OUTLINE_GAP + OUTLINE_WIDTH / 2.0;
    let mut outline: Option<Mesh> = None;
    for axis in 0..3 {
        let mut size = Vec3::splat(OUTLINE_WIDTH);
        size[axis] = 2.0 * half + OUTLINE_WIDTH;
        for corner in [(-1.0, -1.0), (-1.0, 1.0), (1.0, -1.0), (1.0, 1.0)] {
            let mut centre = Vec3::ZERO;
            centre[(axis + 1) % 3] = corner.0 * half;
            centre[(axis + 2) % 3] = corner.1 * half;
            let bar = Cuboid::from_size(size).mesh().build().translated_by(centre);
            match &mut outline {
                Some(mesh) => mesh.merge(&bar).expect("cuboids share attributes"),
                None => outline = Some(bar),
            }
        }
    }
    outline.expect("twelve edges")
}

fn spawn_outline(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.spawn((
        BlockOutline,
        Mesh3d(meshes.add(outline_mesh())),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.05, 0.05, 0.05),
            unlit: true,
            fog_enabled: false,
            ..default()
        })),
        Transform::default(),
        NotShadowCaster,
        NotShadowReceiver,
        Visibility::Hidden,
    ));
}

fn show_outline(
    looked_at: Res<LookedAt>,
    mut outline: Query<(&mut Transform, &mut Visibility), With<BlockOutline>>,
) {
    let Ok((mut transform, mut visibility)) = outline.single_mut() else {
        return;
    };
    match looked_at.0 {
        // a cube round a log that is drawn as a bowed prism would look wrong
        Some((_, BlockType::Log)) => {
            visibility.set_if_neq(Visibility::Hidden);
        }
        Some((hit, _)) => {
            // blocks are drawn centred on their coordinates
            transform.translation = hit.block.as_vec3();
            visibility.set_if_neq(Visibility::Inherited);
        }
        None => {
            visibility.set_if_neq(Visibility::Hidden);
        }
    }
}

#[cfg(test)]
mod tests {
    use bevy::math::{I64Vec3, U16Vec3, Vec3};

    use super::{block_looked_at, brush, outline_mesh, REACH, ROCK_BRUSH, SOFT_BRUSH};
    use ochre_base::block::BlockType;
    use ochre_terrain::chunk::{ChunkCoordinate, ChunkData};
    use ochre_terrain::world::World;

    // A chunk at the origin with a column of blocks, from the bottom up, at x = z = 4.
    fn world_with_column(column: &[BlockType]) -> World {
        let mut world = World::with_terrain(1, 16, ochre_world::worldmap::test_terrain(1));
        let mut data = ChunkData::default();
        for (y, &block) in column.iter().enumerate() {
            data.set_block_at(U16Vec3::new(4, y as u16, 4), block);
        }
        world.insert_chunk(ChunkCoordinate(I64Vec3::ZERO), data);
        world
    }

    #[test]
    fn test_looks_through_water_to_the_block_under_it() {
        use BlockType::*;
        let world = world_with_column(&[Rock, Sand, Water, Water]);
        let eye = Vec3::new(4.0, 6.0, 4.0);
        let (hit, block) = block_looked_at(&world, eye, Vec3::NEG_Y).unwrap();
        assert_eq!((I64Vec3::new(4, 1, 4), Sand), (hit.block, block));
        assert_eq!(I64Vec3::Y, hit.face);
    }

    #[test]
    fn test_nothing_is_looked_at_out_of_reach() {
        let world = world_with_column(&[BlockType::Rock]);
        let eye = Vec3::new(4.0, REACH + 1.0, 4.0);
        assert!(block_looked_at(&world, eye, Vec3::NEG_Y).is_none());
        let eye = Vec3::new(4.0, REACH, 4.0);
        assert!(block_looked_at(&world, eye, Vec3::NEG_Y).is_some());
    }

    #[test]
    fn test_compacted_stone_is_looked_at_but_not_broken() {
        use BlockType::*;
        let mut world = world_with_column(&[CompactedRock, Grass, TallGrass]);
        let eye = Vec3::new(4.0, 5.0, 4.0);
        let mut broken = Vec::new();
        while let Some((hit, _)) = block_looked_at(&world, eye, Vec3::NEG_Y) {
            match world.break_block(hit.block) {
                Some(block) => broken.push(block),
                None => break,
            }
        }
        assert_eq!(vec![TallGrass, Grass], broken);
        let (hit, block) = block_looked_at(&world, eye, Vec3::NEG_Y).unwrap();
        assert_eq!((I64Vec3::new(4, 0, 4), CompactedRock), (hit.block, block));
    }

    #[test]
    fn test_outline_wraps_the_block() {
        let mesh = outline_mesh();
        let positions = mesh
            .attribute(bevy::mesh::Mesh::ATTRIBUTE_POSITION)
            .unwrap()
            .as_float3()
            .unwrap();
        // twelve bars of 24 vertices
        assert_eq!(12 * 24, positions.len());
        let reach = positions
            .iter()
            .flatten()
            .fold(0.0f32, |most, &p| most.max(p.abs()));
        assert!(reach > 0.5 && reach < 0.55, "{reach}");
    }

    #[test]
    fn test_a_dig_takes_a_rough_ball_of_what_it_may_and_always_the_block_it_landed_on() {
        let everything = |_: I64Vec3| true;
        let soft = brush(Vec3::ZERO, SOFT_BRUSH, I64Vec3::ZERO, everything);
        let rock = brush(Vec3::ZERO, ROCK_BRUSH, I64Vec3::ZERO, everything);
        // a ball of radius 1.5 holds 19 block centres and one of 1 holds 7, the roughness taking or adding some
        assert!((8..=40).contains(&soft.len()), "{}", soft.len());
        assert!((3..=19).contains(&rock.len()), "{}", rock.len());
        assert!(soft.len() > rock.len());
        assert!(soft.iter().all(|b| b.as_vec3().length() <= SOFT_BRUSH + 0.4));
        // what can't be dug stays, but the block landed on goes whatever its distance
        let below = brush(Vec3::ZERO, SOFT_BRUSH, I64Vec3::new(0, 3, 0), |b| b.y <= 0 || b.y == 3);
        assert!(below.iter().all(|b| b.y <= 0 || b.y == 3));
        assert!(below.contains(&I64Vec3::new(0, 3, 0)));
    }
}
