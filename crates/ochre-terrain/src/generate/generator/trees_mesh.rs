use super::buffers::{GrassTint, MeshBuffers, UNFOLDED};
use super::mesh::{between, hash01};
use super::shading::face_light;
use super::source::{BlockSource, BoxShape, TreeBox, TreeFaces};
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
const BRANCH: f32 = 0.4;
const BRANCH_MIN: f32 = 0.08;
const LONE_BRANCH_RADIUS: f32 = 0.12;
const SIDES: usize = 8;
// A card's width across, in blocks, on a broadleaf and on a pine, whose tiers are narrow.
const CARD_SIZE: (f32, f32) = (2.0, 2.8);
const PINE_CARD_SIZE: (f32, f32) = (2.0, 3.0);
// How far a card stands out from its voxel's middle toward the open air, how much it tips up, how far its facing strays, and the share of surface leaves that get one.
const CARD_OUT: f32 = 0.25;
const CARD_UP: f32 = 0.35;
const CARD_STRAY: f32 = 0.4;
const CARD_DENSITY: f32 = 1.0;
// A card's outline is this many corners at an irregular distance, in half widths, from its middle, so it has no square corners to stick out.
const RIM: usize = 6;
const RIM_REACH: (f32, f32) = (0.95, 1.3);
// A pine's cards face along the cone of its crown, out from the trunk and this much up, and not away from whichever side of their leaf is open.
const PINE_UP: f32 = 0.6;
// At a pine's tip, where there is no cone to face along, a card stands up facing a hashed way round, this much up, so it does not cap the tip flat.
const PINE_TIP_UP: f32 = 0.4;
// A broadleaf's cards take this share of their facing from the crown's ball, and the rest from their open sides, with this much up.
const CROWN_FACING: f32 = 0.7;
const CROWN_UP: f32 = 0.15;
// A card is darker the more leaves are over it in its column, by this much a leaf, as far as this many above, and at least this bright.
const DEPTH_REACH: i32 = 6;
const DEPTH_DARKEN: f32 = 0.07;
const DEPTH_MIN: f32 = 0.6;
/// The farthest a card's corner can be from its voxel's middle, in blocks: its widest card's longest reach and how far out it stands.
#[cfg(test)]
pub(super) const CARD_REACH: f32 = PINE_CARD_SIZE.1 * 1.1 * 0.5 * RIM_REACH.1 + CARD_OUT;

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
        let points: Vec<Vec3> = std::iter::once(card.centre)
            .chain(card.outline)
            .collect();
        let colors: Vec<[f32; 4]> = points
            .iter()
            .map(|&p| {
                (self.grass_tint.at(p) * tint * shade(p))
                    .extend(atlas_cell)
                    .to_array()
            })
            .collect();
        // one texture repeat to a block, across the card's own plane
        let uvs: Vec<[f32; 2]> = points
            .iter()
            .map(|&p| {
                let from = p - card.centre;
                [from.dot(card.right), from.dot(card.up)]
            })
            .collect();
        // the cube's faces along z store their normals reversed
        let normal = [card.shading.x, card.shading.y, -card.shading.z];
        let positions: Vec<[f32; 3]> = points.iter().map(|p| p.to_array()).collect();
        self.buffers
            .push_double_sided_fan(&positions, normal, &uvs, &colors, lit);
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

/// A painted card on a surface leaf: its middle and the directions across it, its outline, and the way
/// the crown's light treats it, outward from the crown and not from the card itself.
pub(super) struct Card {
    pub(super) centre: Vec3,
    pub(super) right: Vec3,
    pub(super) up: Vec3,
    pub(super) outline: [Vec3; RIM],
    pub(super) shading: Vec3,
}

/// The directions across a card facing `facing`, turned by `spin` of a full turn.
fn card_frame(facing: Vec3, spin: f32) -> (Vec3, Vec3) {
    let [e1, _] = across(facing);
    let (sin, cos) = libm::sincosf(spin * TAU);
    let right = e1 * cos + facing.cross(e1) * sin;
    (right, facing.cross(right))
}

/// A card's outline: `RIM` corners round `centre` at uneven distances, so it has no square corners.
fn card_outline(world: IVec3, centre: Vec3, right: Vec3, up: Vec3, half: f32) -> [Vec3; RIM] {
    std::array::from_fn(|k| {
        let angle = (k as f32 + hash01(world, 100 + k as u32) * 0.6) * TAU / RIM as f32;
        let (sin, cos) = libm::sincosf(angle);
        let reach = half * between(RIM_REACH, hash01(world, 120 + k as u32));
        centre + (right * cos + up * sin) * reach
    })
}

