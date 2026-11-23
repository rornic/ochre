use bevy::math::Vec3;

// A body as spheres stacked over its feet, which round over sculpted ground's bumps and creases.
pub(crate) struct Body {
    pub(crate) radius: f32,
    // each sphere's centre over the feet
    pub(crate) heights: [f32; 3],
}

// The point on triangle `a`, `b`, `c` nearest `p` (Ericson, Real-Time Collision Detection, 5.1.5).
pub(crate) fn nearest_on_triangle(p: Vec3, a: Vec3, b: Vec3, c: Vec3) -> Vec3 {
    let (ab, ac, ap) = (b - a, c - a, p - a);
    let (d1, d2) = (ab.dot(ap), ac.dot(ap));
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    }
    let bp = p - b;
    let (d3, d4) = (ab.dot(bp), ac.dot(bp));
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        return a + ab * (d1 / (d1 - d3));
    }
    let cp = p - c;
    let (d5, d6) = (ab.dot(cp), ac.dot(cp));
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        return a + ac * (d2 / (d2 - d6));
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && d4 - d3 >= 0.0 && d5 - d6 >= 0.0 {
        return b + (c - b) * ((d4 - d3) / ((d4 - d3) + (d5 - d6)));
    }
    let denom = 1.0 / (va + vb + vc);
    a + ab * (vb * denom) + ac * (vc * denom)
}

// What a move over sculpted ground did.
#[derive(Debug, Default, PartialEq)]
pub(crate) struct Moved {
    pub(crate) feet: Vec3,
    // standing on walkable ground at the end of it
    pub(crate) grounded: bool,
    // the walls it was pushed off, their normals summed, level
    pub(crate) wall: Vec3,
    // it rose into a ceiling
    pub(crate) bumped: bool,
    // the outward normal of a face too steep to stand on that it came to rest on, else zero
    pub(crate) slope: Vec3,
}

// How the body moves over sculpted ground: walls and ground kept apart, so neither undoes the other.
pub(crate) struct Walker<'a> {
    pub(crate) body: &'a Body,
    pub(crate) triangles: &'a [[Vec3; 3]],
    // the least a face's normal points up to walk on; steeper faces are walls
    pub(crate) walkable: f32,
    // the tallest rise walked up without a jump, and how far down standing feet follow the ground
    pub(crate) step: f32,
    pub(crate) snap: f32,
    // the highest walkable surface in a column from `below` under a point to `above` over it
    pub(crate) ground: &'a dyn Fn(Vec3, f32, f32) -> Option<f32>,
    // the same for any face that looks up, steep or not, which a fall lands on
    pub(crate) support: &'a dyn Fn(Vec3, f32, f32) -> Option<f32>,
}

// Points under the body the ground is looked up at: its middle and round its edge.
const FOOTPRINT: [(f32, f32); 5] = [(0.0, 0.0), (0.7, 0.0), (-0.7, 0.0), (0.0, 0.7), (0.0, -0.7)];

