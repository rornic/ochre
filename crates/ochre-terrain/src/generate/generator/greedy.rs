use super::buffers::Vertex;
use super::buffers::{
    bevelled_ij, blend_corners, lift_at, unfolded_uv, FanQuad, GrassTint, InnerLeaves, MeshBuffers,
    MeshStyle, BEVEL, RAMP,
};

use super::lod::MAX_SKIRT_CELLS;
use super::shading::{
    axis_of, face_light, vertex_ao, vertex_light, ao_gamma, FACE_NORMAL_OFFSET, FACE_TANGENTS,
};
use super::source::{BlockSource, GridView};
use super::terrain::is_solid;
use crate::chunk::CHUNK_SIZE;
use crate::generate::light::SkyLight;
use crate::generate::light::MAX_LIGHT;
use bevy::math::IVec3;
use bevy::math::Vec3;
use ochre_base::block::BlockType;
use ochre_base::block::Face;
use std::cell::RefCell;
use std::ops::Deref;

// Which tangent (0 or 1) UV component `k` runs along on this face, so a merged quad can repeat it.
pub(super) fn uv_tangent(face: &[Vertex], k: usize, a_axis: usize) -> usize {
    let high = |v: &Vertex| v.position[a_axis] > 0.0;
    let follows_a = face
        .iter()
        .all(|v| (high(v) == high(&face[0])) == (v.uv[k] == face[0].uv[k]));
    if follows_a {
        0
    } else {
        1
    }
}

// What a side of a face is: flat, or an edge a bevel cuts back, which is convex where the surface
// turns away into open air, or concave where it meets a wall and a cove fills the crease.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub(super) enum Edge {
    Flat,
    Convex,
    Concave,
}

#[derive(Copy, Clone)]
pub(super) struct FaceCell {
    pub(super) block: BlockType,
    // the tree's own tint over the grass's, for leaves, see `Tree::tint`
    pub(super) tint: Vec3,
    pub(super) ao: [f32; 4],
    // sky light at each corner, 0..1
    pub(super) light: [f32; 4],
    // where the face starts and ends up the mesh, if not the whole cell, see `face_span`
    pub(super) span: Option<[f32; 2]>,
    // a face between leaves, see `InnerLeaves`
    pub(super) inner: bool,
    // what each of its sides, -ta, +ta, -tb, +tb, is, see `Edge`
    pub(super) bevel: [Edge; 4],
    // which corners, (-ta, -tb), (+ta, -tb), (-ta, +tb), (+ta, +tb), the floor rises at, see `corner_lifts`
    pub(super) corner: [bool; 4],
    // which corners the top is cut down at, between the strips of the two blocks beside it, see `corner_dips`
    pub(super) dip: [bool; 4],
}

impl FaceCell {
    // Whether its raised corners are the same at both its ends along `axis`, 0 for its first tangent and 1 for its
    // second, so a run of cells like it is raised the same all along.
    fn corner_even_along(&self, axis: usize) -> bool {
        let c = self.corner;
        if axis == 0 {
            c[0] == c[1] && c[2] == c[3]
        } else {
            c[0] == c[2] && c[1] == c[3]
        }
    }

    // Whether its shading is the same at both its ends along `axis` of the face, 0 for its
    // first tangent and 1 for its second, with `corners` where each corner is along them.
    pub(super) fn even_along(&self, corners: &[[bool; 2]; 4], axis: usize) -> bool {
        (0..4).all(|j| {
            (0..4).all(|k| {
                corners[j][1 - axis] != corners[k][1 - axis]
                    || (self.ao[j] == self.ao[k] && self.light[j] == self.light[k])
            })
        })
    }

    // Whether `other`, next to it along `axis`, merges into one quad with it: shaded just the
    // same, and evenly along `axis`, so a gradient across the run is stretched only along
    // where it doesn't change, which leaves the shading as it was.
    pub(super) fn merges_along(
        &self,
        other: &FaceCell,
        corners: &[[bool; 2]; 4],
        axis: usize,
    ) -> bool {
        self.block == other.block
            && self.tint == other.tint
            && self.ao == other.ao
            && self.light == other.light
            && self.span == other.span
            && self.inner == other.inner
            && self.bevel == other.bevel
            // a raised corner is on one cell's own, not stretched across a run
            // a raised corner is shared with the cell along, so they merge only where it's the same at both
            && self.corner == other.corner
            && self.corner_even_along(axis)
            && self.dip == other.dip
            && !self.dip.contains(&true)
            && self.even_along(corners, axis)
    }
}

// Which sides of a face of the block at `coord`, -ta, +ta, -tb, +tb, turn away into open air, where
// the surface has a convex edge to bevel. A side next to a block is a seam or an inner
// corner, and a bare side with a block over it is a notch, not an edge.
pub(super) fn convex_sides(
    opaque: impl Fn(IVec3) -> bool,
    coord: IVec3,
    normal: IVec3,
    ta: IVec3,
    tb: IVec3,
) -> [bool; 4] {
    [-ta, ta, -tb, tb].map(|side| !opaque(coord + side) && !opaque(coord + side + normal))
}

