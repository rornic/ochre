use bevy::math::{BVec3, I64Vec3, Vec3};

// Overlaps thinner than this don't count, so a box resting on a face, give or take rounding,
// isn't taken to be inside the block under it and fall through.
const EPSILON: f32 = 1e-3;

/// An axis-aligned box, as offsets from the position it's moved by.
#[derive(Debug, Clone, Copy)]
pub struct Aabb {
    pub min: Vec3,
    pub max: Vec3,
}

/// Moves `aabb` at `position` by `delta` an axis at a time, up first, stopping each axis
/// against the first block `solid` says it would move into. Returns where it ends up, and
/// which axes were stopped. Blocks it already overlaps are ignored, so it can always get out
/// of them.
pub fn move_and_collide(
    position: Vec3,
    delta: Vec3,
    aabb: Aabb,
    solid: impl Fn(I64Vec3) -> bool,
) -> (Vec3, BVec3) {
    let mut position = position;
    let mut hit = BVec3::FALSE;
    for axis in [1, 0, 2] {
        let (moved, stopped) = sweep_axis(position, aabb, axis, delta[axis], &solid);
        position[axis] += moved;
        hit.set(axis, stopped);
    }
    (position, hit)
}

/// Like `move_and_collide`, but a box blocked sideways tries again from `step` higher and
/// settles back down, so it climbs ledges up to `step` tall. Pass a `step` of 0 in the air.
pub fn move_and_step(
    position: Vec3,
    delta: Vec3,
    aabb: Aabb,
    step: f32,
    solid: impl Fn(I64Vec3) -> bool,
) -> (Vec3, BVec3) {
    let flat = move_and_collide(position, delta, aabb, &solid);
    if step <= 0.0 || delta.y > 0.0 || !(flat.1.x || flat.1.z) {
        return flat;
    }
    let sideways = Vec3::new(delta.x, 0.0, delta.z);
    let (raised, _) = move_and_collide(position, Vec3::Y * step, aabb, &solid);
    let (across, hit) = move_and_collide(raised, sideways, aabb, &solid);
    let (settled, landed) = move_and_collide(across, Vec3::Y * (delta.y - step), aabb, &solid);
    let gained = |p: Vec3| (p.x - position.x).powi(2) + (p.z - position.z).powi(2);
    // only a step that ends up on something and gets further than walking into the wall
    if landed.y && gained(settled) > gained(flat.0) {
        (settled, BVec3::new(hit.x, true, hit.z))
    } else {
        flat
    }
}