impl Walker<'_> {
    // Walls only where touched over a step, pushing level: what's lower the ground lookup steps up onto instead.
    pub(crate) fn wall_out(&self, mut feet: Vec3, wall: &mut Vec3) -> Vec3 {
        for _ in 0..4 {
            let mut moved = false;
            for &height in &self.body.heights {
                for &[a, b, c] in self.triangles {
                    let outward = (c - a).cross(b - a).normalize_or_zero();
                    if outward.y >= self.walkable || outward.y < -0.5 {
                        continue;
                    }
                    let centre = feet + Vec3::Y * height;
                    let touch = nearest_on_triangle(centre, a, b, c);
                    let off = centre - touch;
                    let level = Vec3::new(off.x, 0.0, off.z);
                    // a face lower than a step, with walkable ground on top of it within a step, is a step and not a wall
                    if touch.y < feet.y + self.step + 0.05 && level.length() > 1e-6 {
                        let past = touch - level.normalize() * 0.1;
                        if (self.ground)(past.with_y(feet.y), 0.05, self.step + 0.05).is_some() {
                            continue;
                        }
                    }
                    let distance = level.length();
                    if off.length() >= self.body.radius || distance < 1e-6 {
                        continue;
                    }
                    let side = level / distance;
                    feet += side * (self.body.radius - off.length()).max(0.0);
                    *wall += side;
                    moved = true;
                }
            }
            if !moved {
                break;
            }
        }
        feet
    }

    fn height(&self) -> f32 {
        self.body.heights.last().copied().unwrap_or(0.0) + self.body.radius
    }

    // Headroom over `feet` to the lowest downward face over the footprint, or a height and a step if none.
    pub(crate) fn headroom(&self, feet: Vec3) -> f32 {
        let reach = self.height() + self.step;
        let mut room = reach;
        for &[a, b, c] in self.triangles {
            if (c - a).cross(b - a).normalize_or_zero().y > -0.3 {
                continue;
            }
            let area = (b.x - a.x) * (c.z - a.z) - (c.x - a.x) * (b.z - a.z);
            if area.abs() < 1e-6 {
                continue;
            }
            for &(x, z) in &FOOTPRINT {
                let p = feet + Vec3::new(x, 0.0, z) * self.body.radius;
                let u = ((b.x - p.x) * (c.z - p.z) - (c.x - p.x) * (b.z - p.z)) / area;
                let v = ((c.x - p.x) * (a.z - p.z) - (a.x - p.x) * (c.z - p.z)) / area;
                let w = 1.0 - u - v;
                if u >= -1e-4 && v >= -1e-4 && w >= -1e-4 {
                    let over = a.y * u + b.y * v + c.y * w - feet.y;
                    if over > 0.05 && over < room {
                        room = over;
                    }
                }
            }
        }
        room
    }

    // The outward normal of an upward face whose surface under `p` is at `y`.
    fn face_at(&self, p: Vec3, y: f32) -> Option<Vec3> {
        self.triangles.iter().find_map(|&[a, b, c]| {
            let outward = (c - a).cross(b - a).normalize_or_zero();
            let area = (b.x - a.x) * (c.z - a.z) - (c.x - a.x) * (b.z - a.z);
            if outward.y <= 0.0 || area.abs() < 1e-6 {
                return None;
            }
            let u = ((b.x - p.x) * (c.z - p.z) - (c.x - p.x) * (b.z - p.z)) / area;
            let v = ((c.x - p.x) * (a.z - p.z) - (a.x - p.x) * (c.z - p.z)) / area;
            let w = 1.0 - u - v;
            let on = u >= -1e-4 && v >= -1e-4 && w >= -1e-4 && (a.y * u + b.y * v + c.y * w - y).abs() < 0.01;
            on.then_some(outward)
        })
    }

    // The highest walkable ground under the body's footprint, from `below` the feet to a step over them.
    pub(crate) fn ground_under(&self, feet: Vec3, below: f32) -> Option<f32> {
        FOOTPRINT
            .iter()
            .filter_map(|&(x, z)| (self.ground)(feet + Vec3::new(x, 0.0, z) * self.body.radius, below, self.step))
            .max_by(f32::total_cmp)
    }

    // Moves the body by `delta`: across in small steps against walls, then up or down onto the ground.
    pub(crate) fn walk(&self, feet: Vec3, delta: Vec3, standing: bool, across_blocks: &dyn Fn(Vec3, Vec3) -> Vec3) -> Moved {
        let mut moved = Moved { feet, ..Default::default() };
        let across = Vec3::new(delta.x, 0.0, delta.z);
        let steps = (across.length() / SUBSTEP).ceil().max(1.0) as usize;
        let stand = |p: Vec3| p.with_y(self.ground_under(p, self.snap).unwrap_or(p.y).max(p.y));
        for _ in 0..steps {
            let next = across_blocks(moved.feet, across / steps as f32);
            // standing, walls are judged at the height it will stand at, or the lift onto the ground flips the answer
            let judged = if standing { stand(next) } else { next };
            let next = self.wall_out(judged, &mut moved.wall).with_y(next.y);
            // too low under a ceiling, standing where it would stand there, it goes no further, unless it's backing out of one
            let (here, there) = (self.headroom(stand(moved.feet)), self.headroom(stand(next)));
            if there < self.height() && there < here {
                moved.wall -= across.normalize_or_zero();
                break;
            }
            moved.feet = next;
        }
        // standing feet follow the ground down a slope; falling ones land on what they reach
        let below = if standing { self.snap } else { (-delta.y).max(0.0) + 0.05 };
        let fallen = moved.feet.y + delta.y;
        match self.ground_under(moved.feet, below) {
            Some(height) if (standing && delta.y <= 0.0) || fallen <= height => {
                // a lift onto higher ground stops short where a ceiling leaves no room for the head
                let room = self.headroom(moved.feet);
                let lift = (height - moved.feet.y).min((room - self.height()).max(0.0));
                moved.feet.y = if height > moved.feet.y { moved.feet.y + lift } else { height };
                moved.grounded = true;
                // walls and ground settled together, so a lift onto a shelf the wall then pushes it off isn't undone the step after
                for _ in 0..3 {
                    let out = self.wall_out(moved.feet, &mut moved.wall);
                    if out == moved.feet {
                        break;
                    }
                    let Some(height) = self.ground_under(out, self.snap) else { break };
                    moved.feet = out.with_y(height.min(moved.feet.y + (self.headroom(out) - self.height()).max(0.0)));
                }
            }
            // off walkable ground a fall still lands on whatever looks up, steep or not, so it can't pass into the ground
            _ => {
                let landing = FOOTPRINT
                    .iter()
                    .filter_map(|&(x, z)| {
                        let p = moved.feet + Vec3::new(x, 0.0, z) * self.body.radius;
                        (self.support)(p, (-delta.y).max(0.0) + 0.05, 0.05).map(|y| (p, y))
                    })
                    .max_by(|a, b| a.1.total_cmp(&b.1));
                moved.feet.y = match landing {
                    Some((p, height)) if fallen <= height => {
                        moved.slope = self.face_at(p, height).unwrap_or(Vec3::Y);
                        height
                    }
                    _ => fallen,
                };
            }
        }
        // pressing into a wall, a step ending a little behind its start is a bounce, so it keeps only its slide
        let way = across.normalize_or_zero();
        let back = (moved.feet - feet).with_y(0.0).dot(way);
        if standing && moved.grounded && back < 0.0 && back > -BOUNCE {
            let slid = moved.feet - way * back;
            moved.feet = slid.with_y(self.ground_under(slid, self.snap).unwrap_or(moved.feet.y));
        }
        // a rise stops at a ceiling over the head
        if delta.y > 0.0 {
            let head = *self.body.heights.last().unwrap_or(&0.0);
            for &[a, b, c] in self.triangles {
                let outward = (c - a).cross(b - a).normalize_or_zero();
                if outward.y > -0.5 {
                    continue;
                }
                let centre = moved.feet + Vec3::Y * head;
                let off = centre - nearest_on_triangle(centre, a, b, c);
                if off.length() < self.body.radius && off.y < 0.0 {
                    moved.feet.y -= self.body.radius - off.length();
                    moved.bumped = true;
                }
            }
        }
        moved
    }
}