// Blocks whose convex edges are cut back: leaves are boxes of holes.
pub(super) fn cuts(block: BlockType) -> bool {
    block != BlockType::Leaves
}

// Blocks that meet the ground in a cove, which grass does as well: nothing covers a crease.
fn coves(block: BlockType) -> bool {
    block != BlockType::Leaves
}

// What each side of the top face of the block at `coord` is, see `Edge`. A cove needs both blocks
// to be cut back, or the wall's raised foot would leave a gap against a floor that wasn't.
pub(super) fn top_edges(
    source: &impl BlockSource,
    coord: IVec3,
    normal: IVec3,
    ta: IVec3,
    tb: IVec3,
) -> [Edge; 4] {
    let own = source.block(coord);
    if !coves(own) {
        return [Edge::Flat; 4];
    }
    let opaque = |c: IVec3| source.block(c).is_opaque();
    let convex = if cuts(own) {
        convex_sides(opaque, coord, normal, ta, tb)
    } else {
        [false; 4]
    };
    let sides = [-ta, ta, -tb, tb];
    std::array::from_fn(|k| {
        let wall = coord + sides[k];
        if convex[k] {
            Edge::Convex
        } else if opaque(wall) && opaque(wall + normal) && source.block(wall + normal) == own {
            Edge::Concave
        } else {
            Edge::Flat
        }
    })
}

// Which corners of the top face of the block at `coord`, (-ta, -tb), (+ta, -tb), (-ta, +tb), (+ta, +tb), are
// the outside corner of a wall, with ground of the same block on both sides of it: there the ramps either
// side meet at a point, and the floor rises there so they round the corner.
pub(super) fn corner_lifts(
    source: &impl BlockSource,
    coord: IVec3,
    normal: IVec3,
    ta: IVec3,
    tb: IVec3,
) -> [bool; 4] {
    let own = source.block(coord);
    if !coves(own) {
        return [false; 4];
    }
    let opaque = |c: IVec3| source.block(c).is_opaque();
    let floor = |c: IVec3| opaque(c) && !opaque(c + normal) && source.block(c) == own;
    let sides = [-ta, ta, -tb, tb];
    std::array::from_fn(|k| {
        let (a, b) = (sides[k & 1], sides[2 + (k >> 1)]);
        let diagonal = coord + a + b;
        floor(coord + a)
            && floor(coord + b)
            && opaque(diagonal)
            && opaque(diagonal + normal)
            && source.block(diagonal + normal) == own
    })
}

// How a top face is cut where it meets its neighbours: its sides, and which corners it rises at and is cut down at,
// see `top_edges`, `Shapes::shape` and `corner_dips`.
#[derive(Clone, Copy)]
pub(super) struct Shape {
    pub(super) edges: [Edge; 4],
    pub(super) raised: [bool; 4],
    pub(super) dipped: [bool; 4],
}

#[derive(Clone, Copy)]
struct Base {
    edges: [Edge; 4],
    outside: [bool; 4],
    dipped: [bool; 4],
}

// The shapes of the tops in and just round a chunk, worked out once each however many faces and seams ask: the
// rules look at the blocks round a top, and its neighbours' rules look at theirs.
pub(super) struct Shapes {
    base: RefCell<Vec<Option<Base>>>,
    full: RefCell<Vec<Option<Shape>>>,
}

// Tops are all tangent to x and z, up y.
const TOP: (IVec3, IVec3, IVec3) = (IVec3::Y, IVec3::X, IVec3::Z);

impl Shapes {
    const SPAN: i32 = CHUNK_SIZE as i32 + 2;

    // Only bevelled meshes look at shapes, so the others keep none.
    pub(super) fn new(bevelled: bool) -> Self {
        let cells = if bevelled {
            (Self::SPAN * Self::SPAN * Self::SPAN) as usize
        } else {
            0
        };
        Self {
            base: RefCell::new(vec![None; cells]),
            full: RefCell::new(vec![None; cells]),
        }
    }

    // Where `coord` is kept, if it's in or just round the chunk.
    fn slot(coord: IVec3) -> Option<usize> {
        let at = coord + IVec3::ONE;
        (at.min_element() >= 0 && at.max_element() < Self::SPAN)
            .then(|| ((at.x * Self::SPAN + at.y) * Self::SPAN + at.z) as usize)
    }

    fn base(&self, source: &impl BlockSource, coord: IVec3) -> Base {
        let slot = Self::slot(coord);
        if let Some(base) = slot.and_then(|i| self.base.borrow().get(i).copied().flatten()) {
            return base;
        }
        let (normal, ta, tb) = TOP;
        let base = Base {
            edges: top_edges(source, coord, normal, ta, tb),
            outside: corner_lifts(source, coord, normal, ta, tb),
            dipped: corner_dips(source, coord, normal, ta, tb),
        };
        if let Some(i) = slot.filter(|&i| i < self.base.borrow().len()) {
            self.base.borrow_mut()[i] = Some(base);
        }
        base
    }

