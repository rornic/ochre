use bevy::{
    ecs::component::Component,
    log::info,
    math::{BVec3, I64Vec3, Quat, Vec2, Vec3},
    transform::components::Transform,
};
use ochre_base::block::BlockType;
use ochre_terrain::{chunk::ChunkCoordinate, floor::Floor, group::ChunkGroup, lod::LodKey, world::World};

use crate::collision::{move_and_collide, move_and_step, Aabb};
use crate::contact::{Body, Walker};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Movement {
    /// Flying anywhere, through anything, the way the camera is facing.
    #[default]
    Fly,
    /// Walking under gravity, kept out of solid blocks.
    Walk,
}

/// What the body remembers from one step to the next. Position is the body's `Transform`.
#[derive(Component, Debug, Default, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CharacterState {
    pub movement: Movement,
    pub velocity: Vec3,
    pub grounded: bool,
    // whether it walked into a wall last step, so swimming against a bank climbs out
    against_wall: bool,
    // seconds before the next step up is allowed, so a flight of steps isn't taken at a sprint
    step_cooldown: f32,
}

impl CharacterState {
    pub fn new(movement: Movement) -> Self {
        Self { movement, ..Self::default() }
    }
}

/// What the player wants this step. The only thing a step reads besides the state and the world.
#[derive(Component, Debug, Default, Clone, PartialEq)]
pub struct MovementInput {
    /// Right and back, each -1, 0 or 1.
    pub across: Vec2,
    /// Up while flying, -1, 0 or 1.
    pub lift: f32,
    pub sprint: bool,
    /// Held, not pressed: a jump repeats while Space is down and the body is grounded.
    pub jump: bool,
    /// Flips between flying and walking on the next step, which clears it.
    pub toggle_fly: bool,
    /// Radians. The body faces `yaw`, and flying goes the way `yaw` and `pitch` face.
    pub yaw: f32,
    pub pitch: f32,
}

// Turns about an axis through libm: platform sin and cos differ in the last bit, and server and client must agree.
fn about_y(angle: f32) -> Quat {
    let (sin, cos) = libm::sincosf(angle * 0.5);
    Quat::from_xyzw(0.0, sin, 0.0, cos)
}

fn about_x(angle: f32) -> Quat {
    let (sin, cos) = libm::sincosf(angle * 0.5);
    Quat::from_xyzw(sin, 0.0, 0.0, cos)
}

/// What a step did that the player sees or hears, for the presentation to turn into bob, sound and view kick.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct StepEvents {
    // how far a step up lifted the body, else zero
    pub rose: f32,
    // how fast it was falling on the step it landed, else zero
    pub landed: f32,
    // where it pushed off on the step it jumped
    pub jumped: Option<Vec3>,
}

/// What a step collides with. A server and a client must answer the same for the same position, or their steps part.
pub trait MovementEnvironment {
    fn block(&self, at: I64Vec3) -> BlockType;
    /// Whether the block's chunk is loaded. Ground not loaded yet is solid, so the body waits for it.
    fn generated(&self, at: I64Vec3) -> bool;
    /// The sculpted ground's triangles between two corners, in world units, in a fixed order.
    fn triangles(&self, min: Vec3, max: Vec3) -> Vec<[Vec3; 3]>;
    /// The highest sculpted surface from `below` to `above` a point, facing up at least `up`.
    fn ground(&self, at: Vec3, below: f32, above: f32, up: f32) -> Option<f32>;
}

/// The world's blocks and the sculpted ground of the chunk groups round the body.
pub struct WorldEnvironment<'a> {
    world: &'a World,
    // sorted by key, so the triangles come in the same order whatever order the groups were found in
    grounds: Vec<(Vec3, &'a Floor)>,
}

impl<'a> WorldEnvironment<'a> {
    /// `grounds` are the groups' keys, where each sits and its floor.
    pub fn new(world: &'a World, grounds: impl IntoIterator<Item = (LodKey, Vec3, &'a Floor)>) -> Self {
        let mut grounds: Vec<_> = grounds.into_iter().collect();
        grounds.sort_by_key(|(key, _, _)| (key.level, key.coord.to_array()));
        Self { world, grounds: grounds.into_iter().map(|(_, at, floor)| (at, floor)).collect() }
    }
}

/// The group a body at `at` is in, whose ring of neighbours is where it can collide.
pub fn home_group(at: Vec3) -> LodKey {
    LodKey::containing(ChunkCoordinate::containing((at + BLOCK_CORNER).floor().as_i64vec3()), 1)
}