// The most the body moves across before walls push it back, in blocks, less than its spheres' radius.
const SUBSTEP: f32 = 0.2;
// The furthest back a wall pushes a standing body that counts as a bounce, in blocks; further is a real push out.
const BOUNCE: f32 = 0.2;

#[cfg(test)]
mod tests {
    use super::*;

    const BODY: Body = Body { radius: 0.3, heights: [0.3, 1.0, 1.7] };

    // ground at y = 0 for x < 1, and from x = 1 a ledge `rise` high, its face straight up
    fn ledge(rise: f32) -> Vec<[Vec3; 3]> {
        let quad = |a: Vec3, b: Vec3, c: Vec3, d: Vec3| [[a, b, c], [a, c, d]];
        let mut t = Vec::new();
        // wound with the cross product into the ground: the floor and top face up, the face toward -x
        t.extend(quad(Vec3::new(-4.0, 0.0, -4.0), Vec3::new(1.0, 0.0, -4.0), Vec3::new(1.0, 0.0, 4.0), Vec3::new(-4.0, 0.0, 4.0)));
        t.extend(quad(Vec3::new(1.0, rise, -4.0), Vec3::new(6.0, rise, -4.0), Vec3::new(6.0, rise, 4.0), Vec3::new(1.0, rise, 4.0)));
        t.extend(quad(Vec3::new(1.0, 0.0, -4.0), Vec3::new(1.0, 0.0, 4.0), Vec3::new(1.0, rise, 4.0), Vec3::new(1.0, rise, -4.0)));
        t
    }