    // The shape of the top of the block at `coord`. It rises at the corners of the ground that climbs a wall beside
    // it, or round the outside corner of one: a ramp runs to the end of its wall, and past it only where the wall
    // goes on or the ground turns away, as at a chamfer or a second wall; where the ground goes on level with no
    // wall, the ramp comes down to meet it, and the neighbour agrees.
    pub(super) fn shape(&self, source: &impl BlockSource, coord: IVec3) -> Shape {
        let slot = Self::slot(coord);
        if let Some(shape) = slot.and_then(|i| self.full.borrow().get(i).copied().flatten()) {
            return shape;
        }
        let base = self.base(source, coord);
        let (normal, ta, tb) = TOP;
        let sides = [-ta, ta, -tb, tb];
        let opaque = |c: IVec3| source.block(c).is_opaque();
        // whether a ramp along side `wall` goes on past corner `k` at the end of it toward side `other`
        let goes_on = |k: usize, wall: usize, other: usize| match base.edges[other] {
            Edge::Convex | Edge::Concave => true,
            Edge::Flat => {
                let level = coord + sides[other];
                // the corner as the ground beside has it: across the tangent `other` is along
                let shared = if other < 2 { k ^ 1 } else { k ^ 2 };
                opaque(level) && !opaque(level + normal) && {
                    let beside = self.base(source, level);
                    beside.edges[wall] == Edge::Concave || beside.outside[shared]
                }
            }
        };
        let raised = if coves(source.block(coord)) {
            std::array::from_fn(|k| {
                let (a, b) = (k & 1, 2 + (k >> 1));
                base.outside[k]
                    || (base.edges[a] == Edge::Concave && goes_on(k, a, b))
                    || (base.edges[b] == Edge::Concave && goes_on(k, b, a))
            })
        } else {
            [false; 4]
        };
        // ground between walls on opposite sides is a trench a block wide, where ramps from both sides would only lift the whole floor
        let walled = |k: usize| base.edges[k] == Edge::Concave;
        let raised = if (walled(0) && walled(1)) || (walled(2) && walled(3)) {
            [false; 4]
        } else {
            raised
        };
        let shape = Shape {
            edges: base.edges,
            raised,
            dipped: base.dipped,
        };
        if let Some(i) = slot.filter(|&i| i < self.full.borrow().len()) {
            self.full.borrow_mut()[i] = Some(shape);
        }
        shape
    }
}

// Which ends of the face of the block at `coord` facing `normal` along a wall, -ta then +ta, its raised foot
// runs to the end of, as `ramp_corners` has the ground do: past an end only where the wall goes on or the ground
// turns away.
pub(super) fn wall_foot(
    source: &impl BlockSource,
    coord: IVec3,
    normal: IVec3,
    ta: IVec3,
) -> [bool; 2] {
    let own = source.block(coord);
    if !coves(own) {
        return [false; 2];
    }
    let opaque = |c: IVec3| source.block(c).is_opaque();
    let ground = coord - IVec3::Y + normal;
    if !(opaque(coord - IVec3::Y) && opaque(ground) && source.block(ground) == own) {
        return [false; 2];
    }
    // ground in a trench a block wide isn't raised, see `Shapes::shape`
    let walled = |side: IVec3| {
        opaque(ground + side)
            && opaque(ground + side + IVec3::Y)
            && source.block(ground + side + IVec3::Y) == own
    };
    if walled(normal) || (walled(ta) && walled(-ta)) {
        return [false; 2];
    }
    [-ta, ta].map(|side| {
        let beside = ground + side;
        if !opaque(beside) {
            // the ground falls away, with a chamfer along its edge
            !opaque(beside + IVec3::Y)
        } else if opaque(beside + IVec3::Y) {
            // a second wall
            source.block(beside + IVec3::Y) == own
        } else {
            // level ground, which the foot goes on along only where the wall does, or the ground rounds the
            // outside corner of it
            let next = coord + side;
            let under = opaque(next - IVec3::Y);
            let same = source.block(beside);
            under
                && if opaque(next) {
                    source.block(next) == same
                } else {
                    source.block(next - IVec3::Y) == same
                        && source.block(coord) == same
                        && source.block(ground) == same
                }
        }
    })
}