impl<'a> WorldEnvironment<'a> {
    /// The environment of a body at `at`: the floors of the groups at its level within one of its home group.
    pub fn around(world: &'a World, at: Vec3, groups: impl IntoIterator<Item = (&'a ChunkGroup, &'a Transform, &'a Floor)>) -> Self {
        let home = home_group(at);
        let near = groups.into_iter().filter(|(group, _, _)| group.key.level == home.level && (group.key.coord - home.coord).abs().max_element() <= 1);
        Self::new(world, near.map(|(group, at, floor)| (group.key, at.translation, floor)))
    }
}

impl MovementEnvironment for WorldEnvironment<'_> {
    fn block(&self, at: I64Vec3) -> BlockType {
        self.world.get_block_at(at)
    }

    fn generated(&self, at: I64Vec3) -> bool {
        self.world.is_chunk_generated(ChunkCoordinate::containing(at))
    }

    fn triangles(&self, min: Vec3, max: Vec3) -> Vec<[Vec3; 3]> {
        self.grounds
            .iter()
            .flat_map(|&(at, floor)| floor.near(min - at, max - at).into_iter().map(move |t| t.map(|p| p + at)))
            .collect()
    }

    fn ground(&self, p: Vec3, below: f32, above: f32, up: f32) -> Option<f32> {
        self.grounds
            .iter()
            .filter_map(|&(at, floor)| floor.surface_facing(p - at, below, above, up).map(|(y, _)| y + at.y))
            .max_by(f32::total_cmp)
    }
}

// Blocks a second, and how much faster while Shift is held.
const MOVE_SPEED: f32 = 20.0;
const SPRINT_MULTIPLIER: f32 = 8.0;
// The player's size: two blocks tall, so it just fits through a two-block gap.
pub(crate) const BODY: Aabb = Aabb {
    min: Vec3::new(-0.3, 0.0, -0.3),
    max: Vec3::new(0.3, 2.0, 0.3),
};
// Walking, blocks a second, and how much faster while Shift is held.
const WALK_SPEED: f32 = 5.0;
const WALK_SPRINT_MULTIPLIER: f32 = 1.5;
// Blocks a second squared, and the speed falling tops out at.
const GRAVITY: f32 = 32.0;
const TERMINAL_SPEED: f32 = 60.0;
// Enough to clear one block, not two.
pub(crate) const JUMP_SPEED: f32 = 9.0;
// In water: slower across, sinking slowly, Space swimming up.
const SWIM_MULTIPLIER: f32 = 0.6;
const WATER_GRAVITY: f32 = 8.0;
const SINK_SPEED: f32 = 2.0;
const SWIM_UP_SPEED: f32 = 4.0;
// How fast, a share a second, the player's speed across turns toward what's wanted in the air.
const AIR_CONTROL: f32 = 3.0;
// How far up the body the water must reach for the player to be swimming.
const SWIM_CHEST: f32 = 0.6;
// Blocks are drawn centred on their coordinates, and collision takes a block to start at its coordinate.
pub(crate) const BLOCK_CORNER: Vec3 = Vec3::splat(0.5);
// The tallest ledge a grounded player walks up without jumping.
pub(crate) const STEP_HEIGHT: f32 = 1.0;
// How far down sculpted ground a standing player follows it rather than falling.
const SETTLE_SNAP: f32 = 0.6;
// The body against sculpted ground, rounded over its bumps, see `contact`.
const SPHERES: Body = Body { radius: 0.3, heights: [0.3, 1.0, 1.7] };
// The tallest rise the body steps up onto sculpted ground without jumping, in blocks; a jump clears a block.
const SCULPT_STEP: f32 = 0.5;
// How far up a face must face to stand on, ~45 degrees from level; steeper slides. Grass is half dirt here.
const GROUND_FACING: f32 = 0.7;
// A rise onto sculpted ground taller than this in a step counts as a step up.
const SCULPT_RISE_EASE: f32 = 0.25;
// Seconds between steps up.
const STEP_COOLDOWN: f32 = 0.1;
// Gravity's multiple falling, and rising after Space is let go, so a jump arcs sharply and a tap hops.
const FALL_GRAVITY: f32 = 1.4;
const SHORT_HOP_GRAVITY: f32 = 2.5;
// Landing: the slowest fall that counts.
const LAND_MIN_SPEED: f32 = 7.0;

