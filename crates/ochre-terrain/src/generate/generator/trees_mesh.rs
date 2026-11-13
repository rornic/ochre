use super::buffers::{GrassTint, MeshBuffers, UNFOLDED};
use super::shading::face_light;
use super::source::{BlockSource, TreeFaces};
use crate::generate::light::SkyLight;
use crate::generate::trees::TreeLook;
use bevy::math::{IVec3, Vec2, Vec3};
use ochre_base::block::{BlockType, Face, Tile};
use std::f32::consts::TAU;
use std::ops::Deref;

// How much wider than the trunk the lowest log is, where it meets the ground, at most this wide.
const FLARE: f32 = 1.25;
const FOOT_MAX: f32 = 0.55;
// A branch's radius against its tree's trunk, and a branch of no tree's.
const BRANCH: f32 = 0.7;
const LONE_BRANCH_RADIUS: f32 = 0.2;
const SIDES: usize = 8;

/// How far the trunk's centre line is from its voxel column at height `y`, in blocks: none at the foot, the lean at the top.
pub(super) fn trunk_offset(look: &TreeLook, y: f32) -> Vec2 {
    let up = ((y - (look.base.y - 0.5)) / look.height).clamp(0.0, 1.0);
    look.lean * up * up
}

// Two unit vectors across `axis`, which is a unit vector.
fn across(axis: Vec3) -> [Vec3; 2] {
    if axis.x == 0.0 && axis.z == 0.0 {
        return [Vec3::X, Vec3::Z];
    }
    let helper = if axis.y.abs() < 0.9 { Vec3::Y } else { Vec3::X };
    let e1 = axis.cross(helper).normalize();
    [e1, axis.cross(e1)]
}

// The corners sit so that four of the flat sides face the axes.
fn corner_angle(k: usize) -> f32 {
    (k as f32 + 0.5) * TAU / SIDES as f32
}

// A ring of `SIDES` corners round `centre`, in the plane across `axis`.
fn ring(centre: Vec3, axis: Vec3, radius: f32) -> [Vec3; SIDES] {
    let [e1, e2] = across(axis);
    std::array::from_fn(|k| {
        let (sin, cos) = libm::sincosf(corner_angle(k));
        centre + (e1 * cos + e2 * sin) * radius
    })
}

// A length of log: bark between two rings, and an end at each that has `caps`.
struct Segment {
    from: [Vec3; SIDES],
    to: [Vec3; SIDES],
    axis: Vec3,
    radius: f32,
    // where round the bark its pattern starts, so trees differ
    phase: f32,
    caps: [bool; 2],
}

struct Drawing<'a, L: Deref<Target = SkyLight>> {
    buffers: &'a mut MeshBuffers,
    grass_tint: &'a GrassTint,
    light: &'a L,
}

impl<L: Deref<Target = SkyLight>> Drawing<'_, L> {
    // One quad of bark or an end as top left, bottom left, top right, bottom right, wound as the cube's faces are.
    fn quad(
        &mut self,
        mut points: [Vec3; 4],
        mut uvs: [[f32; 2]; 4],
        outward: Vec3,
        tile: Tile,
        lit: f32,
    ) {
        if (points[1] - points[0])
            .cross(points[2] - points[0])
            .dot(outward)
            > 0.0
        {
            points.swap(1, 2);
            uvs.swap(1, 2);
        }
        let atlas_cell = tile.atlas_cell();
        let colors = points.map(|p| {
            self.grass_tint
                .weathered_at(BlockType::Log, p)
                .extend(atlas_cell)
                .to_array()
        });
        // the cube's faces along z store their normals reversed
        let normal = [outward.x, outward.y, -outward.z];
        self.buffers
            .push_tiled_quad(points.map(|p| p.to_array()), normal, uvs, colors, [lit; 4]);
    }

    fn segment(&mut self, segment: &Segment, lit: f32) {
        let Segment {
            from,
            to,
            axis,
            radius,
            phase,
            caps,
        } = segment;
        let origin = self.grass_tint.origin.as_vec3();
        let [e1, e2] = across(*axis);
        let long = *axis;
        let side = 2.0 * radius * libm::sinf(TAU / (2.0 * SIDES as f32));
        // painted bark takes its texture in blocks: round by arc length, along from the world's own coordinates
        let down = |p: Vec3| {
            if axis.x == 0.0 && axis.z == 0.0 {
                -(origin.y + p.y)
            } else {
                (origin + p).dot(*axis)
            }
        };
        let bark = BlockType::Log.tile(Face::Side);
        for k in 0..SIDES {
            let next = (k + 1) % SIDES;
            let (sin, cos) = libm::sincosf(corner_angle(k) + TAU / (2.0 * SIDES as f32));
            let (u0, u1) = (
                UNFOLDED + phase + k as f32 * side,
                UNFOLDED + phase + (k + 1) as f32 * side,
            );
            self.quad(
                [to[k], from[k], to[next], from[next]],
                [
                    [u0, down(to[k])],
                    [u0, down(from[k])],
                    [u1, down(to[next])],
                    [u1, down(from[next])],
                ],
                e1 * cos + e2 * sin,
                bark,
                lit,
            );
        }
        let end = BlockType::Log.tile(Face::Top);
        let flat = |p: Vec3| {
            let at = origin + p;
            let (x, y, z) = (axis.x.abs(), axis.y.abs(), axis.z.abs());
            if y >= x && y >= z {
                [UNFOLDED + at.x, at.z]
            } else if x >= z {
                [UNFOLDED + at.y, at.z]
            } else {
                [UNFOLDED + at.x, at.y]
            }
        };
        for (corners, outward, capped) in [(to, long, caps[1]), (from, -long, caps[0])] {
            if !capped {
                continue;
            }
            // the octagon as three quads from its first corner
            for [a, b, c, d] in [[0, 1, 2, 3], [0, 3, 4, 5], [0, 5, 6, 7]] {
                let points = [corners[a], corners[b], corners[d], corners[c]];
                self.quad(points, points.map(flat), outward, end, lit);
            }
        }
    }

    // The light of the open air next to the log at `coord`, whose own cell holds none.
    fn light_at(&self, coord: IVec3) -> f32 {
        let level = [
            IVec3::X,
            IVec3::NEG_X,
            IVec3::Y,
            IVec3::NEG_Y,
            IVec3::Z,
            IVec3::NEG_Z,
        ]
        .into_iter()
        .map(|d| self.light.level(coord + d))
        .max()
        .unwrap_or(0);
        face_light(level)
    }
}

