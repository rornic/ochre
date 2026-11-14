use bevy::math::{BVec3, I64Vec3, Vec3};

// Overlaps thinner than this don't count, so a box resting on a face, give or take rounding,
// isn't taken to be inside the block under it and fall through.
const EPSILON: f32 = 1e-3;

/// How far into the face of a ledge it can step onto the box walks before it steps, so the step isn't taken a body's width away from
/// the ledge. It is also how shallow an overlap on a horizontal axis still stops the box and pushes it back out, so a box in the air
/// can't be walked on through the ledge it walked into.
pub const STEP_REACH: f32 = 0.25;

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
        let shallow = if axis == 1 { 0.0 } else { STEP_REACH };
        let (moved, stopped) = sweep_axis(position, aabb, axis, delta[axis], shallow, &solid);
        position[axis] += moved;
        hit.set(axis, stopped);
    }
    (position, hit)
}

/// Like `move_and_collide`, but a box blocked sideways tries again from `step` higher and
/// settles back down, so it climbs ledges up to `step` tall. It walks `STEP_REACH` into a ledge it can climb before it does.
/// Pass a `step` of 0 in the air.
pub fn move_and_step(
    position: Vec3,
    delta: Vec3,
    aabb: Aabb,
    step: f32,
    solid: impl Fn(I64Vec3) -> bool,
) -> (Vec3, BVec3) {
    let flat = move_and_collide(position, delta, aabb, &solid);
    if step <= 0.0 || delta.y > 0.0 {
        return flat;
    }
    // the box as a ledge's face meets it: thinner by `STEP_REACH` a side, so it gets that far in first
    let inset = Vec3::new(STEP_REACH, 0.0, STEP_REACH);
    let thin = Aabb {
        min: aabb.min + inset,
        max: aabb.max - inset,
    };
    let walked = move_and_collide(position, delta, thin, &solid);
    let sideways = |hit: BVec3| hit.x || hit.z;
    if !sideways(flat.1) && !sideways(walked.1) {
        return flat;
    }
    match climb(position, delta, aabb, step, &solid, flat.0) {
        // the thin box is against it, so up it goes
        Some(climbed) if sideways(walked.1) => climbed,
        // a ledge the box can climb: it walks in a little further first
        Some(_) => walked,
        None => flat,
    }
}

// Where the box ends up if it rises `step`, moves across and settles back down, when that lands it on something and gets it further
// along its way than `flat`, where walking into the wall left it, which can be backwards if it was in the wall's face.
fn climb(
    position: Vec3,
    delta: Vec3,
    aabb: Aabb,
    step: f32,
    solid: &impl Fn(I64Vec3) -> bool,
    flat: Vec3,
) -> Option<(Vec3, BVec3)> {
    let sideways = Vec3::new(delta.x, 0.0, delta.z);
    let (raised, _) = move_and_collide(position, Vec3::Y * step, aabb, solid);
    let (across, hit) = move_and_collide(raised, sideways, aabb, solid);
    let (settled, landed) = move_and_collide(across, Vec3::Y * (delta.y - step), aabb, solid);
    let gained = |p: Vec3| (p.x - position.x) * delta.x + (p.z - position.z) * delta.z;
    (landed.y && gained(settled) > gained(flat)).then_some((settled, BVec3::new(hit.x, true, hit.z)))
}

// How far the box gets along `axis` towards `delta`, and whether a block stopped it. A block it overlaps by no more than `shallow` along
// the axis stops it too, and moves it back out of the block; deeper overlaps are ignored, so it can always get out.
fn sweep_axis(
    position: Vec3,
    aabb: Aabb,
    axis: usize,
    delta: f32,
    shallow: f32,
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
        let inside = edge - edge.floor();
        // a cell the face has only just entered counts too
        let first = if inside > EPSILON && inside <= shallow + EPSILON {
            edge.floor() as i64
        } else {
            (edge - EPSILON).ceil() as i64
        };
        let last = (edge + delta - EPSILON).ceil() as i64 - 1;
        match (first..=last).find(|&cell| blocked(cell)) {
            Some(cell) => (cell as f32 - edge, true),
            None => (delta, false),
        }
    } else {
        let edge = min[axis];
        let inside = edge.floor() + 1.0 - edge;
        let first = if inside > EPSILON && inside <= shallow + EPSILON {
            edge.floor() as i64
        } else {
            (edge + EPSILON).floor() as i64 - 1
        };
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

    // walks at `speed` a frame along +x from `x`, a little gravity each frame, and gives each frame's position
    fn walk(x: f32, speed: f32, solid: impl Fn(I64Vec3) -> bool + Copy) -> Vec<Vec3> {
        let mut position = Vec3::new(x, 0.0, 0.5);
        (0..120)
            .map(|_| {
                position = move_and_step(position, Vec3::new(speed, -0.05, 0.0), BOX, 1.0, solid).0;
                position
            })
            .collect()
    }

    #[test]
    fn test_walks_into_a_ledge_it_can_climb_before_stepping_up() {
        let frames = walk(1.0, 0.04, ledge);
        let stepped = frames.iter().position(|p| p.y > 0.5).expect("never stepped up");
        // contact is at 1.7, and the step comes once the box is `STEP_REACH` further in
        let before = frames[stepped - 1].x;
        assert!(before > 1.7 + STEP_REACH - 0.06 && before <= 1.7 + STEP_REACH + 1e-3, "{before}");
        assert_eq!(frames[stepped - 1].y, 0.0);
        assert!((frames[stepped].y - 1.0).abs() < 1e-5);
    }

    #[test]
    fn test_walks_no_closer_to_a_wall_it_cannot_climb() {
        let wall = |cell: I64Vec3| cell.y < 2 * i64::from(cell.x >= 2);
        let end = *walk(1.0, 0.04, wall).last().unwrap();
        assert!((end.x - 1.7).abs() < 1e-4 && end.y == 0.0, "{end}");
    }

    #[test]
    fn test_a_box_in_the_air_is_pushed_back_out_of_a_ledge_it_walked_into() {
        // 0.25 into the ledge's face, the most a walk in goes
        let (position, hit) = move_and_step(Vec3::new(1.7 + STEP_REACH, 0.0, 0.5), Vec3::X * 0.1, BOX, 0.0, ledge);
        assert!((position.x - 1.7).abs() < 1e-5);
        assert!(hit.x);
        // and moving away is free
        let (position, hit) = move_and_step(Vec3::new(1.7 + STEP_REACH, 0.0, 0.5), -Vec3::X * 0.1, BOX, 0.0, ledge);
        assert!((position.x - (1.7 + STEP_REACH - 0.1)).abs() < 1e-5);
        assert!(!hit.x);
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
}