// Which corners of the top face of the block at `coord`, (-ta, -tb), (+ta, -tb), (-ta, +tb), (+ta, +tb), are
// the inside corner of a plus: level blocks on both sides of it and open air between them, so each has a
// chamfer along its edge to the air that ends at this block's corner. The corner is cut down to where the
// two chamfers meet, which joins them.
pub(super) fn corner_dips(
    source: &impl BlockSource,
    coord: IVec3,
    normal: IVec3,
    ta: IVec3,
    tb: IVec3,
) -> [bool; 4] {
    if !cuts(source.block(coord)) {
        return [false; 4];
    }
    let opaque = |c: IVec3| source.block(c).is_opaque();
    // a block level with this one, with a chamfer of its own to cut
    let level = |c: IVec3| opaque(c) && !opaque(c + normal) && cuts(source.block(c));
    let sides = [-ta, ta, -tb, tb];
    std::array::from_fn(|k| {
        let (a, b) = (sides[k & 1], sides[2 + (k >> 1)]);
        let diagonal = coord + a + b;
        level(coord + a) && level(coord + b) && !opaque(diagonal) && !opaque(diagonal + normal)
    })
}

// Which corners of the face of the block at `coord` facing `normal` along a wall, (-ta, -tb), (+ta, -tb), are at
// the end of a raised foot, where the ground in front of it has a chamfer along its edge to the air past the
// wall's end. The corner is let down to where that chamfer's foot is, so the wall covers the gap the chamfer's
// end leaves under the foot.
pub(super) fn wall_dips(
    source: &impl BlockSource,
    coord: IVec3,
    normal: IVec3,
    ta: IVec3,
) -> [bool; 4] {
    let own = source.block(coord);
    if !coves(own) || !cuts(own) {
        return [false; 4];
    }
    let opaque = |c: IVec3| source.block(c).is_opaque();
    let ground = coord - IVec3::Y + normal;
    // the foot is raised only against ground of the same block, see `wall_edges`
    if !(opaque(coord - IVec3::Y) && opaque(ground) && source.block(ground) == own) {
        return [false; 4];
    }
    let chamfered = |side: IVec3| !opaque(ground + side) && !opaque(ground + side + IVec3::Y);
    [chamfered(-ta), chamfered(ta), false, false]
}

// What each side of the face of the block at `coord` facing `normal` along a wall is: its top turns
// away over the edge, and its foot meets the ground in a crease, with the sides left flat.
pub(super) fn wall_edges(source: &impl BlockSource, coord: IVec3, normal: IVec3) -> [Edge; 4] {
    let own = source.block(coord);
    if !coves(own) {
        return [Edge::Flat; 4];
    }
    let opaque = |c: IVec3| source.block(c).is_opaque();
    let top = if cuts(own) && !opaque(coord + IVec3::Y) && !opaque(coord + IVec3::Y + normal) {
        Edge::Convex
    } else {
        Edge::Flat
    };
    let ground = coord - IVec3::Y + normal;
    let foot = if opaque(coord - IVec3::Y) && opaque(ground) && source.block(ground) == own {
        Edge::Concave
    } else {
        Edge::Flat
    };
    [Edge::Flat, Edge::Flat, foot, top]
}

// Whether a face of `block` at `coord` shows, and if so how much of it up the mesh's y axis,
// with LOD surface cells topped out at the ground's real height (see `LodBlocks::surface_top`):
// `Some(None)` for the whole face. Faces show through leaves' holes, other leaves' too, see
// `InnerLeaves`.
pub(super) fn face_span(
    source: &impl BlockSource,
    coord: IVec3,
    normal: IVec3,
    block: BlockType,
) -> Option<Option<[f32; 2]>> {
    // flora draws as crossed quads of its own, see `mesh_flora`
    if matches!(block, BlockType::Air | BlockType::Water) || block.is_flora() {
        return None;
    }
    let neighbour = coord + normal;
    let neighbour_block = source.block(neighbour);
    let covered = neighbour_block.is_opaque()
        || (block == BlockType::Ice && neighbour_block == BlockType::Ice);
    // every face is asked about, and most are hidden, so those leave before any lookups
    if !source.has_surface_tops() || normal.y < 0 {
        return (!covered).then_some(None);
    }
    if normal.y > 0 {
        // a lowered top still faces the air
        return (!covered).then(|| source.surface_top(coord).map(|t| [t, t]));
    }
    let bottom = coord.y as f32 - 0.5;
    // a lowered neighbour leaves this side bare above the ground's real height
    let floor = if covered {
        source.surface_top(neighbour)?
    } else {
        bottom
    };
    let top = source.surface_top(coord);
    let ceiling = top.unwrap_or(bottom + 1.0);
    if ceiling <= floor {
        return None;
    }
    Some((top.is_some() || covered).then_some([floor, ceiling]))
}