/// What a leaf's card faces by: along a pine's cone, from its offset from the trunk, or a broadleaf's crown, by the way the crown faces there.
#[derive(Clone, Copy)]
pub(super) enum Crown {
    Pine(Vec3),
    Broadleaf(Vec3),
}

/// How bright a card is with `above` leaves over it in its column.
pub(super) fn depth_shade(above: usize) -> f32 {
    (1.0 - DEPTH_DARKEN * above as f32).max(DEPTH_MIN)
}

/// The card for the leaf whose middle is `middle`, at `world` in the world, open to the air along `open` (the sum of its open
/// sides), or none if this leaf gets no card.
pub(super) fn card(world: IVec3, middle: Vec3, open: Vec3, crown: Crown, scale: f32) -> Option<Card> {
    if hash01(world, 90) >= CARD_DENSITY {
        return None;
    }
    let outward = match crown {
        Crown::Pine(from_trunk) => {
            let across = from_trunk.with_y(0.0);
            if across.length() < 0.5 {
                let (sin, cos) = libm::sincosf(hash01(world, 96) * TAU);
                (Vec3::new(cos, 0.0, sin) + Vec3::Y * PINE_TIP_UP).normalize()
            } else {
                (across.normalize() + Vec3::Y * PINE_UP).normalize()
            }
        }
        Crown::Broadleaf(ball) if ball != Vec3::ZERO => (ball * CROWN_FACING
            + open.normalize_or_zero() * (1.0 - CROWN_FACING)
            + Vec3::Y * CROWN_UP)
            .normalize(),
        Crown::Broadleaf(_) => (open.normalize_or_zero() + Vec3::Y * CARD_UP).normalize(),
    };
    let stray = Vec3::new(
        hash01(world, 91) - 0.5,
        hash01(world, 92) - 0.5,
        hash01(world, 93) - 0.5,
    ) * 2.0
        * CARD_STRAY;
    let facing = (outward + stray).normalize();
    let size = scale
        * between(
            if matches!(crown, Crown::Pine(_)) { PINE_CARD_SIZE } else { CARD_SIZE },
            hash01(world, 94),
        );
    let (right, up) = card_frame(facing, hash01(world, 95));
    let (half, centre) = (size * 0.5, middle + outward * CARD_OUT);
    let outline = card_outline(world, centre, right, up, half);
    Some(Card {
        centre,
        right,
        up,
        outline,
        shading: outward,
    })
}

// How many sides a far tree's shapes have, seen from too far for more to count.
const FAR_SIDES: usize = 8;
// A far crown's ball reaches this much past its box, so it fills about as much as the box did.
const FAR_BALL: f32 = 1.2;

#[derive(Clone, Copy)]
// One ring of a far shape: its middle, its radii across x and z, and which way its surface faces there, `out` sideways and `up`.
struct FarRing {
    centre: Vec3,
    radius: Vec2,
    out: f32,
    up: f32,
}

