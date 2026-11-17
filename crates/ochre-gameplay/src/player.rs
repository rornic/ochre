
use std::f32::consts::{FRAC_PI_2, PI, TAU};

use bevy::{
    camera::Camera,
    ecs::hierarchy::ChildOf,
    ecs::{
        component::Component,
        message::{MessageReader, MessageWriter},
        query::{With, Without},
        resource::Resource,
        system::{Query, Res},
    },
    input::{
        keyboard::KeyCode,
        mouse::{MouseButton, MouseMotion},
        ButtonInput,
    },
    log::info,
    math::{BVec3, Dir3, EulerRot, I64Vec3, Quat, Vec3, Vec3Swizzles},
    prelude::Transform,
    time::Time,
    window::{CursorGrabMode, CursorOptions, PrimaryWindow, Window},
};

use crate::collision::{move_and_collide, move_and_step, Aabb};
use crate::contact::{Body, Walker};
use crate::footsteps::{Footstep, Surface, SPRINT_LOUDER};
use crate::view::{
    apply_cold_grade, lift_cold_grade, spawn_camera, update_underwater_effects, ColdGrade,
    MurkMaterial, ViewAssets,
};
use bevy::{
    app::{App, Plugin, Startup, Update},
    camera::Projection,
    ecs::{
        schedule::IntoScheduleConfigs,
        system::Commands,
    },
    pbr::MaterialPlugin,
};
use ochre_base::block::BlockType;
use ochre_terrain::{
    chunk::ChunkCoordinate,
    floor::Floor,
    group::ChunkGroup,
    lod::LodKey,
    horizon::update_horizon,
    settings::RendererSettings,
    world::World,
};
use ochre_world::ambience::SurroundingsPlugin;