#[cfg(test)]
thread_local! {
    // lets tests mesh face by face, to check merging leaves the shading as it was
    pub(super) static NO_MERGING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    // lets tests of merging mesh without the bevel flags that split quads along edges
    pub(super) static NO_BEVELS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

// Merges runs of identically shaded opaque faces into single quads, one face direction slice at a
// time, where the shading doesn't change along the run, see `FaceCell::merges_along`.
// Faces between leaves go to `inner`, or nowhere, see `InnerLeaves`.
#[allow(clippy::too_many_arguments)]
pub(super) fn mesh_opaque_greedy(
    source: &GridView<impl BlockSource>,
    faces: &[[Vertex; 4]; 6],
    buffers: &mut MeshBuffers,
    inner: &mut MeshBuffers,
    style: MeshStyle,
    // how many times the texture repeats across a cell
    uv_tiles: f32,
    grass_tint: &GrassTint,
    light: &impl Deref<Target = SkyLight>,
    shapes: &Shapes,
) {
    let MeshStyle {
        ambient_occlusion,
        textured,
        inner_leaves,
        bevel,
    } = style;
    let size = CHUNK_SIZE as i32;
    let n = size as usize;
    let mut mask: Vec<Option<FaceCell>> = vec![None; n * n];
    // how many cells of each slice along each axis hold a block, and how many hold an opaque one
    let (mut held, mut opaque) = (vec![[0usize; 3]; n], vec![[0usize; 3]; n]);
    for x in 0..n {
        for y in 0..n {
            for z in 0..n {
                let block = source.blocks[(x * n + y) * n + z];
                let is_opaque = block.is_opaque() as usize;
                let is_held = !matches!(block, BlockType::Air | BlockType::Water) as usize;
                for (axis, at) in [x, y, z].into_iter().enumerate() {
                    held[at][axis] += is_held;
                    opaque[at][axis] += is_opaque;
                }
            }
        }
    }

    for (i, template) in faces.iter().enumerate() {
        let normal = FACE_NORMAL_OFFSET[i];
        let (ta, tb) = FACE_TANGENTS[i];
        let (a_axis, b_axis) = (axis_of(ta), axis_of(tb));
        let uv_axes = [0, 1].map(|k| {
            if uv_tangent(template, k, a_axis) == 0 {
                a_axis
            } else {
                b_axis
            }
        });
        let uv_ascending = [0, 1].map(|k| {
            template
                .iter()
                .find(|v| v.position[uv_axes[k]] > 0.0)
                .is_some_and(|v| v.uv[k] > 0.5)
        });
        let normal_axis = axis_of(normal);
        let normal_unit = normal.abs();
        let (p0, p1, p2) = (
            Vec3::from(template[0].position),
            Vec3::from(template[1].position),
            Vec3::from(template[2].position),
        );
        let ccw_in_ab = (p1 - p0)
            .cross(p2 - p0)
            .dot(ta.as_vec3().cross(tb.as_vec3()))
            > 0.0;
        // whether each corner is at the far end along each tangent
        let corners: [[bool; 2]; 4] = std::array::from_fn(|j| {
            [
                template[j].position[a_axis] > 0.0,
                template[j].position[b_axis] > 0.0,
            ]
        });
        let by_corner = |values: [f32; 4]| {
            let mut out = [[0.0; 2]; 2];
            for (j, [far_a, far_b]) in corners.iter().enumerate() {
                out[*far_a as usize][*far_b as usize] = values[j];
            }
            out
        };

        // the chunk's blocks are a dense grid, see `grid_index`: how far each axis steps in it
        let strides = [size * size, size, 1];
        let (step_a, step_b, step_n) = (strides[a_axis], strides[b_axis], strides[normal_axis]);
        let toward = normal[normal_axis];
        // what `face_span` says of a block under a block that hides it
        let hidden_is_final = normal.y != 0 || !source.has_surface_tops();

        // the face of `block` at `coord` as a cell to merge and draw, if it shows
        let cell_at = |coord: IVec3, block: BlockType| -> Option<FaceCell> {
            // past the mesh's edge the terrain may be drawn at another detail, with no
            // leaves just there, so leaves on the edge keep their faces
            let inside = source.contains(coord + normal);
            let between_leaves = inside
                && block == BlockType::Leaves
                && source.block(coord + normal) == BlockType::Leaves;
            if between_leaves && inner_leaves == InnerLeaves::Hidden {
                return None;
            }
            let span = face_span(source, coord, normal, block)?;
            // unshaded leaves merge into big quads, which keeps the faces between them cheap,
            // so they and coarse LODs take one light level a face instead of smooth light
            let smooth = ambient_occlusion && block != BlockType::Leaves;
            // a step above lowered ground faces into that ground's cell, which is dark: it's
            // lit by the air over it, and is too thin to shade smoothly
            let step = normal.y == 0 && source.block(coord + normal).is_opaque();
            // A skirt wall hangs from the node's own ground over solid cells beyond, whose
            // occlusion and light are none, so it is lit as that ground is, by the open air above.
            let skirt = !inside && normal.y == 0 && source.has_skirts();
            let smooth = smooth && !step && !skirt;
            let lit_from = if step {
                coord + normal + IVec3::Y
            } else {
                coord + normal
            };
            let skirt_light = skirt.then(|| {
                let air = (0..=MAX_SKIRT_CELLS + 1)
                    .find(|&dy| !is_solid(source.terrain(coord + IVec3::Y * dy)));
                face_light(air.map_or(MAX_LIGHT, |dy| light.level(coord + IVec3::Y * dy)))
            });
            // a trimmed face and a skirt's wall have no edges to bevel
            let bevelled = bevel && span.is_none() && !skirt;
            Some(FaceCell {
                span,
                inner: between_leaves,
                corner: match (bevelled, normal.y) {
                    (true, 1) => shapes.shape(source, coord).raised,
                    (true, 0) => {
                        let [near, far] = wall_foot(source, coord, normal, ta);
                        [near, far, false, false]
                    }
                    _ => [false; 4],
                },
                dip: match (bevelled, normal.y) {
                    (true, 1) => shapes.shape(source, coord).dipped,
                    (true, 0) => wall_dips(source, coord, normal, ta),
                    _ => [false; 4],
                },
                // only the edges round tops are bevelled so far: a top's four sides, and a wall's
                // top and foot
                bevel: match (bevelled, normal.y) {
                    (true, 1) => shapes.shape(source, coord).edges,
                    (true, 0) => wall_edges(source, coord, normal),
                    _ => [Edge::Flat; 4],
                },
                block,
                tint: if block == BlockType::Leaves {
                    source.leaf_tint(coord)
                } else {
                    Vec3::ONE
                },
                ao: if smooth {
                    std::array::from_fn(|j| vertex_ao(source, coord, i, template[j].position))
                } else {
                    [1.0; 4]
                },
                light: if smooth {
                    std::array::from_fn(|j| {
                        vertex_light(source, light, coord, i, template[j].position)
                    })
                } else {
                    [skirt_light.unwrap_or_else(|| face_light(light.level(lit_from))); 4]
                },
            })
        };

        // draws `cell`, `w` by `h` cells from the one at `at`, into `buffers`
        let emit = |cell: &FaceCell,
                    (w, h): (usize, usize),
                    at: IVec3,
                    buffers: &mut MeshBuffers| {
            let origin = at.as_vec3();
            let tile = if textured {
                cell.block.tile(Face::of_normal_y(normal.y as f32))
            } else {
                cell.block.lod_tile()
            };
            let atlas_cell = tile.atlas_cell();
            let painted = textured && tile.material_layer().is_some();
            let color = |ao: f32, position: Vec3| {
                // leaves share grass's tint, over a darker texture
                let tint = if tile.tinted() {
                    grass_tint.at(position) * cell.tint
                } else {
                    Vec3::ONE
                };
                let shade = match cell.block {
                    BlockType::Leaves => source.canopy_shade(position),
                    BlockType::Grass => source.forest_shade(position),
                    _ => 1.0,
                };
                (tint * ao.powf(ao_gamma()) * shade)
                    .extend(atlas_cell)
                    .to_array()
            };
            // a convex edge is cut back, a wall's foot is raised, and a floor's side by a wall climbs it
            let inset = cell.bevel.map(|e| match e {
                Edge::Convex => BEVEL,
                _ => 0.0,
            });
            let corner_lift = cell.corner.map(|c| if c { RAMP } else { 0.0 });
            // a wall's foot corner is let down off its raised foot to the chamfer's foot
            let dip = cell.dip.map(|d| match (d, normal.y) {
                (false, _) => 0.0,
                (true, 0) => RAMP + BEVEL,
                (true, _) => BEVEL,
            });
            let bevelled = cell.bevel.iter().any(|&e| e != Edge::Flat)
                || cell.corner.contains(&true)
                || cell.dip.contains(&true);
            if w == 1 && h == 1 && !bevelled {
                let positions: [Vec3; 4] = std::array::from_fn(|j| {
                    let mut position = Vec3::from(template[j].position) + origin;
                    if let Some([floor, ceiling]) = cell.span {
                        position.y = if template[j].position[1] > 0.0 {
                            ceiling
                        } else {
                            floor
                        };
                    }
                    position
                });
                let uvs = std::array::from_fn(|j| template[j].uv.map(|c| c * uv_tiles));
                let colors = std::array::from_fn(|j| color(cell.ao[j], positions[j]));
                let positions = positions.map(|p| p.to_array());
                buffers.push_tiled_quad(positions, template[0].normal, uvs, colors, cell.light);
                return;
            }

            let mut corner = origin;
            corner[a_axis] -= 0.5;
            corner[b_axis] -= 0.5;
            corner[normal_axis] += template[0].position[normal_axis];
            let (mut ta_step, mut tb_step) = (ta.as_vec3(), tb.as_vec3());
            match cell.span {
                Some([_, ceiling]) if normal.y > 0 => corner.y = ceiling,
                Some([floor, ceiling]) => {
                    corner.y = floor;
                    // the side's one step up spans just its trimmed height
                    let up = if a_axis == 1 {
                        &mut ta_step
                    } else {
                        &mut tb_step
                    };
                    up.y *= ceiling - floor;
                }
                None => {}
            }
            let extent = [w as f32, h as f32];
            let uv_at = |i: f32, j: f32| {
                let along = [i, j];
                [0, 1].map(|k| {
                    let t = (uv_axes[k] == b_axis) as usize;
                    (if uv_ascending[k] {
                        along[t]
                    } else {
                        extent[t] - along[t]
                    }) * uv_tiles
                })
            };
            buffers.push_fan_quad(FanQuad {
                corner,
                ta: ta_step,
                tb: tb_step,
                size: (w, h),
                normal: template[0].normal,
                color: &color,
                ao: by_corner(cell.ao),
                light: by_corner(cell.light),
                uv_at: &uv_at,
                inset,
                corner_lift,
                dip,
                reversed: !ccw_in_ab,
                painted,
            });
            if normal.y > 0 && bevelled {
                let (ta_v, tb_v) = (ta_step, tb_step);
                let base = |i: usize, j: usize| corner + ta_v * i as f32 + tb_v * j as f32;
                let (corner_ao, corner_light) = (by_corner(cell.ao), by_corner(cell.light));
                // a quad of these points, with the texture at `at` cells along the top, wound as the
                // cube's faces are, lit and tinted as the top is
                let mut push = |points: [Vec3; 4], at: [(f32, f32); 4], outward: Vec3| {
                    let across = |(i, j): (f32, f32)| {
                        (
                            (i / w as f32).clamp(0.0, 1.0),
                            (j / h as f32).clamp(0.0, 1.0),
                        )
                    };
                    let colour = |o: usize| {
                        let (u, v) = across(at[o]);
                        color(blend_corners(corner_ao, u, v), points[o])
                    };
                    let light_at = |o: usize| {
                        let (u, v) = across(at[o]);
                        blend_corners(corner_light, u, v)
                    };
                    // the facet of three of the points, facing out
                    let facet = |a: usize, b: usize, c: usize| {
                        let n = (points[b] - points[a])
                            .cross(points[c] - points[a])
                            .normalize_or_zero();
                        if n.dot(outward) > 0.0 {
                            n
                        } else {
                            -n
                        }
                    };
                    let (first, second) = (facet(0, 1, 2), facet(2, 1, 3));
                    // a strip that rises at its ends, as the ground does where it climbs a wall, is steeper
                    // than the chamfer, so is textured as it really lies, a triangle at a time, and lit as a chamfer
                    if first.dot(outward) < 0.9995 || second.dot(outward) < 0.9995 {
                        for ([a, b, c], normal) in [([0, 1, 2], first), ([2, 1, 3], second)] {
                            // the cube's faces wind with the cross product pointing in
                            let wound = if (points[b] - points[a])
                                .cross(points[c] - points[a])
                                .dot(normal)
                                > 0.0
                            {
                                [a, c, b]
                            } else {
                                [a, b, c]
                            };
                            // the atlas tiles take their texture along the strip, the painted ones their plane's
                            let unfolded =
                                unfolded_uv(wound.map(|o| points[o]), outward, normal, points[0]);
                            let first_vertex = buffers.vertices.len();
                            buffers.push_tiled_triangle(
                                wound.map(|o| points[o].to_array()),
                                // the cube's faces along z store their normals reversed
                                [outward.x, outward.y, -outward.z],
                                if painted {
                                    unfolded
                                } else {
                                    wound.map(|o| uv_at(at[o].0, at[o].1))
                                },
                                wound.map(colour),
                                wound.map(light_at),
                            );
                            if painted {
                                for (j, o) in wound.into_iter().enumerate() {
                                    let foot = (o >= 2) as u32 as f32;
                                    buffers.set_blend(first_vertex + j, outward, foot);
                                }
                            }
                        }
                        return;
                    }
                    let flipped = (points[1] - points[0])
                        .cross(points[2] - points[0])
                        .dot(outward)
                        > 0.0;
                    let order = if flipped { [1, 0, 3, 2] } else { [0, 1, 2, 3] };
                    let first_vertex = buffers.vertices.len();
                    buffers.push_tiled_quad(
                        order.map(|o| points[o].to_array()),
                        // the cube's faces along z store their normals reversed
                        [outward.x, outward.y, -outward.z],
                        // a painted strip's texture runs on from the top's edge
                        if painted {
                            let unfolded = unfolded_uv(points, outward, first, points[0]);
                            order.map(|o| unfolded[o])
                        } else {
                            order.map(|o| uv_at(at[o].0, at[o].1))
                        },
                        order.map(colour),
                        order.map(light_at),
                    );
                    if painted {
                        for (j, o) in order.into_iter().enumerate() {
                            let foot = (o >= 2) as u32 as f32;
                            buffers.set_blend(first_vertex + j, outward, foot);
                        }
                    }
                };
                for k in (0..4).filter(|&k| cell.bevel[k] != Edge::Flat) {
                    let along_b = k < 2;
                    let len = if along_b { h } else { w };
                    let point = |s: usize| match k {
                        0 => (0, s),
                        1 => (w, s),
                        2 => (s, 0),
                        _ => (s, h),
                    };
                    let side = [-ta_v, ta_v, -tb_v, tb_v][k];
                    // the same direction in cells along the quad
                    let side_ij = [(-1.0, 0.0), (1.0, 0.0), (0.0, -1.0), (0.0, 1.0)][k];
                    // the top's edge point at step `s`, moved as the top's edge is
                    let edge_ij = |s: usize| {
                        let (i, j) = point(s);
                        bevelled_ij(inset, (i as f32, j as f32), (w, h))
                    };
                    // where the quad's own vertex at `(i, j)` is: cut back, and up where the floor
                    // climbs a wall
                    let ring = |i: usize, j: usize| {
                        let (at_i, at_j) = bevelled_ij(inset, (i as f32, j as f32), (w, h));
                        corner
                            + ta_v * at_i
                            + tb_v * at_j
                            + Vec3::Y * lift_at(corner_lift, (i as f32, j as f32), (w, h))
                    };
                    let edge = |s: usize| {
                        let (i, j) = point(s);
                        ring(i, j)
                    };
                    let sharp = |s: usize| {
                        let (i, j) = point(s);
                        base(i, j)
                    };
                    let sharp_ij = |s: usize| (point(s).0 as f32, point(s).1 as f32);
                    // where the texture is `by` cells past the sharp edge, away from the top
                    let past = |s: usize, by: f32| {
                        let (i, j) = sharp_ij(s);
                        (i + side_ij.0 * by, j + side_ij.1 * by)
                    };
                    let kind = cell.bevel[k];
                    let concave = kind == Edge::Concave;
                    let chamfer = (Vec3::Y + side).normalize();
                    let foot = |s: usize| sharp(s) - Vec3::Y * BEVEL;
                    // a cut is a strip across the corner, and a floor climbing a wall needs none
                    for s in (0..len).filter(|_| !concave) {
                        // the strip's texture spans its length, over from the top's edge
                        let at = [
                            edge_ij(s),
                            edge_ij(s + 1),
                            past(s, 0.083),
                            past(s + 1, 0.083),
                        ];
                        push([edge(s), edge(s + 1), foot(s), foot(s + 1)], at, chamfer);
                    }
                }
            }
        };

        for slice in 0..size {
            // merging takes every face it finds out of the mask, so it's left empty for the next slice
            let mut faces = 0;
            let inside_next = (0..size).contains(&(slice + toward));
            // nothing to show in a slice with no blocks, or whose next slice is wholly opaque
            if held[slice as usize][normal_axis] == 0
                || (inside_next
                    && hidden_is_final
                    && opaque[(slice + toward) as usize][normal_axis] == n * n)
            {
                continue;
            }
            for b in 0..size {
                for a in 0..size {
                    let at = (slice * step_n + a * step_a + b * step_b) as usize;
                    let block = source.blocks[at];
                    if matches!(block, BlockType::Air | BlockType::Water) {
                        continue;
                    }
                    // most blocks are buried, and face_span would turn each down after looking where we can
                    if inside_next && hidden_is_final {
                        let beyond = source.blocks[(at as i32 + toward * step_n) as usize];
                        if beyond.is_opaque()
                            || (block == BlockType::Ice && beyond == BlockType::Ice)
                        {
                            continue;
                        }
                    }
                    let coord = normal_unit * slice + ta * a + tb * b;
                    let Some(cell) = cell_at(coord, block) else {
                        continue;
                    };
                    faces += 1;
                    mask[(b * size + a) as usize] = Some(cell);
                }
            }
            if faces == 0 {
                continue;
            }

            for b0 in 0..n {
                for a0 in 0..n {
                    let Some(cell) = mask[b0 * n + a0] else {
                        continue;
                    };

                    let (mut w, mut h) = (1, 1);
                    // a trimmed side's top and bottom sit off the cell grid, so it merges only
                    // along the ground's contour, where its neighbours share those heights
                    let trimmed_side = cell.span.is_some() && normal.y == 0;
                    let (grow_a, grow_b) =
                        (!trimmed_side || a_axis != 1, !trimmed_side || b_axis != 1);
                    #[cfg(test)]
                    let (grow_a, grow_b) = if NO_MERGING.get() {
                        (false, false)
                    } else {
                        (grow_a, grow_b)
                    };
                    while grow_a
                        && a0 + w < n
                        && mask[b0 * n + a0 + w].is_some_and(|o| cell.merges_along(&o, &corners, 0))
                    {
                        w += 1;
                    }
                    while grow_b
                        && b0 + h < n
                        && (a0..a0 + w).all(|a| {
                            mask[(b0 + h) * n + a]
                                .is_some_and(|o| cell.merges_along(&o, &corners, 1))
                        })
                    {
                        h += 1;
                    }
                    for b in b0..b0 + h {
                        for a in a0..a0 + w {
                            mask[b * n + a] = None;
                        }
                    }

                    let at = normal_unit * slice + ta * a0 as i32 + tb * b0 as i32;
                    let target = if cell.inner {
                        &mut *inner
                    } else {
                        &mut *buffers
                    };
                    emit(&cell, (w, h), at, target);
                }
            }
        }
    }
}