    fn walker<'a>(triangles: &'a [[Vec3; 3]], ground: &'a dyn Fn(Vec3, f32, f32) -> Option<f32>) -> Walker<'a> {
        Walker { body: &BODY, triangles, walkable: 0.4, step: 1.0, snap: 0.6, ground, support: ground }
    }

    fn stepped_ground(rise: f32) -> impl Fn(Vec3, f32, f32) -> Option<f32> {
        move |p: Vec3, below: f32, above: f32| {
            let y = if p.x >= 1.0 { rise } else { 0.0 };
            (y >= p.y - below && y <= p.y + above).then_some(y)
        }
    }

    // walks along +x from x = 0 for a second at 4 blocks a second, standing
    fn walk_into(rise: f32) -> Moved {
        let (triangles, ground) = (ledge(rise), stepped_ground(rise));
        let w = walker(&triangles, &ground);
        let mut feet = Vec3::ZERO;
        let mut last = Moved::default();
        for _ in 0..60 {
            last = w.walk(feet, Vec3::new(4.0 / 60.0, -0.1, 0.0), true, &|f, d| f + d);
            feet = last.feet;
        }
        last
    }

    #[test]
    fn test_the_nearest_point_is_inside_on_an_edge_or_at_a_corner() {
        let (a, b, c) = (Vec3::ZERO, Vec3::X, Vec3::Z);
        let near = |p: Vec3, q: Vec3| nearest_on_triangle(p, a, b, c).distance(q) < 1e-5;
        assert!(near(Vec3::new(0.2, 3.0, 0.2), Vec3::new(0.2, 0.0, 0.2)));
        assert!(near(Vec3::new(0.5, 1.0, -2.0), Vec3::new(0.5, 0.0, 0.0)));
        assert!(near(Vec3::new(-1.0, 0.0, -1.0), a));
    }

    #[test]
    fn test_a_ledge_up_to_a_step_is_walked_up_onto() {
        for rise in [0.4, 0.8, 1.0] {
            let end = walk_into(rise);
            assert!(end.grounded && end.feet.x > 2.0 && (end.feet.y - rise).abs() < 1e-4, "rise {rise}: {end:?}");
        }
    }

    #[test]
    fn test_a_ledge_over_a_step_walls_the_body_without_lifting_it() {
        let end = walk_into(1.6);
        assert!(end.feet.x < 1.0 && end.feet.y.abs() < 1e-4 && end.grounded, "{end:?}");
        assert!(end.wall.x < 0.0);
    }

    #[test]
    fn test_sliding_along_a_wall_stays_steady() {
        let (triangles, ground) = (ledge(2.0), stepped_ground(2.0));
        let w = walker(&triangles, &ground);
        // pressed into the wall at 45 degrees, it slides along it at one height and one distance off it
        let mut feet = Vec3::new(0.65, 0.0, 0.0);
        let mut seen = Vec::new();
        for _ in 0..60 {
            feet = w.walk(feet, Vec3::new(0.05, -0.1, 0.05), true, &|f, d| f + d).feet;
            seen.push(feet);
        }
        for p in &seen[5..] {
            assert!(p.y.abs() < 1e-4 && (p.x - seen[5].x).abs() < 1e-3, "{p}");
        }
        assert!(seen[59].z > seen[5].z + 2.0);
    }

    #[test]
    fn test_a_falling_body_lands_on_the_ground_it_reaches() {
        let (triangles, ground) = (ledge(1.0), stepped_ground(1.0));
        let w = walker(&triangles, &ground);
        let mid_air = w.walk(Vec3::new(0.0, 3.0, 0.0), Vec3::new(0.0, -0.5, 0.0), false, &|f, d| f + d);
        assert!(!mid_air.grounded && (mid_air.feet.y - 2.5).abs() < 1e-4);
        let landed = w.walk(Vec3::new(0.0, 0.3, 0.0), Vec3::new(0.0, -0.5, 0.0), false, &|f, d| f + d);
        assert!(landed.grounded && landed.feet.y.abs() < 1e-4);
    }

