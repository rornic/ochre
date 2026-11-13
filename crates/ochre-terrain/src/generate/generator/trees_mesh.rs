use super::buffers::{GrassTint, MeshBuffers, UNFOLDED};
use super::mesh::{between, hash01};
use super::shading::face_light;
use super::source::{BlockSource, TreeFaces};
use crate::generate::light::SkyLight;
use crate::generate::trees::TreeLook;
use bevy::math::{IVec3, Vec2, Vec3};
use ochre_base::block::{BlockType, Face, Tile};
#[cfg(test)]
use std::f32::consts::SQRT_2;
use std::f32::consts::TAU;
use std::ops::Deref;

// How much wider than the trunk the lowest log is, where it meets the ground, at most this wide.
const FLARE: f32 = 1.25;
const FOOT_MAX: f32 = 0.55;
// A branch's radius against its tree's trunk, and a branch of no tree's.
const BRANCH: f32 = 0.7;
const LONE_BRANCH_RADIUS: f32 = 0.2;
const SIDES: usize = 8;
// A card's width across, in blocks, on a broadleaf and on a pine, whose tiers are narrow.
const CARD_SIZE: (f32, f32) = (1.6, 2.2);
const PINE_CARD_SIZE: (f32, f32) = (1.0, 1.5);
// How far a card stands out from its voxel's middle toward the open air, how much it tips up, how far its facing strays, and the share of surface leaves that get one.
const CARD_OUT: f32 = 0.25;
const CARD_UP: f32 = 0.35;
const CARD_STRAY: f32 = 0.4;
const CARD_DENSITY: f32 = 1.0;
/// The farthest a card's corner can be from its voxel's middle, in blocks: half the widest card's diagonal and how far out it stands.
#[cfg(test)]
pub(super) const CARD_REACH: f32 = CARD_SIZE.1 * 0.5 * SQRT_2 + CARD_OUT;

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

    // A card of leaves: tinted as the leaves are, lit by the air round the leaf, and shaded as the crown it is part of.
    fn card(&mut self, card: &Card, tint: Vec3, shade: impl Fn(Vec3) -> f32, lit: f32) {
        let leaves = BlockType::Leaves.tile(Face::Side);
        let atlas_cell = leaves.atlas_cell();
        let colors = card.corners.map(|p| {
            (self.grass_tint.at(p) * tint * shade(p))
                .extend(atlas_cell)
                .to_array()
        });
        // one texture repeat to a block
        let s = card.size;
        // the cube's faces along z store their normals reversed
        let normal = [card.shading.x, card.shading.y, -card.shading.z];
        self.buffers.push_double_sided_quad(
            card.corners.map(|p| p.to_array()),
            normal,
            [[0.0, 0.0], [0.0, s], [s, 0.0], [s, s]],
            colors,
            [lit; 4],
        );
    }

    // The light of the open air next to the block at `coord`, and its own, which a log's cell holds none of.
    fn light_at(&self, coord: IVec3) -> f32 {
        let level = [
            IVec3::ZERO,
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

/// A painted card on a surface leaf: its corners as top left, bottom left, top right, bottom right, how wide it is, and the way
/// the crown's light treats it, outward from the crown and not from the card itself.
pub(super) struct Card {
    pub(super) corners: [Vec3; 4],
    pub(super) size: f32,
    pub(super) shading: Vec3,
}

/// The card for the leaf whose middle is `middle`, at `world` in the world, open to the air along `open` (the sum of its open
/// sides), or none if this leaf gets no card.
pub(super) fn card(world: IVec3, middle: Vec3, open: Vec3, pine: bool) -> Option<Card> {
    if hash01(world, 90) >= CARD_DENSITY {
        return None;
    }
    let outward = (open.normalize_or_zero() + Vec3::Y * CARD_UP).normalize();
    let stray = Vec3::new(
        hash01(world, 91) - 0.5,
        hash01(world, 92) - 0.5,
        hash01(world, 93) - 0.5,
    ) * 2.0
        * CARD_STRAY;
    let facing = (outward + stray).normalize();
    let size = between(
        if pine { PINE_CARD_SIZE } else { CARD_SIZE },
        hash01(world, 94),
    );
    let [e1, _] = across(facing);
    let (sin, cos) = libm::sincosf(hash01(world, 95) * TAU);
    let right = e1 * cos + facing.cross(e1) * sin;
    let up = facing.cross(right);
    let (half, centre) = (size * 0.5, middle + outward * CARD_OUT);
    Some(Card {
        corners: [
            centre - right * half + up * half,
            centre - right * half - up * half,
            centre + right * half + up * half,
            centre + right * half - up * half,
        ],
        size,
        shading: outward,
    })
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
    }    for &coord in &faces.leaves {
        let open: Vec3 = [
            IVec3::X,
            IVec3::NEG_X,
            IVec3::Y,
            IVec3::NEG_Y,
            IVec3::Z,
            IVec3::NEG_Z,
        ]
        .into_iter()
        .filter(|&d| faces.is_open(coord + d))
        .map(|d| d.as_vec3())
        .sum();
        if open == Vec3::ZERO {
            continue;
        }
        let pine = faces.tree_look(coord).is_some_and(|look| look.pine);
        let world = coord + grass_tint.origin;
        let Some(card) = card(world, coord.as_vec3(), open, pine) else {
            continue;
        };
        let lit = drawing.light_at(coord);
        drawing.card(&card, faces.leaf_tint(coord), |p| faces.canopy_shade(p), lit);
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

    #[test]
    fn no_card_reaches_further_from_its_leaf_than_the_groups_overhang_allows() {
        let sides = [
            IVec3::X,
            IVec3::NEG_X,
            IVec3::Y,
            IVec3::NEG_Y,
            IVec3::Z,
            IVec3::NEG_Z,
        ];
        let mut farthest: f32 = 0.0;
        for i in 0..3000 {
            let world = IVec3::new(i * 7 - 4000, (i * 3) % 90, i * 13 - 9000);
            let open: Vec3 = sides
                .iter()
                .enumerate()
                .filter(|(k, _)| (i >> k) & 1 == 1)
                .map(|(_, d)| d.as_vec3())
                .sum();
            for pine in [false, true] {
                if let Some(card) = card(world, Vec3::ZERO, open, pine) {
                    for corner in card.corners {
                        farthest = farthest.max(corner.length());
                    }
                }
            }
        }
        assert!(farthest <= CARD_REACH + 1e-4, "{farthest}");
        // a corner is at most this far past the face of the voxel it hangs on, and the group's edge is half a block further in
        const { assert!(CARD_REACH - 0.5 <= crate::group::GROUP_OVERHANG) };
    }
}