/// Draws `tree_box`, which is not a cuboid, as the shape it stands for: a trunk as a bowed prism, a pine's layer as a cone, a
/// broadleaf's crown as a ball, with smooth normals so the crown shades as a mass. `cell` is the mesh's blocks to a cell.
pub(super) fn far_shape(buffers: &mut MeshBuffers, tree_box: &TreeBox, tint: Vec3, cell: f32, light: f32) {
    let (min, max) = (tree_box.min, tree_box.max);
    let (centre, half) = ((min + max) / 2.0, (max - min) / 2.0);
    if let BoxShape::Card { facing } = tree_box.shape {
        // one of a far crown's few cards, in a fan like a near one, hashed by where it is
        let world = (centre * 16.0).as_ivec3();
        let (right, up) = card_frame(facing, hash01(world, 95));
        let outline = card_outline(world, centre, right, up, half.x);
        let colour = tint.extend(tree_box.block.tile(Face::Side).atlas_cell()).to_array();
        let mut vertices = vec![(centre, facing, [0.0, 0.0], colour)];
        for corner in outline {
            let from = corner - centre;
            vertices.push((corner, facing, [from.dot(right) * cell, from.dot(up) * cell], colour));
        }
        let triangles: Vec<[u32; 3]> = (0..RIM as u32)
            .map(|k| [0, 1 + k, 1 + (k + 1) % RIM as u32])
            .collect();
        buffers.push_double_sided_mesh(&vertices, light, &triangles);
        return;
    }
    let (rings, top, bottom): (Vec<FarRing>, Option<Vec3>, Option<Vec3>) = match tree_box.shape {
        BoxShape::Cuboid | BoxShape::Card { .. } => return,
        BoxShape::Trunk { lean, radius } => (
            [0.0f32, 0.5, 1.0]
                .map(|t| {
                    let off = lean * t * t;
                    FarRing {
                        centre: Vec3::new(centre.x + off.x, min.y + (max.y - min.y) * t, centre.z + off.y),
                        radius: Vec2::splat(radius * (1.2 - 0.4 * t)),
                        out: 1.0,
                        up: 0.0,
                    }
                })
                .to_vec(),
            Some(Vec3::new(centre.x + lean.x, max.y, centre.z + lean.y)),
            None,
        ),
        BoxShape::Ball => (
            [-45.0f32, 0.0, 45.0]
                .map(|degrees| {
                    let (sin, cos) = libm::sincosf(degrees.to_radians());
                    FarRing {
                        centre: Vec3::new(centre.x, centre.y + half.y * FAR_BALL * sin, centre.z),
                        radius: Vec2::new(half.x, half.z) * (FAR_BALL * cos),
                        out: cos,
                        up: sin,
                    }
                })
                .to_vec(),
            Some(Vec3::new(centre.x, centre.y + half.y * FAR_BALL, centre.z)),
            Some(Vec3::new(centre.x, centre.y - half.y * FAR_BALL, centre.z)),
        ),
        BoxShape::Cone => {
            let foot = Vec2::new(half.x, half.z) * 1.1;
            let tip = foot * (1.0 - tree_box.taper);
            // the side leans in by the narrowing over the height
            let slope = (foot.x - tip.x) / (max.y - min.y).max(1e-3);
            (
                vec![
                    FarRing { centre: Vec3::new(centre.x, min.y, centre.z), radius: foot, out: 1.0, up: slope },
                    FarRing { centre: Vec3::new(centre.x, max.y, centre.z), radius: tip, out: 1.0, up: slope },
                ],
                Some(Vec3::new(centre.x, max.y, centre.z)),
                Some(Vec3::new(centre.x, min.y, centre.z)),
            )
        }
    };
    let side = tree_box.block.tile(Face::Side).atlas_cell();
    let end = tree_box.block.tile(Face::Top).atlas_cell();
    let colour = |cell: f32| tint.extend(cell).to_array();
    let blocks = |units: f32| units * cell;
    let mut vertices = Vec::new();
    let mut triangles: Vec<[u32; 3]> = Vec::new();
    for ring in &rings {
        // painted textures take blocks: round by arc length, and up from the shape's foot
        let round = blocks(TAU * 0.5 * (ring.radius.x + ring.radius.y));
        for k in 0..=FAR_SIDES {
            let (sin, cos) = libm::sincosf(k as f32 / FAR_SIDES as f32 * TAU);
            let at = ring.centre + Vec3::new(cos * ring.radius.x, 0.0, sin * ring.radius.y);
            let normal = Vec3::new(cos * ring.out, ring.up, sin * ring.out).normalize_or_zero();
            vertices.push((
                at,
                normal,
                [k as f32 / FAR_SIDES as f32 * round, blocks(at.y - min.y)],
                colour(side),
            ));
        }
    }
    let across = FAR_SIDES as u32 + 1;
    for i in 0..rings.len() as u32 - 1 {
        for k in 0..FAR_SIDES as u32 {
            let (a, c) = (i * across + k, (i + 1) * across + k);
            triangles.push([a, c, a + 1]);
            triangles.push([a + 1, c, c + 1]);
        }
    }
    // a cap is a fan from a pole to the ring beside it
    for (pole, ring, up) in [
        (top, rings.len() as u32 - 1, 1.0),
        (bottom, 0, -1.0),
    ] {
        let Some(pole) = pole else { continue };
        let hub = vertices.len() as u32;
        vertices.push((pole, Vec3::Y * up, [0.0, blocks(pole.y - min.y)], colour(end)));
        for k in 0..FAR_SIDES as u32 {
            triangles.push([hub, ring * across + k, ring * across + k + 1]);
        }
    }
    buffers.push_double_sided_mesh(&vertices, light, &triangles);
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
            // a pine's boughs start in its trunk, each a straight prism out to its tip, however the voxels step
            let thin = (look.radius * BRANCH).max(BRANCH_MIN);
            for along in look.boughs_from(coord) {
                let start = at(centre.y);
                let axis = along.normalize();
                drawing.segment(
                    &Segment {
                        from: ring(start, axis, thin),
                        to: ring(start + along, axis, thin * 0.5),
                        axis,
                        radius: thin,
                        phase: 0.0,
                        caps: [false, true],
                    },
                    lit,
                );
            }
            continue;
        }
        // a pine's boughs are drawn from its trunk, not from the logs that fill in their line
        if faces.tree_look(coord).is_some_and(|look| look.pine) {
            continue;
        }
        // any other log reaches toward each log round it, diagonals too, so a branch that steps across the grid still joins up
        let radius = faces
            .tree_look(coord)
            .map_or(LONE_BRANCH_RADIUS, |look| (look.radius * BRANCH).max(BRANCH_MIN));
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
        // a pine's card faces along its crown's cone, from its offset from the trunk, and a broadleaf's as its crown's ball does there
        let at = coord.as_vec3();
        let crown = match faces.tree_look(coord) {
            Some(look) if look.pine => Crown::Pine(at - look.base),
            Some(look) => Crown::Broadleaf(look.crown_normal(at)),
            None => Crown::Broadleaf(Vec3::ZERO),
        };
        let world = coord + grass_tint.origin;
        let scale = faces.tree_look(coord).map_or(1.0, |look| look.card_scale);
        let Some(card) = card(world, at, open, crown, scale) else {
            continue;
        };
        let lit = drawing.light_at(coord);
        // darker the more leaves are over it, so the lower and inner crown is in the shade of the upper
        let above = (1..=DEPTH_REACH)
            .filter(|&up| faces.is_leaf(coord + IVec3::Y * up))
            .count();
        let depth = depth_shade(above);
        drawing.card(
            &card,
            faces.leaf_tint(coord),
            |p| faces.canopy_shade(p) * depth,
            lit,
        );
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
            for crown in [
                Crown::Broadleaf(Vec3::ZERO),
                Crown::Broadleaf(Vec3::new(0.6, 0.8, 0.0)),
                Crown::Pine(Vec3::new(2.0, 5.0, -1.0)),
            ] {
                if let Some(card) = card(world, Vec3::ZERO, open, crown, 1.1) {
                    for corner in card.outline {
                        farthest = farthest.max(corner.length());
                    }
                }
            }
        }
        assert!(farthest <= CARD_REACH + 1e-4, "{farthest}");
        // a corner is at most this far past the face of the voxel it hangs on, and the group's edge is half a block further in
        const { assert!(CARD_REACH - 0.5 <= crate::group::GROUP_OVERHANG) };
    }

    #[test]
    fn a_pines_tip_is_not_capped_by_a_flat_card() {
        for i in 0..400 {
            let world = IVec3::new(i * 11, 60, i * 5 - 700);
            // on the trunk's axis at the very top, where a cone has no side to face
            let card = card(world, Vec3::ZERO, Vec3::Y, Crown::Pine(Vec3::new(0.0, 16.0, 0.0)), 1.0).unwrap();
            let facing = card.right.cross(card.up).normalize();
            assert!(facing.y.abs() < 0.9, "card faces {facing}");
            assert!(card.shading.y < 0.5, "shaded as facing {}", card.shading);
        }
    }

    #[test]
    fn a_broadleafs_cards_face_out_of_the_crown_and_the_lower_ones_are_darker() {
        let tree = Tree::new(TreeKind::Broadleaf, I64Vec3::new(8, 4, 8), 10, 4.0, 0.7, 5);
        let look = TreeTints::new([&tree], I64Vec3::ZERO, 1)
            .look_at(IVec3::new(8, 4, 8))
            .unwrap();
        // the crown's middle is a block under the trunk's top: above it faces up, and to the side faces out
        let top = Vec3::new(8.0, 4.0 + 9.0 + 3.0, 8.0);
        assert!(look.crown_normal(top).y > 0.9);
        let side = look.crown_normal(Vec3::new(8.0 + 4.0, 4.0 + 9.0, 8.0));
        assert!(side.x > 0.9 && side.y.abs() < 0.2, "{side}");
        assert!(depth_shade(0) == 1.0 && depth_shade(3) < depth_shade(1));
        assert!(depth_shade(50) >= DEPTH_MIN);
    }
}