    // flat ground for x < 1, then a slope rising 3 for 1 along x, too steep to walk and taller than a step
    #[allow(clippy::type_complexity)]
    fn cliff() -> (Vec<[Vec3; 3]>, impl Fn(Vec3, f32, f32) -> Option<f32>, impl Fn(Vec3, f32, f32) -> Option<f32>) {
        let quad = |a: Vec3, b: Vec3, c: Vec3, d: Vec3| [[a, b, c], [a, c, d]];
        let mut t = Vec::new();
        t.extend(quad(Vec3::new(-4.0, 0.0, -4.0), Vec3::new(1.0, 0.0, -4.0), Vec3::new(1.0, 0.0, 4.0), Vec3::new(-4.0, 0.0, 4.0)));
        t.extend(quad(Vec3::new(1.0, 0.0, -4.0), Vec3::new(4.0, 9.0, -4.0), Vec3::new(4.0, 9.0, 4.0), Vec3::new(1.0, 0.0, 4.0)));
        let walkable = |p: Vec3, below: f32, above: f32| (p.x < 1.0 && 0.0 >= p.y - below && 0.0 <= p.y + above).then_some(0.0);
        let any = |p: Vec3, below: f32, above: f32| {
            let y = if p.x < 1.0 { 0.0 } else { (p.x - 1.0) * 3.0 };
            (y >= p.y - below && y <= p.y + above).then_some(y)
        };
        (t, walkable, any)
    }

    #[test]
    fn test_a_slope_too_steep_and_too_tall_walls_the_body_out_of_it() {
        let (triangles, walkable, any) = cliff();
        let w = Walker { body: &BODY, triangles: &triangles, walkable: 0.4, step: 1.0, snap: 0.6, ground: &walkable, support: &any };
        let mut feet = Vec3::ZERO;
        for _ in 0..60 {
            feet = w.walk(feet, Vec3::new(4.0 / 60.0, -0.1, 0.0), true, &|f, d| f + d).feet;
        }
        assert!(feet.x < 1.0 && feet.y.abs() < 1e-4, "walked into the slope: {feet}");
    }

    #[test]
    fn test_a_fall_onto_a_steep_face_lands_on_it() {
        let (triangles, walkable, any) = cliff();
        let w = Walker { body: &BODY, triangles: &triangles, walkable: 0.4, step: 1.0, snap: 0.6, ground: &walkable, support: &any };
        let landed = w.walk(Vec3::new(2.0, 3.2, 0.0), Vec3::new(0.0, -0.5, 0.0), false, &|f, d| f + d);
        assert!(landed.feet.y >= 3.0 - 1e-4 - 0.3 * 0.7 * 3.0, "fell into the slope: {:?}", landed.feet);
    }

    // ground at y = 0, under a ceiling 2.2 over it from x = 1 on, too low to step up under once the ground rises a block
    #[test]
    fn test_the_head_stays_out_of_a_ceiling_too_low_to_walk_under() {
        let quad = |a: Vec3, b: Vec3, c: Vec3, d: Vec3| [[a, b, c], [a, c, d]];
        let mut triangles = ledge(1.0);
        // looking down, wound so the cross product points up into the rock over it
        triangles.extend(quad(Vec3::new(1.0, 2.2, -4.0), Vec3::new(1.0, 2.2, 4.0), Vec3::new(6.0, 2.2, 4.0), Vec3::new(6.0, 2.2, -4.0)));
        let ground = stepped_ground(1.0);
        let w = walker(&triangles, &ground);
        let mut feet = Vec3::ZERO;
        for _ in 0..60 {
            feet = w.walk(feet, Vec3::new(4.0 / 60.0, -0.1, 0.0), true, &|f, d| f + d).feet;
            assert!(w.headroom(feet) >= w.height() - 1e-3, "head in the ceiling at {feet}");
        }
        assert!(feet.x < 1.0, "{feet}");
    }

    // ---- spike: which staircase faces a jumping player can climb (docs/meso-spike.md) ----