/// The player's body, at its feet, with the camera at its eyes.
#[derive(Component, Default)]
pub(crate) struct Player {
    pub movement: Movement,
    velocity: Vec3,
    grounded: bool,
    // whether it walked into a wall last frame, so swimming against a bank climbs out
    against_wall: bool,
    // how far through its stride the head's bob is, in radians a pair of steps, and how big the bob is now
    bob_phase: f32,
    bob: f32,
    // how far the eyes still trail below the body after a step up
    eye_lag: f32,
    // seconds before the next step up is allowed, so a flight of steps isn't taken at a sprint
    step_cooldown: f32,
    // how fast it was falling on the frame it landed, else zero
    landed: f32,
    // what it pushed off on the frame it jumped, else none
    jumped: Option<Surface>,
    // how much of a jump's kick is left, from 1 at take-off, and the pitch it has added to the camera
    kick: f32,
    // the same for a landing, as a multiple of a walking step's thud
    thud: f32,
    nudge: f32,
    // how fast the field of view is opening, in radians a second
    fov_speed: f32,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Movement {
    /// Flying anywhere, through anything, the way the camera is facing.
    #[default]
    Fly,
    /// Walking under gravity, kept out of solid blocks.
    Walk,
}

const FLY_TOGGLE_KEY: KeyCode = KeyCode::KeyF;

// Blocks a second, and how much faster while Shift is held.
const MOVE_SPEED: f32 = 20.0;
const SPRINT_MULTIPLIER: f32 = 8.0;
const LOOK_SENSITIVITY: f32 = 0.1;
// Just short of straight up or down, where the view would flip over.
const MAX_PITCH: f32 = 89.0_f32.to_radians();

/// How high the camera sits above the player's feet, far enough under the top of its head
/// that the camera's near plane stays out of the ceiling.
pub const EYE_HEIGHT: f32 = 1.9;
// How close the camera's near plane is. A 90 degree view's near plane reaches `NEAR * (sin(pitch) + cos(pitch))` above the eye, at most
// `NEAR * sqrt 2`, which has to stay under the 0.1 between the eye and the top of the head, or a two-block gap shows the inside of its ceiling.
pub(crate) const NEAR: f32 = 0.05;
// The player's size: two blocks tall, so it just fits through a two-block gap.
const BODY: Aabb = Aabb {
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
const JUMP_SPEED: f32 = 9.0;
// In water: slower across, sinking slowly, Space swimming up.
const SWIM_MULTIPLIER: f32 = 0.6;
const WATER_GRAVITY: f32 = 8.0;
const SINK_SPEED: f32 = 2.0;
const SWIM_UP_SPEED: f32 = 4.0;
// Head bob walking and sprinting in blocks, blocks walked a pair of steps, and how fast it eases in and out a second.
const BOB_WALK: f32 = 0.05;
const BOB_SPRINT: f32 = 0.065;
const BOB_STRIDE: f32 = 3.6;
// Sprinting takes longer strides, so its bob is only a little quicker than walking's.
const BOB_SPRINT_STRIDE: f32 = 4.6;
const BOB_EASE: f32 = 6.0;
// Vertical field of view in degrees, how much wider it opens sprinting, and the spring it follows with: stiffness, and twice its square root in damping, which settles without overshoot.
pub(crate) const FOV: f32 = 90.0;
const SPRINT_FOV: f32 = 8.0;
const FOV_STIFFNESS: f32 = 36.0;
const FOV_DAMPING: f32 = 12.0;
// How fast, a share a second, the player's speed across turns toward what's wanted in the air.
const AIR_CONTROL: f32 = 3.0;
// How far up the body the water must reach for the player to be swimming.
const SWIM_CHEST: f32 = 0.6;
// Blocks are drawn centred on their coordinates, and collision takes a block to start at its coordinate.
const BLOCK_CORNER: Vec3 = Vec3::splat(0.5);
// The tallest ledge a grounded player walks up without jumping.
const STEP_HEIGHT: f32 = 1.0;
// How far down sculpted ground a standing player follows it rather than falling.
const SETTLE_SNAP: f32 = 0.6;
// The body against sculpted ground, rounded over its bumps, see `contact`.
const SPHERES: Body = Body { radius: 0.3, heights: [0.3, 1.0, 1.7] };
// The tallest rise the body steps up onto sculpted ground without jumping, in blocks.
const SCULPT_STEP: f32 = 1.0;
// How far up a sculpted face must face to stand on, about 66 degrees from level; steeper is a wall.
const GROUND_FACING: f32 = 0.4;
// A rise onto sculpted ground taller than this in a frame eases the eyes up, as a step does.
const SCULPT_RISE_EASE: f32 = 0.25;
// Seconds between steps up.
const STEP_COOLDOWN: f32 = 0.1;
// How fast, a share a second, the eyes catch up with the body after a step up.
const STEP_EASE: f32 = 10.0;
// The longest frame moved in one step, so a hitch doesn't fling the player.
const MAX_STEP: f32 = 0.1;
// Gravity's multiple falling, and rising after Space is let go, so a jump arcs sharply and a tap hops.
const FALL_GRAVITY: f32 = 1.4;
const SHORT_HOP_GRAVITY: f32 = 2.5;
// A jump's push-off, as a multiple of a step's loudness, the degrees it widens the view and tips it up, and how fast, a share a second, that fades.
const TAKEOFF_LOUDNESS: f32 = 0.6;
const JUMP_FOV: f32 = 1.0;
const JUMP_PITCH: f32 = 0.5;
const KICK_DECAY: f32 = 5.0;
// How many degrees a landing narrows the view and tips it down, at a walking step's thud.
const LAND_FOV: f32 = 1.0;
const LAND_PITCH: f32 = 0.75;
// Landing: the slowest fall that counts, how far the eyes drop for each block a second it was falling and at most, and how loud the thud is, as a multiple of a step, at the softest and hardest.
const LAND_MIN_SPEED: f32 = 7.0;
const LAND_DIP: f32 = 0.012;
const LAND_DIP_MAX: f32 = 0.3;
const LAND_LOUDNESS: (f32, f32) = (0.8, 2.0);

type PlayerCamera<'a> = (&'a ChildOf, &'a mut Transform);

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn player_move(
    time: Res<Time>,
    world: Res<World>,
    mut player_query: Query<(&mut Transform, &mut Player)>,
    mut camera_query: Query<(&ChildOf, &mut Transform), (With<Camera>, Without<Player>)>,
    keys: Res<ButtonInput<KeyCode>>,
    mut steps: MessageWriter<Footstep>,
    mut projection: Query<&mut Projection, With<Camera>>,
    floors: Query<(&ChunkGroup, &Transform, &Floor), (Without<Player>, Without<Camera>)>,
) {
    let (parent, mut camera) = camera_query.single_mut().expect("camera does not exist");
    let (mut transform, mut player) = player_query
        .get_mut(parent.parent())
        .expect("player does not exist");
    if keys.just_pressed(FLY_TOGGLE_KEY) {
        player.movement = match player.movement {
            Movement::Walk => Movement::Fly,
            Movement::Fly => Movement::Walk,
        };
        player.velocity = Vec3::ZERO;
        info!("{:?}ing", player.movement);
    }
    let dt = time.delta_secs().min(MAX_STEP);
    let axis = |negative, positive| {
        if keys.pressed(negative) {
            -1.0
        } else if keys.pressed(positive) {
            1.0
        } else {
            0.0
        }
    };
    let across = Vec3::new(
        axis(KeyCode::KeyA, KeyCode::KeyD),
        0.0,
        axis(KeyCode::KeyW, KeyCode::KeyS),
    );
    let sprinting = keys.pressed(KeyCode::ShiftLeft);

    // the sculpted ground of the groups round the player, in world units: the highest surface from `below` to `above` a point
    let home = LodKey::containing(ChunkCoordinate::containing((transform.translation + BLOCK_CORNER).floor().as_i64vec3()), 1);
    let near: Vec<(Vec3, &Floor)> = floors
        .iter()
        .filter(|(group, _, _)| group.key.level == home.level && (group.key.coord - home.coord).abs().max_element() <= 1)
        .map(|(_, at, floor)| (at.translation, floor))
        .collect();
    // and its triangles between two corners, in world units
    let sculpted = |min: Vec3, max: Vec3| {
        near.iter()
            .flat_map(|&(at, floor)| floor.near(min - at, max - at).into_iter().map(move |t| t.map(|p| p + at)))
            .collect::<Vec<_>>()
    };
    let floor_at = |p: Vec3, below: f32, above: f32, up: f32| {
        near.iter()
            .filter_map(|&(at, floor)| floor.surface_facing(p - at, below, above, up).map(|(y, _)| y + at.y))
            .max_by(f32::total_cmp)
    };

    match player.movement {
        Movement::Fly => {
            let speed = MOVE_SPEED * dt * if sprinting { SPRINT_MULTIPLIER } else { 1.0 };
            let up = -axis(KeyCode::Space, KeyCode::ControlLeft);
            let step = transform.rotation * camera.rotation * across + Vec3::Y * up;
            transform.translation += step * speed;
        }
        Movement::Walk => walk(
            &world,
            &mut transform,
            &mut player,
            across,
            sprinting,
            &keys,
            dt,
            &floor_at,
            &sculpted,
        ),
    }

    // the head bobs a little walking and more sprinting, as far through a stride as the player has walked
    let across_speed = player.velocity.xz().length();
    let striding = player.movement == Movement::Walk && player.grounded && across_speed > 0.1;
    let target = match (striding, sprinting) {
        (false, _) => 0.0,
        (true, false) => BOB_WALK,
        (true, true) => BOB_SPRINT,
    };
    player.bob += (target - player.bob) * (BOB_EASE * dt).min(1.0);
    let stride = if sprinting {
        BOB_SPRINT_STRIDE
    } else {
        BOB_STRIDE
    };
    let before = player.bob_phase;
    player.bob_phase = (player.bob_phase + across_speed * dt * TAU / stride) % TAU;
    if striding && footfall(before, player.bob_phase) {
        if let Some(surface) = Surface::under(&world, transform.translation) {
            let loudness = if sprinting { SPRINT_LOUDER } else { 1.0 };
            steps.write(Footstep { surface, loudness });
        }
    }
    let rushing = sprinting
        && across != Vec3::ZERO
        && (player.movement == Movement::Fly || across_speed > 0.1);
    if let Ok(mut projection) = projection.single_mut() {
        if let Projection::Perspective(perspective) = &mut *projection {
            let wanted = (FOV + if rushing { SPRINT_FOV } else { 0.0 } + JUMP_FOV * player.kick
                - LAND_FOV * player.thud)
                .to_radians();
            let pull = (wanted - perspective.fov) * FOV_STIFFNESS - player.fov_speed * FOV_DAMPING;
            player.fov_speed += pull * dt;
            perspective.fov += player.fov_speed * dt;
        }
    }
    if let Some(surface) = player.jumped.take() {
        steps.write(Footstep {
            surface,
            loudness: TAKEOFF_LOUDNESS,
        });
        player.kick = 1.0;
    }
    player.kick -= player.kick * (KICK_DECAY * dt).min(1.0);
    player.thud -= player.thud * (KICK_DECAY * dt).min(1.0);
    let landed = std::mem::take(&mut player.landed);
    if landed > 0.0 {
        let hardness = (landed / JUMP_SPEED).clamp(LAND_LOUDNESS.0, LAND_LOUDNESS.1);
        player.thud = hardness;
        player.eye_lag = (player.eye_lag + landed * LAND_DIP).min(player.eye_lag.max(LAND_DIP_MAX));
        if let Some(surface) = Surface::under(&world, transform.translation) {
            steps.write(Footstep {
                surface,
                loudness: hardness,
            });
        }
    }
    player.eye_lag -= player.eye_lag * (STEP_EASE * dt).min(1.0);
    camera.translation = Vec3::new(
        player.bob * 0.5 * player.bob_phase.sin(),
        EYE_HEIGHT - player.eye_lag - player.bob * (1.0 - (2.0 * player.bob_phase).cos()) / 2.0,
        0.0,
    );
}

// Whether the bob's phase passed where the head is lowest, a quarter and three quarters of the way round.
fn footfall(before: f32, after: f32) -> bool {
    let side = |phase: f32| ((phase + FRAC_PI_2) / PI) as u32 % 2;
    side(before) != side(after)
}

#[allow(clippy::too_many_arguments)]
fn walk(
    world: &World,
    transform: &mut Transform,
    player: &mut Player,
    across: Vec3,
    sprinting: bool,
    keys: &ButtonInput<KeyCode>,
    dt: f32,
    floor_at: &dyn Fn(Vec3, f32, f32, f32) -> Option<f32>,
    sculpted: &dyn Fn(Vec3, Vec3) -> Vec<[Vec3; 3]>,
) {
    // swimming until the chest is out, so the head bobs well clear of the water
    let chest = transform.translation + Vec3::Y * SWIM_CHEST;
    let swimming = world.get_block_at(chest.round().as_i64vec3()) == BlockType::Water;
    // just over the feet, so wading through a shallow river slows the player as swimming does
    let feet = transform.translation + Vec3::Y * 0.1;
    let wading = world.get_block_at(feet.round().as_i64vec3()) == BlockType::Water;
    let jumping = keys.pressed(KeyCode::Space);

    let mut speed = WALK_SPEED;
    if sprinting {
        speed *= WALK_SPRINT_MULTIPLIER;
    }
    if swimming || wading {
        speed *= SWIM_MULTIPLIER;
    }
    // where the body faces, not the camera, so looking down doesn't slow it
    let wanted = transform.rotation * across.normalize_or_zero() * speed;
    // in the air the player keeps its way and only steers toward where it's heading, as the air holds it back
    let grip = if player.grounded || swimming || wading {
        1.0
    } else {
        (AIR_CONTROL * dt).min(1.0)
    };
    player.velocity.x += (wanted.x - player.velocity.x) * grip;
    player.velocity.z += (wanted.z - player.velocity.z) * grip;

    if swimming {
        player.velocity.y = if jumping && player.against_wall {
            JUMP_SPEED
        } else if jumping {
            SWIM_UP_SPEED
        } else {
            (player.velocity.y - WATER_GRAVITY * dt).max(-SINK_SPEED)
        };
    } else {
        if jumping && player.grounded {
            player.velocity.y = JUMP_SPEED;
            player.jumped = Surface::under(world, transform.translation);
        }
        // falling with the feet still in water is gentle, so the head stays up a while, but a jump out of shallows isn't
        let gravity = if wading && player.velocity.y <= 0.0 {
            WATER_GRAVITY
        } else if player.velocity.y > 0.0 && !jumping {
            GRAVITY * SHORT_HOP_GRAVITY
        } else if player.velocity.y < 0.0 {
            GRAVITY * FALL_GRAVITY
        } else {
            GRAVITY
        };
        player.velocity.y = (player.velocity.y - gravity * dt).max(-TERMINAL_SPEED);
    }

    // Chunks not generated yet are solid, so the player waits where it is for the ground to
    // load rather than falling through it.
    player.step_cooldown = (player.step_cooldown - dt).max(0.0);
    let ready = player.grounded && !swimming && player.step_cooldown == 0.0;
    let step = if ready { STEP_HEIGHT } else { 0.0 };
    let start = transform.translation;
    let delta = player.velocity * dt;
    let reach = Vec3::new(1.5, 1.5, 1.5);
    let ground_near = sculpted(start - reach + delta.min(Vec3::ZERO), start + reach + Vec3::Y * 2.0 + delta.max(Vec3::ZERO));
    let generated = |block: I64Vec3| world.is_chunk_generated(ChunkCoordinate::containing(block));
    let mut walled = false;
    let (position, hit) = if ground_near.is_empty() {
        // Chunks not generated yet are solid, so the player waits where it is for the ground to load rather than falling through it.
        let solid = |block: I64Vec3| !generated(block) || world.get_block_at(block).is_solid();
        move_and_step(start + BLOCK_CORNER, delta, BODY, step, solid)
    } else {
        // sculpted ground collides as its surface, walls and ground apart, see `Walker`; trees and unloaded chunks as blocks
        let solid = |block: I64Vec3| !generated(block) || matches!(world.get_block_at(block), BlockType::Leaves | BlockType::Log);
        let across_blocks = |feet: Vec3, delta: Vec3| move_and_collide(feet + BLOCK_CORNER, delta, BODY, solid).0 - BLOCK_CORNER;
        let ground = |p: Vec3, below: f32, above: f32| floor_at(p, below, above, GROUND_FACING);
        let support = |p: Vec3, below: f32, above: f32| floor_at(p, below, above, 1e-3);
        let walker = Walker {
            body: &SPHERES,
            triangles: &ground_near,
            walkable: GROUND_FACING,
            step: SCULPT_STEP,
            snap: SETTLE_SNAP,
            ground: &ground,
            support: &support,
        };
        let standing = !swimming && player.velocity.y <= 0.0 && player.grounded;
        // over a chunk not generated yet the player waits rather than falling into it
        let under = (start + BLOCK_CORNER - Vec3::Y).floor().as_i64vec3();
        let delta = if generated(under) { delta } else { delta.with_y(delta.y.max(0.0)) };
        let moved = walker.walk(start, delta, standing, &across_blocks);
        let hit = BVec3::new(false, moved.grounded || moved.bumped, false);
        // a wall takes away only the way in to it, so the body slides along it whichever way it faces
        let wall = moved.wall.normalize_or_zero();
        let into_wall = player.velocity.dot(wall);
        if into_wall < 0.0 {
            player.velocity -= wall * into_wall;
        }
        walled = wall != Vec3::ZERO;
        let feet = moved.feet;
        (feet + BLOCK_CORNER, hit)
    };
    // a step up moves the body a block in one frame, so the eyes start where they were and ease up
    let rose = position.y - BLOCK_CORNER.y - transform.translation.y;
    // on sculpted ground only a rise sharper than walking up its slopes counts
    let least = if ground_near.is_empty() { 0.0 } else { SCULPT_RISE_EASE };
    if step > 0.0 && rose > least {
        player.eye_lag = (player.eye_lag + rose).min(STEP_HEIGHT);
        player.step_cooldown = STEP_COOLDOWN;
    }
    transform.translation = position - BLOCK_CORNER;
    let fall = -player.velocity.y;
    let was_grounded = player.grounded;
    player.grounded = hit.y && player.velocity.y < 0.0;
    if player.grounded && !was_grounded && !swimming && !wading && fall >= LAND_MIN_SPEED {
        player.landed = fall;
    }
    if hit.y {
        player.velocity.y = 0.0;
    }
    if hit.x {
        player.velocity.x = 0.0;
    }
    if hit.z {
        player.velocity.z = 0.0;
    }
    player.against_wall = (hit.x || hit.z || walled) && across != Vec3::ZERO;
}

/// Whether the cursor is held in the window, so the mouse turns the player.
pub(crate) fn cursor_grabbed(cursor: Query<&CursorOptions, With<PrimaryWindow>>) -> bool {
    cursor
        .single()
        .is_ok_and(|cursor| cursor.grab_mode != CursorGrabMode::None)
}

/// Holds the cursor in the window once it's clicked into, and lets it go on Escape or when the
/// window loses focus.
pub(crate) fn grab_cursor(
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut windows: Query<(&Window, &mut CursorOptions), With<PrimaryWindow>>,
) {
    let Ok((window, mut cursor)) = windows.single_mut() else {
        return;
    };
    let grabbed = cursor.grab_mode != CursorGrabMode::None;
    if grabbed && (keys.just_pressed(KeyCode::Escape) || !window.focused) {
        cursor.grab_mode = CursorGrabMode::None;
        cursor.visible = true;
    } else if !grabbed && window.focused && buttons.just_pressed(MouseButton::Left) {
        // X11 can't lock the cursor, so it's confined to the window there instead
        cursor.grab_mode = CursorGrabMode::Locked;
        cursor.visible = false;
    }
}

fn player_look(
    time: Res<Time>,
    mut player_query: Query<(&mut Transform, &mut Player)>,
    mut camera_query: Query<PlayerCamera, (With<Camera>, Without<Player>)>,
    mut motion_evr: MessageReader<MouseMotion>,
    cursor: Query<&CursorOptions, With<PrimaryWindow>>,
) {
    if !cursor_grabbed(cursor) {
        motion_evr.clear();
        return;
    }
    let (parent, mut camera) = camera_query.single_mut().expect("camera does not exist");
    let (mut body, mut player) = player_query
        .get_mut(parent.parent())
        .expect("player does not exist");
    let turn = LOOK_SENSITIVITY * time.delta_secs();
    // the camera only ever turns about x, under the body's turn about y, and the jump's tip is taken off and put back on each frame
    let (tipped, _, _) = camera.rotation.to_euler(EulerRot::XYZ);
    let mut pitch = tipped - player.nudge;
    for ev in motion_evr.read() {
        body.rotate_axis(Dir3::Y, -ev.delta.x * turn);
        pitch = (pitch - ev.delta.y * turn).clamp(-MAX_PITCH, MAX_PITCH);
    }
    let tip = JUMP_PITCH * player.kick - LAND_PITCH * player.thud;
    let shown = (pitch + tip.to_radians()).clamp(-MAX_PITCH, MAX_PITCH);
    player.nudge = shown - pitch;
    camera.rotation = Quat::from_rotation_x(shown);
}

/// Walking and flying, looking round, and the view underwater. Spawns the player at the world's spawn, facing `look`.
pub struct PlayerPlugin {
    pub look: Vec3,
}

impl Default for PlayerPlugin {
    fn default() -> Self {
        Self { look: Vec3::Z }
    }
}

#[derive(Resource)]
struct PlayerSpawn {
    look: Vec3,
}

impl Plugin for PlayerPlugin {
    fn build(&self, app: &mut App) {
        // the cold grade follows the climate it senses
        if !app.is_plugin_added::<SurroundingsPlugin>() {
            app.add_plugins(SurroundingsPlugin);
        }
        app.add_plugins(MaterialPlugin::<MurkMaterial>::default())
            .add_message::<Footstep>()
            .insert_resource(PlayerSpawn { look: self.look })
            .init_resource::<ColdGrade>()
            .add_systems(Startup, spawn_player)
            .add_systems(
                Update,
                (
                    player_move,
                    grab_cursor,
                    player_look,
                    update_horizon,
                    lift_cold_grade,
                    update_underwater_effects,
                    apply_cold_grade,
                )
                    .chain(),
            );
    }
}

/// Spawns the player at the world's spawn, with a camera that sees out to the renderer's fog distance, and the murk the
/// camera sees through underwater.
fn spawn_player(
    mut commands: Commands,
    mut assets: ViewAssets,
    renderer: Res<RendererSettings>,
    world: Res<World>,
    spawn: Res<PlayerSpawn>,
) {
    let (position, look) = (world.spawn(), spawn.look);
    info!("spawned at {position:?}");
    let player = commands
        .spawn((
            Player::default(),
            Transform::from_translation(position).looking_to(look, Vec3::Y),
        ))
        .id();
    spawn_camera(&mut commands, &mut assets, &renderer, player);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_footfalls_a_stride_where_the_head_is_lowest() {
        let count = (0..2000)
            .filter(|i| {
                footfall(
                    *i as f32 * TAU / 2000.0,
                    (*i + 1) as f32 * TAU / 2000.0 % TAU,
                )
            })
            .count();
        assert_eq!(count, 2);
        assert!(footfall(FRAC_PI_2 - 0.01, FRAC_PI_2 + 0.01));
        assert!(!footfall(0.1, 0.2));
    }

    // In a two-block gap the camera is `BODY.max.y - EYE_HEIGHT` under the ceiling, and a view of `fov` degrees at `pitch` has its near
    // plane's top edge `NEAR * (sin(pitch) + cos(pitch) * tan(fov / 2))` above the eye: it must never reach it, however far up the
    // player looks, or the camera is inside the block above.
    #[test]
    fn test_the_near_plane_stays_out_of_the_ceiling_of_a_two_block_gap() {
        let headroom = BODY.max.y - EYE_HEIGHT;
        let reach = |pitch: f32| NEAR * (pitch.sin() + pitch.cos() * ((FOV + SPRINT_FOV + JUMP_FOV).to_radians() / 2.0).tan());
        for degrees in 0..=89 {
            let up = reach((degrees as f32).to_radians());
            assert!(up < headroom, "at {degrees} degrees up the near plane reaches {up}, headroom {headroom}");
        }
        assert!(reach(MAX_PITCH) < headroom);
        // the eye is inside the body, which fits a two-block gap exactly
        const { assert!(EYE_HEIGHT < BODY.max.y && BODY.max.y == 2.0) };
    }
}