pub fn step_character(
    state: &mut CharacterState,
    position: &mut Vec3,
    input: &MovementInput,
    env: &impl MovementEnvironment,
    dt: f32,
) -> StepEvents {
    if input.toggle_fly {
        state.movement = match state.movement {
            Movement::Walk => Movement::Fly,
            Movement::Fly => Movement::Walk,
        };
        state.velocity = Vec3::ZERO;
        info!("{:?}ing", state.movement);
    }
    let across = Vec3::new(input.across.x, 0.0, input.across.y);
    match state.movement {
        Movement::Fly => {
            let speed = MOVE_SPEED * dt * if input.sprint { SPRINT_MULTIPLIER } else { 1.0 };
            let facing = about_y(input.yaw) * about_x(input.pitch);
            *position += (facing * across + Vec3::Y * input.lift) * speed;
            StepEvents::default()
        }
        Movement::Walk => walk(env, state, position, input, across, dt),
    }
}

fn walk(
    env: &impl MovementEnvironment,
    state: &mut CharacterState,
    position: &mut Vec3,
    input: &MovementInput,
    across: Vec3,
    dt: f32,
) -> StepEvents {
    let mut events = StepEvents::default();
    // swimming until the chest is out, so the head bobs well clear of the water
    let chest = *position + Vec3::Y * SWIM_CHEST;
    let swimming = env.block(chest.round().as_i64vec3()) == BlockType::Water;
    // just over the feet, so wading through a shallow river slows the player as swimming does
    let feet = *position + Vec3::Y * 0.1;
    let wading = env.block(feet.round().as_i64vec3()) == BlockType::Water;
    let jumping = input.jump;

    let mut speed = WALK_SPEED;
    if input.sprint {
        speed *= WALK_SPRINT_MULTIPLIER;
    }
    if swimming || wading {
        speed *= SWIM_MULTIPLIER;
    }
    // where the body faces, not the camera, so looking down doesn't slow it
    let wanted = about_y(input.yaw) * across.normalize_or_zero() * speed;
    // in the air the player keeps its way and only steers toward where it's heading, as the air holds it back
    let grip = if state.grounded || swimming || wading {
        1.0
    } else {
        (AIR_CONTROL * dt).min(1.0)
    };
    state.velocity.x += (wanted.x - state.velocity.x) * grip;
    state.velocity.z += (wanted.z - state.velocity.z) * grip;

    if swimming {
        state.velocity.y = if jumping && state.against_wall {
            JUMP_SPEED
        } else if jumping {
            SWIM_UP_SPEED
        } else {
            (state.velocity.y - WATER_GRAVITY * dt).max(-SINK_SPEED)
        };
    } else {
        if jumping && state.grounded {
            state.velocity.y = JUMP_SPEED;
            events.jumped = Some(*position);
        }
        // falling with the feet still in water is gentle, so the head stays up a while, but a jump out of shallows isn't
        let gravity = if wading && state.velocity.y <= 0.0 {
            WATER_GRAVITY
        } else if state.velocity.y > 0.0 && !jumping {
            GRAVITY * SHORT_HOP_GRAVITY
        } else if state.velocity.y < 0.0 {
            GRAVITY * FALL_GRAVITY
        } else {
            GRAVITY
        };
        state.velocity.y = (state.velocity.y - gravity * dt).max(-TERMINAL_SPEED);
    }

    state.step_cooldown = (state.step_cooldown - dt).max(0.0);
    let ready = state.grounded && !swimming && state.step_cooldown == 0.0;
    let step = if ready { STEP_HEIGHT } else { 0.0 };
    let start = *position;
    let delta = state.velocity * dt;
    let reach = Vec3::new(1.5, 1.5, 1.5);
    let ground_near = env.triangles(start - reach + delta.min(Vec3::ZERO), start + reach + Vec3::Y * 2.0 + delta.max(Vec3::ZERO));
    let generated = |block: I64Vec3| env.generated(block);
    let mut walled = false;
    let (moved_to, hit) = if ground_near.is_empty() {
        // Chunks not generated yet are solid, so the player waits for the ground to load rather than falling through.
        let solid = |block: I64Vec3| !generated(block) || env.block(block).is_solid();
        move_and_step(start + BLOCK_CORNER, delta, BODY, step, solid)
    } else {
        // sculpted ground collides as its surface, walls and ground apart, see `Walker`; trees and unloaded chunks as blocks
        let solid = |block: I64Vec3| !generated(block) || matches!(env.block(block), BlockType::Leaves | BlockType::Log);
        let across_blocks = |feet: Vec3, delta: Vec3| move_and_collide(feet + BLOCK_CORNER, delta, BODY, solid).0 - BLOCK_CORNER;
        let ground = |p: Vec3, below: f32, above: f32| env.ground(p, below, above, GROUND_FACING);
        let support = |p: Vec3, below: f32, above: f32| env.ground(p, below, above, 1e-3);
        let walker = Walker {
            body: &SPHERES,
            triangles: &ground_near,
            walkable: GROUND_FACING,
            step: SCULPT_STEP,
            snap: SETTLE_SNAP,
            ground: &ground,
            support: &support,
        };
        let standing = !swimming && state.velocity.y <= 0.0 && state.grounded;
        // over a chunk not generated yet the player waits rather than falling into it
        let under = (start + BLOCK_CORNER - Vec3::Y).floor().as_i64vec3();
        let delta = if generated(under) { delta } else { delta.with_y(delta.y.max(0.0)) };
        let moved = walker.walk(start, delta, standing, &across_blocks);
        let hit = BVec3::new(false, moved.grounded || moved.bumped, false);
        // a wall takes away only the way in to it, so the body slides along it whichever way it faces
        let wall = moved.wall.normalize_or_zero();
        let into_wall = state.velocity.dot(wall);
        if into_wall < 0.0 {
            state.velocity -= wall * into_wall;
        }
        walled = wall != Vec3::ZERO;
        // on a face too steep to stand on nothing into it is kept, so gravity carries the body down it
        let into_slope = state.velocity.dot(moved.slope);
        if into_slope < 0.0 {
            state.velocity -= moved.slope * into_slope;
        }
        (moved.feet + BLOCK_CORNER, hit)
    };
    // a step up moves the body a block in one step
    let rose = moved_to.y - BLOCK_CORNER.y - position.y;
    // on sculpted ground only a rise sharper than walking up its slopes counts
    let least = if ground_near.is_empty() { 0.0 } else { SCULPT_RISE_EASE };
    if step > 0.0 && rose > least {
        events.rose = rose;
        state.step_cooldown = STEP_COOLDOWN;
    }
    *position = moved_to - BLOCK_CORNER;
    let fall = -state.velocity.y;
    let was_grounded = state.grounded;
    state.grounded = hit.y && state.velocity.y < 0.0;
    if state.grounded && !was_grounded && !swimming && !wading && fall >= LAND_MIN_SPEED {
        events.landed = fall;
    }
    if hit.y {
        state.velocity.y = 0.0;
    }
    if hit.x {
        state.velocity.x = 0.0;
    }
    if hit.z {
        state.velocity.z = 0.0;
    }
    state.against_wall = (hit.x || hit.z || walled) && across != Vec3::ZERO;
    events
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::{math::U16Vec3, utils::default};
    use ochre_terrain::chunk::{ChunkData, CHUNK_SIZE};

    const DT: f32 = 1.0 / 60.0;

    // One chunk at the origin, rock below y = 8 and air above. Beyond it nothing is generated, so it's solid.
    fn flat_world() -> World {
        let mut world = World::with_terrain(1, 16, ochre_world::worldmap::test_terrain(1));
        let mut data = ChunkData::default();
        for (x, y, z) in (0..CHUNK_SIZE).flat_map(|x| (0..8).flat_map(move |y| (0..CHUNK_SIZE).map(move |z| (x, y, z)))) {
            data.set_block_at(U16Vec3::new(x, y, z), BlockType::Rock);
        }
        world.insert_chunk(ChunkCoordinate(I64Vec3::ZERO), data);
        world
    }

    fn step(world: &World, state: &mut CharacterState, at: &mut Vec3, input: &MovementInput) -> StepEvents {
        step_character(state, at, input, &WorldEnvironment::new(world, []), DT)
    }

    fn settle(world: &World, state: &mut CharacterState, at: &mut Vec3) {
        for _ in 0..120 {
            step(world, state, at, &MovementInput::default());
        }
    }

    #[test]
    fn falls_onto_the_ground_and_lands_once() {
        let world = flat_world();
        let (mut state, mut at) = (CharacterState::new(Movement::Walk), Vec3::new(8.0, 12.0, 8.0));
        let landings: Vec<f32> = (0..120).map(|_| step(&world, &mut state, &mut at, &MovementInput::default()).landed).filter(|l| *l > 0.0).collect();
        assert_eq!(landings.len(), 1);
        assert!(state.grounded && state.velocity.y == 0.0);
        assert!((at.y - 7.5).abs() < 0.1, "stands at {}", at.y);
    }

    #[test]
    fn walks_forward_at_walking_pace_and_a_jump_leaves_the_ground() {
        let world = flat_world();
        let (mut state, mut at) = (CharacterState::new(Movement::Walk), Vec3::new(8.0, 8.0, 12.0));
        settle(&world, &mut state, &mut at);
        let rest = at;
        let forward = MovementInput { across: Vec2::new(0.0, -1.0), ..default() };
        for _ in 0..30 {
            step(&world, &mut state, &mut at, &forward);
        }
        assert!((state.velocity.z + WALK_SPEED).abs() < 1e-4);
        assert!((rest.z - at.z - WALK_SPEED * 0.5).abs() < 0.1, "walked {}", rest.z - at.z);
        let jump = MovementInput { jump: true, ..default() };
        let events = step(&world, &mut state, &mut at, &jump);
        assert!(events.jumped.is_some() && state.velocity.y > 0.0);
    }

    #[test]
    fn the_same_inputs_give_the_same_bits() {
        let world = flat_world();
        let run = || {
            let (mut state, mut at) = (CharacterState::new(Movement::Walk), Vec3::new(8.0, 12.0, 8.0));
            for tick in 0..600 {
                let input = MovementInput {
                    across: Vec2::new(((tick / 7) % 3) as f32 - 1.0, ((tick / 11) % 3) as f32 - 1.0),
                    sprint: tick % 90 < 40,
                    jump: tick % 50 < 5,
                    yaw: tick as f32 * 0.01,
                    ..default()
                };
                step(&world, &mut state, &mut at, &input);
            }
            (state, at.to_array().map(f32::to_bits))
        };
        assert_eq!(run(), run());
    }

    #[test]
    fn toggling_flight_flips_the_mode_and_stops_the_body() {
        let world = flat_world();
        let (mut state, mut at) = (CharacterState::new(Movement::Walk), Vec3::new(8.0, 12.0, 8.0));
        state.velocity = Vec3::new(1.0, -3.0, 2.0);
        step(&world, &mut state, &mut at, &MovementInput { toggle_fly: true, ..default() });
        assert_eq!((state.movement, state.velocity), (Movement::Fly, Vec3::ZERO));
        let up = MovementInput { lift: 1.0, ..default() };
        let start = at;
        step(&world, &mut state, &mut at, &up);
        assert!((at.y - start.y - MOVE_SPEED * DT).abs() < 1e-5);
    }

    // A flat quad over x, z in 0..16 at `y`.
    fn quad(y: f32) -> Floor {
        let (a, b, c, d) = (Vec3::new(0.0, y, 0.0), Vec3::new(16.0, y, 0.0), Vec3::new(16.0, y, 16.0), Vec3::new(0.0, y, 16.0));
        Floor::new(vec![[a, b, c], [a, c, d]])
    }

    // Level ground at y = 2 for x < 4, a ramp rising `grade` blocks a block to x = 10, and level ground on top past it.
    fn ramp(grade: f32) -> Floor {
        let top = 2.0 + 6.0 * grade;
        let quad = |x0: f32, y0: f32, x1: f32, y1: f32| {
            let (a, b, c, d) = (Vec3::new(x0, y0, 0.0), Vec3::new(x1, y1, 0.0), Vec3::new(x1, y1, 16.0), Vec3::new(x0, y0, 16.0));
            [[a, b, c], [a, c, d]]
        };
        Floor::new([quad(0.0, 2.0, 4.0, 2.0), quad(4.0, 2.0, 10.0, top), quad(10.0, top, 16.0, top)].concat())
    }

    // `seconds` of `input` on `floor` from `at`, and the highest the feet got.
    fn run_on(floor: &Floor, at: Vec3, input: &MovementInput, seconds: f32) -> (CharacterState, Vec3, f32) {
        let world = flat_world();
        let env = WorldEnvironment::new(&world, [(LodKey { level: 1, coord: I64Vec3::ZERO }, Vec3::ZERO, floor)]);
        let (mut state, mut at, mut highest) = (CharacterState::new(Movement::Walk), at, at.y);
        for _ in 0..(seconds / DT) as usize {
            step_character(&mut state, &mut at, input, &env, DT);
            highest = highest.max(at.y);
        }
        (state, at, highest)
    }

    #[test]
    fn walks_up_a_gentle_slope() {
        let right = MovementInput { across: Vec2::new(1.0, 0.0), ..default() };
        let (state, at, _) = run_on(&ramp(0.4), Vec3::new(2.0, 2.0, 8.0), &right, 3.0);
        assert!(at.x > 11.0 && (at.y - 4.4).abs() < 0.1 && state.grounded, "at {at}");
    }

    #[test]
    fn cannot_walk_or_jump_up_a_steep_slope() {
        let climb = MovementInput { across: Vec2::new(1.0, 0.0), sprint: true, jump: true, ..default() };
        let (_, at, highest) = run_on(&ramp(1.4), Vec3::new(2.0, 2.0, 8.0), &climb, 4.0);
        assert!(highest < 2.0 + 1.6, "climbed to {highest}, at {at}");
    }

    #[test]
    fn slides_down_a_steep_slope_onto_level_ground() {
        let (state, at, _) = run_on(&ramp(1.4), Vec3::new(8.0, 2.0 + 4.0 * 1.4, 8.0), &MovementInput::default(), 3.0);
        assert!(at.x < 4.6 && (at.y - 2.0).abs() < 0.05 && state.grounded, "at {at}");
    }

    #[test]
    fn groups_found_in_any_order_give_the_same_triangles_and_ground() {
        let world = flat_world();
        let (low, high) = (quad(8.0), quad(8.5));
        let group = |x: i64| LodKey { level: 1, coord: I64Vec3::new(x, 0, 0) };
        let grounds = [(group(0), Vec3::ZERO, &low), (group(1), Vec3::new(0.0, 0.0, 0.0), &high), (group(-1), Vec3::new(-16.0, 0.0, 0.0), &low)];
        let forward = WorldEnvironment::new(&world, grounds);
        let reversed = WorldEnvironment::new(&world, grounds.into_iter().rev());
        let (min, max) = (Vec3::new(-20.0, 7.0, -1.0), Vec3::new(20.0, 9.0, 17.0));
        let bits = |t: Vec<[Vec3; 3]>| t.into_iter().map(|t| t.map(|p| p.to_array().map(f32::to_bits))).collect::<Vec<_>>();
        let triangles = bits(forward.triangles(min, max));
        assert!(!triangles.is_empty());
        assert_eq!(triangles, bits(reversed.triangles(min, max)));
        let at = Vec3::new(4.0, 8.0, 4.0);
        assert_eq!(forward.ground(at, 1.0, 1.0, 0.0), Some(8.5));
        assert_eq!(forward.ground(at, 1.0, 1.0, 0.0), reversed.ground(at, 1.0, 1.0, 0.0));
    }

    #[test]
    fn the_libm_quaternions_match_glams_to_rounding() {
        use bevy::math::EulerRot;
        for tenths in -400..400 {
            let (yaw, pitch) = (tenths as f32 * 0.1, tenths as f32 * 0.037);
            assert!(about_y(yaw).abs_diff_eq(Quat::from_rotation_y(yaw), 1e-5), "yaw {yaw}");
            assert!((about_y(yaw) * about_x(pitch)).abs_diff_eq(Quat::from_euler(EulerRot::YXZ, yaw, pitch, 0.0), 1e-5), "yaw {yaw} pitch {pitch}");
        }
    }

    #[test]
    fn around_takes_the_groups_touching_the_home_group_at_its_level() {
        let world = flat_world();
        let floor = quad(8.0);
        let at = |x: f32| Transform::from_xyz(x, 0.0, 0.0);
        let group = |level: u8, x: i64| ChunkGroup { key: LodKey { level, coord: I64Vec3::new(x, 0, 0) } };
        // a body in group 0 of level 1, which spans 32 blocks
        let groups = [(group(1, 0), at(0.0)), (group(1, 1), at(32.0)), (group(1, 2), at(64.0)), (group(2, 0), at(0.0)), (group(1, -2), at(-64.0))];
        let env = WorldEnvironment::around(&world, Vec3::new(4.0, 8.0, 4.0), groups.iter().map(|(g, t)| (g, t, &floor)));
        let found = env.triangles(Vec3::new(-200.0, 7.0, -1.0), Vec3::new(300.0, 9.0, 17.0));
        assert_eq!(found.len(), 4, "two quads of two triangles: the home group and its neighbour");
    }
}