    // `n` ledges from x = 1, each `rise` high with a flat `tread` and vertical face, over ground at 0.
    #[allow(clippy::type_complexity)]
    fn stairs(rise: f32, tread: f32, n: usize) -> (Vec<[Vec3; 3]>, impl Fn(Vec3, f32, f32) -> Option<f32>) {
        let quad = |a: Vec3, b: Vec3, c: Vec3, d: Vec3| [[a, b, c], [a, c, d]];
        let mut t = Vec::new();
        t.extend(quad(Vec3::new(-4.0, 0.0, -4.0), Vec3::new(1.0, 0.0, -4.0), Vec3::new(1.0, 0.0, 4.0), Vec3::new(-4.0, 0.0, 4.0)));
        for i in 0..n {
            let (x, y0, y1) = (1.0 + i as f32 * tread, i as f32 * rise, (i + 1) as f32 * rise);
            t.extend(quad(Vec3::new(x, y0, -4.0), Vec3::new(x, y0, 4.0), Vec3::new(x, y1, 4.0), Vec3::new(x, y1, -4.0)));
            let end = if i + 1 == n { x + 6.0 } else { x + tread };
            t.extend(quad(Vec3::new(x, y1, -4.0), Vec3::new(end, y1, -4.0), Vec3::new(end, y1, 4.0), Vec3::new(x, y1, 4.0)));
        }
        let height = move |x: f32| if x < 1.0 { 0.0 } else { ((((x - 1.0) / tread).floor() as usize + 1).min(n)) as f32 * rise };
        (t, move |p: Vec3, below: f32, above: f32| {
            let y = height(p.x);
            (y >= p.y - below && y <= p.y + above).then_some(y)
        })
    }

    // Height reached in 12 s walking +x at 4 blocks/s, jumping at every chance, at the game's jump speed and gravity.
    fn climb(rise: f32, tread: f32, n: usize) -> f32 {
        let (triangles, ground) = stairs(rise, tread, n);
        let w = walker(&triangles, &ground);
        let (mut feet, mut vy, mut grounded, mut best) = (Vec3::ZERO, 0.0f32, true, 0.0f32);
        for _ in 0..720 {
            let dt = 1.0 / 60.0;
            if grounded {
                vy = 9.0;
            }
            vy -= 32.0 * dt;
            let moved = w.walk(feet, Vec3::new(4.0 * dt, vy * dt, 0.0), false, &|f, d| f + d);
            feet = moved.feet;
            grounded = moved.grounded;
            if grounded && vy <= 0.0 {
                vy = 0.0;
            }
            if moved.bumped {
                vy = vy.min(0.0);
            }
            best = best.max(feet.y);
        }
        best
    }

    #[test]
    fn spike_which_ledge_heights_a_jumping_player_climbs() {
        eprintln!("rise  tread  top reached of 6 ledges");
        for rise in [1.0f32, 1.5, 2.0, 2.3, 2.6, 3.0, 4.0] {
            let row: Vec<String> = [0.6f32, 1.0, 2.0].iter().map(|&tread| format!("{:5.1}", climb(rise, tread, 6))).collect();
            eprintln!("{rise:4.1}  tread 0.6 / 1 / 2:  {}", row.join(" "));
        }
        eprintln!("the limit: rise 2.05 .. 2.30, tread 1");
        for rise in [2.05f32, 2.1, 2.15, 2.2, 2.25, 2.3] {
            eprintln!("  rise {rise:.2}: {:.1}", climb(rise, 1.0, 6));
        }
        eprintln!("how shallow a tread still gives footing, rise 1.5, 6 ledges (top 9)");
        for tread in [0.05f32, 0.1, 0.2, 0.3, 0.4, 0.5] {
            eprintln!("  tread {tread:.2}: {:.1}", climb(1.5, tread, 6));
        }
        // a rise of a step or a jump is climbed, one past both is a wall whatever the tread
        assert!(climb(1.0, 1.0, 6) > 5.9, "a step-high stair is climbed");
        assert!(climb(4.0, 1.0, 6) < 4.0, "a four-block face is not");
        // measured at jump 9, gravity 32, step 1: a 2.15 ledge is climbed, 2.2 is a wall; 0.2 deep holds, 0.1 doesn't
        assert!(climb(2.15, 1.0, 6) > 12.9 && climb(2.2, 1.0, 6) < 2.0);
        assert!(climb(1.5, 0.2, 6) > 8.9 && climb(1.5, 0.1, 6) < 2.0);
    }
}