/// Draws the logs of `faces` as eight-sided prisms: a tree's trunk bowed along its lean, any other log
/// straight along the logs beside it.
pub(super) fn mesh_trees<S: BlockSource>(
    faces: &TreeFaces<S>,
    buffers: &mut MeshBuffers,
    grass_tint: &GrassTint,
    light: &impl Deref<Target = SkyLight>,
) {
    let mut drawing = Drawing {
        buffers,
        grass_tint,
        light,
    };
    let origin = grass_tint.origin.as_vec3();
    for &coord in &faces.logs {
        let lit = drawing.light_at(coord);
        let centre = coord.as_vec3();
        let trunk = faces.tree_look(coord).filter(|look| look.in_trunk(coord));
        if let Some(look) = trunk {
            let radius = look.radius;
            let grounded = !faces.is_log(coord - IVec3::Y);
            let foot = if grounded {
                (radius * FLARE).min(FOOT_MAX).max(radius)
            } else {
                radius
            };
            let at = |y: f32| {
                let off = trunk_offset(&look, y);
                Vec3::new(centre.x + off.x, y, centre.z + off.y)
            };
            drawing.segment(
                &Segment {
                    from: ring(at(centre.y - 0.5), Vec3::Y, foot),
                    to: ring(at(centre.y + 0.5), Vec3::Y, radius),
                    axis: Vec3::Y,
                    radius,
                    phase: ((origin.x + look.base.x) * 0.618 + (origin.z + look.base.z) * 0.382)
                        * 4.0,
                    caps: [
                        grounded && !faces.block(coord - IVec3::Y).is_opaque(),
                        !faces.is_log(coord + IVec3::Y),
                    ],
                },
                lit,
            );
            continue;
        }
        // any other log reaches toward each log round it, diagonals too, so a branch that steps across the grid still joins up
        let radius = faces
            .tree_look(coord)
            .map_or(LONE_BRANCH_RADIUS, |look| look.radius * BRANCH);
        let joined: Vec<IVec3> = (-1..=1)
            .flat_map(|x| (-1..=1).flat_map(move |y| (-1..=1).map(move |z| IVec3::new(x, y, z))))
            .filter(|&offset| offset != IVec3::ZERO && faces.is_log(coord + offset))
            .collect();
        if joined.is_empty() {
            // a log with none beside it is a stub the width of its cell
            let half = Vec3::Y * 0.5;
            drawing.segment(
                &Segment {
                    from: ring(centre - half, Vec3::Y, radius),
                    to: ring(centre + half, Vec3::Y, radius),
                    axis: Vec3::Y,
                    radius,
                    phase: 0.0,
                    caps: [true, true],
                },
                lit,
            );
        }
        for &offset in &joined {
            let neighbour = coord + offset;
            // into a trunk it reaches the trunk's centre line, to be out of sight in it, and otherwise halfway, where the next log's own reach meets it
            let end = match faces.tree_look(neighbour).filter(|look| look.in_trunk(neighbour)) {
                Some(look) => {
                    let off = trunk_offset(&look, neighbour.y as f32);
                    neighbour.as_vec3() + Vec3::new(off.x, 0.0, off.y)
                }
                None => centre + offset.as_vec3() * 0.5,
            };
            let axis = (end - centre).normalize();
            drawing.segment(
                &Segment {
                    from: ring(centre, axis, radius),
                    to: ring(end, axis, radius),
                    axis,
                    radius,
                    phase: 0.0,
                    caps: [joined.len() == 1, false],
                },
                lit,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generate::trees::{Tree, TreeKind, TreeTints, MAX_LEAN};
    use bevy::math::I64Vec3;

    #[test]
    fn a_trunk_starts_straight_leans_a_little_more_each_block_and_never_past_the_cap() {
        for seed in 0..50 {
            let tree = Tree::new(
                TreeKind::Broadleaf,
                I64Vec3::new(8, 4, 8),
                10,
                3.0,
                0.7,
                seed,
            );
            let look = TreeTints::new([&tree], I64Vec3::ZERO, 1)
                .look_at(IVec3::new(8, 4, 8))
                .unwrap();
            assert_eq!(Vec2::ZERO, trunk_offset(&look, 3.5));
            let mut last = 0.0;
            for block in 0..=10 {
                let off = trunk_offset(&look, 3.5 + block as f32).length();
                assert!(
                    off >= last - 1e-6 && off <= MAX_LEAN + 1e-4,
                    "{off} at {block}"
                );
                last = off;
            }
        }
    }
}