// How far the box gets along `axis` towards `delta`, and whether a block stopped it.
fn sweep_axis(
    position: Vec3,
    aabb: Aabb,
    axis: usize,
    delta: f32,
    solid: &impl Fn(I64Vec3) -> bool,
) -> (f32, bool) {
    if delta == 0.0 {
        return (0.0, false);
    }
    let min = position + aabb.min;
    let max = position + aabb.max;
    // the cells the box covers across the other two axes
    let [a, b] = [(axis + 1) % 3, (axis + 2) % 3];
    let span = |i: usize| (min[i] + EPSILON).floor() as i64..=(max[i] - EPSILON).floor() as i64;
    let blocked = |cell: i64| {
        span(a).any(|x| {
            span(b).any(|y| {
                let mut coord = I64Vec3::ZERO;
                coord[axis] = cell;
                coord[a] = x;
                coord[b] = y;
                solid(coord)
            })
        })
    };

    if delta > 0.0 {
        // the cells past the leading face, up to where it ends up
        let edge = max[axis];
        let first = (edge - EPSILON).ceil() as i64;
        let last = (edge + delta - EPSILON).ceil() as i64 - 1;
        match (first..=last).find(|&cell| blocked(cell)) {
            Some(cell) => (cell as f32 - edge, true),
            None => (delta, false),
        }
    } else {
        let edge = min[axis];
        let first = (edge + EPSILON).floor() as i64 - 1;
        let last = (edge + delta + EPSILON).floor() as i64;
        match (last..=first).rev().find(|&cell| blocked(cell)) {
            Some(cell) => ((cell + 1) as f32 - edge, true),
            None => (delta, false),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOX: Aabb = Aabb {
        min: Vec3::new(-0.3, 0.0, -0.3),
        max: Vec3::new(0.3, 1.8, 0.3),
    };

    // solid at and below y = 0
    fn floor(cell: I64Vec3) -> bool {
        cell.y < 0
    }

    #[test]
    fn test_falls_onto_the_floor_and_stops_on_it() {
        let (position, hit) =
            move_and_collide(Vec3::new(0.5, 3.0, 0.5), -Vec3::Y * 10.0, BOX, floor);
        assert_eq!(position, Vec3::new(0.5, 0.0, 0.5));
        assert!(hit.y && !hit.x && !hit.z);
    }

    #[test]
    fn test_falls_freely_where_nothing_is_solid() {
        let (position, hit) = move_and_collide(Vec3::new(0.5, 3.0, 0.5), -Vec3::Y, BOX, floor);
        assert_eq!(position, Vec3::new(0.5, 2.0, 0.5));
        assert_eq!(hit, BVec3::FALSE);
    }

    #[test]
    fn test_rests_on_the_floor_over_many_small_steps() {
        let mut position = Vec3::new(0.5, 0.0, 0.5);
        for _ in 0..1000 {
            position = move_and_collide(position, Vec3::new(0.001, -0.016, 0.0), BOX, floor).0;
        }
        assert_eq!(position.y, 0.0);
        assert!((position.x - 1.5).abs() < 1e-3);
    }

    #[test]
    fn test_slides_along_a_wall() {
        let wall = |cell: I64Vec3| floor(cell) || cell.x >= 2;
        let (position, hit) = move_and_collide(
            Vec3::new(0.5, 0.0, 0.5),
            Vec3::new(5.0, 0.0, 3.0),
            BOX,
            wall,
        );
        assert!((position.x - 1.7).abs() < 1e-5);
        assert_eq!(position.z, 3.5);
        assert!(hit.x && !hit.z);
    }

    #[test]
    fn test_stops_under_a_ceiling() {
        let ceiling = |cell: I64Vec3| cell.y >= 3;
        let (position, hit) =
            move_and_collide(Vec3::new(0.5, 0.0, 0.5), Vec3::Y * 5.0, BOX, ceiling);
        assert!((position.y - 1.2).abs() < 1e-5);
        assert!(hit.y);
    }

    #[test]
    fn test_moves_out_of_a_block_it_starts_inside() {
        let one_block = |cell: I64Vec3| cell == I64Vec3::new(0, 1, 0);
        let (position, hit) =
            move_and_collide(Vec3::new(0.5, 0.5, 0.5), Vec3::X * 2.0, BOX, one_block);
        assert_eq!(position, Vec3::new(2.5, 0.5, 0.5));
        assert_eq!(hit, BVec3::FALSE);
    }

    #[test]
    fn test_doesnt_tunnel_through_a_thin_wall_in_one_big_step() {
        let wall = |cell: I64Vec3| cell.x == 10;
        let (position, hit) =
            move_and_collide(Vec3::new(0.5, 0.0, 0.5), Vec3::X * 100.0, BOX, wall);
        assert!((position.x - 9.7).abs() < 1e-5);
        assert!(hit.x);
    }

    #[test]
    fn test_fits_through_a_gap_its_own_width() {
        // walls at x = -1 and x = 1, so the one-block gap at x = 0 is open
        let walls = |cell: I64Vec3| cell.x != 0;
        let (position, hit) = move_and_collide(Vec3::new(0.5, 0.0, 0.5), Vec3::Z * 4.0, BOX, walls);
        assert_eq!(position.z, 4.5);
        assert!(!hit.z);
    }

    // a floor with a one-block ledge from x = 2 on
    fn ledge(cell: I64Vec3) -> bool {
        cell.y < i64::from(cell.x >= 2)
    }

    #[test]
    fn test_steps_up_a_one_block_ledge() {
        let (position, hit) = move_and_step(
            Vec3::new(1.6, 0.0, 0.5),
            Vec3::new(0.5, -0.5, 0.0),
            BOX,
            1.0,
            ledge,
        );
        assert!((position.x - 2.1).abs() < 1e-5);
        assert!((position.y - 1.0).abs() < 1e-5);
        assert!(hit.y && !hit.x);
    }

    #[test]
    fn test_doesnt_step_up_in_the_air() {
        let (position, _) = move_and_step(
            Vec3::new(1.6, 0.0, 0.5),
            Vec3::X * 0.5,
            BOX,
            0.0,
            ledge,
        );
        assert!((position.x - 1.7).abs() < 1e-5);
        assert_eq!(position.y, 0.0);
    }

    #[test]
    fn test_doesnt_step_up_a_two_block_wall() {
        let wall = |cell: I64Vec3| cell.y < 2 * i64::from(cell.x >= 2);
        let (position, hit) = move_and_step(
            Vec3::new(1.6, 0.0, 0.5),
            Vec3::new(0.5, -0.5, 0.0),
            BOX,
            1.0,
            wall,
        );
        assert!((position.x - 1.7).abs() < 1e-5);
        assert_eq!(position.y, 0.0);
        assert!(hit.x);
    }

    #[test]
    fn test_doesnt_step_up_into_a_low_ceiling() {
        let low = |cell: I64Vec3| ledge(cell) || cell.y >= 2;
        let (position, hit) = move_and_step(
            Vec3::new(1.6, 0.0, 0.5),
            Vec3::new(0.5, -0.5, 0.0),
            BOX,
            1.0,
            low,
        );
        assert!((position.x - 1.7).abs() < 1e-5);
        assert!(hit.x);
    }

    #[test]
    fn test_box_as_tall_as_a_gap_walks_through_it() {
        let tall = Aabb {
            min: BOX.min,
            max: Vec3::new(0.3, 2.0, 0.3),
        };
        let tunnel = |cell: I64Vec3| cell.y < 0 || cell.y >= 2;
        let (position, hit) =
            move_and_collide(Vec3::new(0.5, 0.0, 0.5), Vec3::Z * 4.0, tall, tunnel);
        assert_eq!(position, Vec3::new(0.5, 0.0, 4.5));
        assert_eq!(hit, BVec3::FALSE);
    }

    // SCRATCH: a simulated player walks random block fields, as `walk` in player.rs moves it, and must never end a frame inside a block.
    #[test]
    fn tmp_random_walks_never_enter_a_block() {
        const BODY: Aabb = Aabb { min: Vec3::new(-0.3, 0.0, -0.3), max: Vec3::new(0.3, 2.0, 0.3) };
        let hash = |a: i64, b: i64, c: i64, seed: u64| -> u64 {
            let mut h = seed ^ (a as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ (b as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F) ^ (c as u64).wrapping_mul(0x1656_67B1_9E37_79F9);
            h = (h ^ (h >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            h = (h ^ (h >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            h ^ (h >> 31)
        };
        let mut worst: Option<String> = None;
        let mut violations = 0;
        for world in 0..300u64 {
            let ceilings = world % 2 == 1;
            let solid = |c: I64Vec3| -> bool {
                if c.y < 0 {
                    return true;
                }
                let ground = (hash(c.x, c.z, 0, world) % 4) as i64; // 0 to 3 high columns
                if c.y < ground {
                    return true;
                }
                // floating ledges two or three above the ground, in half the worlds
                ceilings && c.y >= ground + 2 && c.y <= ground + 3 && hash(c.x, c.z, 1, world) % 100 < 35
            };
            let mut position = Vec3::new(0.5, 12.0, 0.5);
            let mut velocity = Vec3::ZERO;
            let (mut grounded, mut cooldown) = (false, 0.0f32);
            let mut wish = Vec3::ZERO;
            let dt = 1.0 / 60.0;
            for frame in 0..6000u64 {
                if frame % 37 == 0 {
                    let r = hash(frame as i64, 7, 7, world);
                    let a = (r % 8) as f32 * std::f32::consts::FRAC_PI_4;
                    wish = if r % 11 == 0 { Vec3::ZERO } else { Vec3::new(a.cos(), 0.0, a.sin()) * 5.0 };
                }
                velocity.x = wish.x;
                velocity.z = wish.z;
                if grounded && hash(frame as i64, 3, 3, world) % 60 == 0 {
                    velocity.y = 9.0;
                }
                velocity.y = (velocity.y - 32.0 * dt).max(-60.0);
                cooldown = (cooldown - dt).max(0.0);
                let step = if grounded && cooldown == 0.0 { 1.0 } else { 0.0 };
                let before = position;
                let (moved, hit) = move_and_step(position, velocity * dt, BODY, step, solid);
                if step > 0.0 && moved.y > before.y {
                    cooldown = 0.3;
                }
                position = moved;
                grounded = hit.y && velocity.y < 0.0;
                if hit.y { velocity.y = 0.0; }
                if hit.x { velocity.x = 0.0; }
                if hit.z { velocity.z = 0.0; }
                // inside a block, by more than rounding
                let (lo, hi) = (position + BODY.min, position + BODY.max);
                let cells = |i: usize| (lo[i] + EPSILON * 5.0).floor() as i64..=(hi[i] - EPSILON * 5.0).floor() as i64;
                let inside = cells(0).flat_map(|x| cells(1).flat_map(move |y| cells(2).map(move |z| I64Vec3::new(x, y, z)))).find(|&c| solid(c));
                if let Some(cell) = inside {
                    violations += 1;
                    if worst.is_none() {
                        worst = Some(format!("world {world} (ceilings {ceilings}) frame {frame}: body at {position}, inside block {cell}, was at {before}, step {step}, velocity {velocity}"));
                    }
                    break; // one per world
                }
            }
        }
        eprintln!("RANDOM WALKS: {violations} of 300 worlds ended a frame inside a block");
        if let Some(w) = &worst {
            eprintln!("FIRST: {w}");
        }
        assert_eq!(0, violations, "{worst:?}");
    }
}

