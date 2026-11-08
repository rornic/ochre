use crate::chunk::CHUNK_SIZE;
use bevy::asset::RenderAssetUsages;
use bevy::math::{IVec3, Vec2, Vec3};
use bevy::render::mesh::Indices;
use bevy::render::mesh::Mesh;
use bevy::render::mesh::VertexAttributeValues;
use ochre_base::biome::grass_tint;
use ochre_base::block::TILE_COUNT;
use ochre_base::terrain_source::TerrainSource;

// Accumulates one mesh's worth of vertex/index/color data - one instance for the
// opaque blocks, a second for water, so they can become separate `Mesh`es with
// separate materials (water needs alpha blending, opaque blocks don't).
#[derive(Default)]
pub(super) struct MeshBuffers {
    pub(super) vertices: Vec<Vertex>,
    pub(super) indices: Vec<u32>,
    pub(super) colors: Vec<[f32; 4]>,
    // sky light per vertex, for block faces only, see `world.wesl`
    pub(super) lights: Vec<f32>,
    // whether faces draw their block's mean colour instead of its texture, see `world.wesl`
    pub(super) untextured: bool,
    // where tree boxes start and how they draw, which is on their own terms
    pub(super) boxes: Option<(usize, f32)>,
    // per vertex, how far a painted top is drawn in the wall's projection, see `set_blend`, for the vertices that have one
    pub(super) blends: Vec<f32>,
}

// A painted top meeting a wall is drawn partly in the wall's projection so the texture runs on into the wall, and lit
// partly as the wall. Each of its vertices carries how much, 0 to 1, in its mode (`uv_b.y`) as `BLEND_BASE` plus
// `BLEND_SPAN` times it, in the band for which way the wall faces: x+, x-, z+ or z-, each `BLEND_BAND` wide, so the
// largest mode stays under the 0.5 that marks coarse cells. See `world.wesl`.
// How far up a ramp, as a share of its rise, the wall's projection starts to show.
const RAMP_BLEND_FROM: f32 = 0.6;

const BLEND_BAND: f32 = 0.125;

// Keeps a marked vertex off its band's edge, where interpolation could tip it into the next, and above 0, which is unmarked.
const BLEND_BASE: f32 = 0.0125;

const BLEND_SPAN: f32 = 0.1;

// How far a bevelled edge is cut back along each face that meets there, in blocks: the chamfer is
// a 45 degree strip across the corner this big on each side.
pub(super) const BEVEL: f32 = 0.2;

// How far up a wall the ground against it climbs, in blocks, over the one cell of floor that meets it,
// and its foot is raised to meet: the crease is a slope, not a right angle.
pub(super) const RAMP: f32 = 0.3;

// How much of a crease's ambient occlusion goes where a ramp fills it, at the top of the ramp: the geometry
// does the shading there, and the darkening would count it twice.
const AO_RELIEF: f32 = 0.7;

// Where a point `(i, j)` cells along a quad's tangents ends up, `size` cells wide and tall, with the
// quad's edge pulled in on each side, -ta, +ta, -tb, +tb, by `inset` blocks.
pub(super) fn bevelled_ij(inset: [f32; 4], (i, j): (f32, f32), size: (usize, usize)) -> (f32, f32) {
    let pull = |near: f32, far: f32, at: f32, end: f32| {
        (if at == 0.0 { near } else { 0.0 }) - (if at == end { far } else { 0.0 })
    };
    (
        i + pull(inset[0], inset[1], i, size.0 as f32),
        j + pull(inset[2], inset[3], j, size.1 as f32),
    )
}

// How far up a quad rises at `(i, j)` cells along it, from the rise at each corner, (-ta, -tb), (+ta, -tb),
// (-ta, +tb), (+ta, +tb): the ground that climbs a wall, or the foot of a wall that meets it. Neighbours that
// agree on a corner's rise agree on every point of the edge between, so ramps join.
pub(super) fn lift_at(corners: [f32; 4], (i, j): (f32, f32), size: (usize, usize)) -> f32 {
    let (u, v) = (
        (i / size.0 as f32).clamp(0.0, 1.0),
        (j / size.1 as f32).clamp(0.0, 1.0),
    );
    blend_corners([[corners[0], corners[2]], [corners[1], corners[3]]], u, v)
}

// The value at `(u, v)` across a quad from its values at the four corners, [along ta][along tb].
pub(super) fn blend_corners(corners: [[f32; 2]; 2], u: f32, v: f32) -> f32 {
    let along = |c: [f32; 2]| c[0] + (c[1] - c[0]) * v;
    along(corners[0]) + (along(corners[1]) - along(corners[0])) * u
}

// How a face draws, carried in each vertex's second uv, see `world.wesl`: its own tile, its
// tile with leaves' holes filled, or its block's mean colour.
pub(super) const TEXTURED: f32 = 0.0;

pub(super) const TEXTURED_SOLID: f32 = 0.75;

pub(super) const UNTEXTURED: f32 = 1.0;

// Added to the `x` of a painted face's texture coordinates that the mesh worked out itself, so `material_uv` in
// `block_texture.wesl` takes them as they are, in blocks along the face's plane from the chunk's corner.
pub(super) const UNFOLDED: f32 = 1024.0;

// Texture coordinates for a triangle lying `facet` way, in the axes of the plane `face` is projected onto (x and z for
// a top, z and down y for a side along x, x and down y for one along z), with its tilt from that plane spread out so it
// covers as much of the texture as a flat one does. The points are spread about `anchor`, which keeps the
// texture where it is, so it's the same map across all of them.
pub(super) fn unfolded_uv<const N: usize>(
    points: [Vec3; N],
    face: Vec3,
    facet: Vec3,
    anchor: Vec3,
) -> [[f32; 2]; N] {
    let n = face.abs();
    let top = n.y >= n.x && n.y >= n.z;
    let side_x = !top && n.x >= n.z;
    let plane = |p: Vec3| -> Vec2 {
        if top {
            Vec2::new(p.x, p.z)
        } else if side_x {
            Vec2::new(p.z, -p.y)
        } else {
            Vec2::new(p.x, -p.y)
        }
    };
    let tilt = if top {
        Vec2::new(facet.x, facet.z) / facet.y.abs().max(1e-3)
    } else if side_x {
        Vec2::new(facet.z, -facet.y) / facet.x.abs().max(1e-3)
    } else {
        Vec2::new(facet.x, -facet.y) / facet.z.abs().max(1e-3)
    };
    let anchor = plane(anchor);
    let steep = tilt.length();
    points.map(|p| {
        let across = plane(p) - anchor;
        let uv = if steep < 1e-4 {
            anchor + across
        } else {
            let along = tilt / steep;
            anchor + across + along * across.dot(along) * ((1.0 + steep * steep).sqrt() - 1.0)
        };
        [uv.x + UNFOLDED, uv.y]
    })
}

impl MeshBuffers {
    pub(super) fn push_face(
        &mut self,
        vs: &[Vertex],
        position: Vec3,
        uv_offset: f32,
        colors: &[[f32; 4]],
    ) {
        let uv_scale = 1.0 / TILE_COUNT as f32;

        let triangle_start: u32 = self.vertices.len() as u32;
        for (v, color) in vs.iter().zip(colors.iter()) {
            self.vertices.push(Vertex {
                position: (Vec3::from(v.position) + position).into(),
                normal: v.normal,
                uv: [uv_scale * (v.uv[0] + uv_offset), v.uv[1]],
            });
            self.colors.push(*color);
        }
        self.push_quad_indices(triangle_start);
    }

    // A spray particle: four corners at `base` the shader spreads to face the camera, thrown along `flow` in blocks
    // a second, `size` its width in blocks, a droplet scattering, `burst` the phase of its emitter's bursts and `seed`
    // when in each burst it's thrown.
    pub(super) fn push_particle(
        &mut self,
        base: Vec3,
        flow: Vec3,
        seed: f32,
        size: f32,
        droplet: bool,
        burst: f32,
    ) {
        let start = self.vertices.len() as u32;
        for uv in [[-1.0, -1.0], [1.0, -1.0], [-1.0, 1.0], [1.0, 1.0]] {
            self.vertices.push(Vertex {
                position: base.to_array(),
                normal: flow.to_array(),
                uv,
            });
            self.colors
                .push([seed, size, if droplet { 1.0 } else { 0.0 }, burst]);
        }
        self.push_quad_indices(start);
    }

    // Draws the vertex `vertex` of a painted top `amount` in the projection and light of a wall facing `facing`, which
    // is horizontal along one axis, in the mesh's own axes.
    pub(super) fn set_blend(&mut self, vertex: usize, facing: Vec3, amount: f32) {
        if self.blends.len() < self.vertices.len() {
            self.blends.resize(self.vertices.len(), 0.0);
        }
        // the cube's faces along z store their normals reversed, and the shader reads the band as they are stored
        let band = if facing.x.abs() >= facing.z.abs() {
            (facing.x < 0.0) as u32
        } else {
            2 + (facing.z > 0.0) as u32
        };
        self.blends[vertex] =
            band as f32 * BLEND_BAND + BLEND_BASE + amount.clamp(0.0, 1.0) * BLEND_SPAN;
    }

    fn push_quad_indices(&mut self, start: u32) {
        self.indices.extend([0, 1, 2, 2, 1, 3].map(|i| start + i));
    }

    // `uvs` count tiles across the quad, and the colour alpha carries the atlas cell, see `world.wesl`.
    pub(super) fn push_tiled_quad(
        &mut self,
        positions: [[f32; 3]; 4],
        normal: [f32; 3],
        uvs: [[f32; 2]; 4],
        colors: [[f32; 4]; 4],
        lights: [f32; 4],
    ) {
        let triangle_start: u32 = self.vertices.len() as u32;
        for j in 0..4 {
            self.vertices.push(Vertex {
                position: positions[j],
                normal,
                uv: uvs[j],
            });
            self.colors.push(colors[j]);
            self.lights.push(lights[j]);
        }
        self.push_quad_indices(triangle_start);
    }

    // One triangle with a vertex set of its own, so its normal is its facet's.
    pub(super) fn push_tiled_triangle(
        &mut self,
        positions: [[f32; 3]; 3],
        normal: [f32; 3],
        uvs: [[f32; 2]; 3],
        colors: [[f32; 4]; 3],
        lights: [f32; 3],
    ) {
        let start = self.vertices.len() as u32;
        for j in 0..3 {
            self.vertices.push(Vertex {
                position: positions[j],
                normal,
                uv: uvs[j],
            });
            self.colors.push(colors[j]);
            self.lights.push(lights[j]);
        }
        self.indices.extend([start, start + 1, start + 2]);
    }

    // As `push_tiled_quad`, drawn from both sides: the one set of vertices, with the indices of
    // both windings, as the material culls one side of a triangle.
    pub(super) fn push_double_sided_quad(
        &mut self,
        positions: [[f32; 3]; 4],
        normal: [f32; 3],
        uvs: [[f32; 2]; 4],
        colors: [[f32; 4]; 4],
        lights: [f32; 4],
    ) {
        let start = self.vertices.len() as u32;
        self.push_tiled_quad(positions, normal, uvs, colors, lights);
        self.indices.extend([0, 2, 1, 1, 2, 3].map(|i| start + i));
    }

    // Vertices at every block step round the border, so neighbours share exact edge points and leave no cracks.
    pub(super) fn push_fan_quad(&mut self, quad: FanQuad) {
        let (w, h) = quad.size;
        let mut ring = Vec::with_capacity(2 * (w + h));
        ring.extend((0..w).map(|i| (i, 0)));
        ring.extend((0..h).map(|j| (w, j)));
        ring.extend((1..=w).rev().map(|i| (i, h)));
        ring.extend((1..=h).rev().map(|j| (0, j)));
        // each point of the ring, and the corner it's a bevel's width from, if it's one of those
        let mut ring: Vec<(f32, f32, Option<usize>)> = ring
            .into_iter()
            .map(|(i, j)| (i as f32, j as f32, None))
            .collect();
        // the corners, in the order of `dip`
        let corners = [
            (0.0, 0.0),
            (w as f32, 0.0),
            (0.0, h as f32),
            (w as f32, h as f32),
        ];
        // a wall's feet are cut down along the foot, and its sides run straight
        let wall = quad.normal[1] == 0.0;
        for (k, corner) in corners.into_iter().enumerate() {
            if quad.dip[k] <= 0.0 {
                continue;
            }
            let at = ring
                .iter()
                .position(|p| (p.0, p.1) == corner)
                .expect("a corner is on the ring");
            let (before, after) = (
                ring[(at + ring.len() - 1) % ring.len()],
                ring[(at + 1) % ring.len()],
            );
            // a bevel's width from the corner along an edge, toward the ring's next point
            let along = |to: (f32, f32, Option<usize>)| {
                let (di, dj) = (to.0 - corner.0, to.1 - corner.1);
                if wall && di == 0.0 {
                    return None;
                }
                // the ring runs along the quad's axes, so one of these is none
                let length = di.abs().max(dj.abs());
                Some((
                    corner.0 + di / length * BEVEL,
                    corner.1 + dj / length * BEVEL,
                    Some(k),
                ))
            };
            let (near_before, near_after) = (along(before), along(after));
            if let Some(near) = near_after {
                ring.insert(at + 1, near);
            }
            if let Some(near) = near_before {
                ring.insert(at, near);
            }
        }

        // a painted top that climbs a wall is drawn partly in the wall's projection at its foot, so the texture runs on
        // into the wall: the wall is on the side whose corners rise, and faces away from it
        let [c0, c1, c2, c3] = quad.corner_lift;
        let (rise_a, rise_b) = (c1 + c3 - c0 - c2, c2 + c3 - c0 - c1);
        let wall_facing = -if rise_b.abs() >= rise_a.abs() {
            quad.tb * rise_b.signum()
        } else {
            quad.ta * rise_a.signum()
        };
        let blended = quad.painted && !wall && quad.corner_lift.iter().any(|&l| l > 0.0);
        let blend = blend_corners;
        let mut push = |i: f32, j: f32, near: Option<usize>| {
            let (at_i, at_j) = bevelled_ij(quad.inset, (i, j), quad.size);
            let dip = corners
                .iter()
                .zip(quad.dip)
                .find(|(&c, _)| c == (i, j))
                .map_or(0.0, |(_, dip)| dip);
            // a point a bevel's width from a cut corner is as high as the corner is raised, as the chamfer's end beside it
            let raised = match near {
                Some(k) => quad.corner_lift[k],
                None => lift_at(quad.corner_lift, (i, j), quad.size),
            };
            let lift = raised - dip;
            let position = quad.corner + quad.ta * at_i + quad.tb * at_j + Vec3::Y * lift;
            // the texture follows the point to where it's moved to
            self.vertices.push(Vertex {
                position: position.into(),
                normal: quad.normal,
                uv: (quad.uv_at)(at_i, at_j),
            });
            let (u, v) = (i / w as f32, j / h as f32);
            let ao = blend(quad.ao, u, v);
            let ao = ao + (1.0 - ao) * AO_RELIEF * (lift / RAMP).clamp(0.0, 1.0);
            self.colors.push((quad.color)(ao, position));
            self.lights.push(blend(quad.light, u, v));
            if blended {
                // the last part of the ramp up to the wall: further out the ground's own projection is the better fit
                let amount = (raised / RAMP - RAMP_BLEND_FROM) / (1.0 - RAMP_BLEND_FROM);
                self.set_blend(self.vertices.len() - 1, wall_facing, amount);
            }
            self.vertices.len() as u32 - 1
        };
        let centre = push(w as f32 / 2.0, h as f32 / 2.0, None);
        let first = centre + 1;
        for &(i, j, near) in &ring {
            push(i, j, near);
        }
        for k in 0..ring.len() as u32 {
            let (from, to) = (first + k, first + (k + 1) % ring.len() as u32);
            if quad.reversed {
                self.indices.extend([centre, to, from]);
            } else {
                self.indices.extend([centre, from, to]);
            }
        }
    }

    pub(super) fn build_mesh(self) -> Mesh {
        let mut mesh = Mesh::new(
            bevy::render::mesh::PrimitiveTopology::TriangleList,
            RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
        );
        mesh.insert_indices(Indices::U32(self.indices));
        mesh.insert_attribute(
            Mesh::ATTRIBUTE_POSITION,
            VertexAttributeValues::Float32x3(self.vertices.iter().map(|v| v.position).collect()),
        );
        mesh.insert_attribute(
            Mesh::ATTRIBUTE_NORMAL,
            VertexAttributeValues::Float32x3(self.vertices.iter().map(|v| v.normal).collect()),
        );
        mesh.insert_attribute(
            Mesh::ATTRIBUTE_UV_0,
            VertexAttributeValues::Float32x2(self.vertices.iter().map(|v| v.uv).collect()),
        );
        mesh.insert_attribute(
            Mesh::ATTRIBUTE_COLOR,
            VertexAttributeValues::Float32x4(self.colors),
        );
        if !self.lights.is_empty() {
            assert_eq!(
                self.lights.len(),
                self.vertices.len(),
                "only some vertices carry light"
            );
            let lights: Vec<[f32; 2]> = self
                .lights
                .into_iter()
                .enumerate()
                .map(|(i, l)| match self.boxes {
                    Some((start, flag)) if i >= start => [l, flag],
                    _ => [
                        l,
                        if self.untextured {
                            UNTEXTURED
                        } else {
                            TEXTURED + self.blends.get(i).copied().unwrap_or(0.0)
                        },
                    ],
                })
                .collect();
            mesh.insert_attribute(
                Mesh::ATTRIBUTE_UV_1,
                VertexAttributeValues::Float32x2(lights),
            );
        }
        mesh
    }
}

pub(super) struct FanQuad<'a> {
    pub(super) corner: Vec3,
    pub(super) ta: Vec3,
    pub(super) tb: Vec3,
    pub(super) size: (usize, usize),
    pub(super) normal: [f32; 3],
    // the colour at a position, given its ambient occlusion
    pub(super) color: &'a dyn Fn(f32, Vec3) -> [f32; 4],
    // ambient occlusion and sky light at the quad's corners, [along ta][along tb], blended across it
    pub(super) ao: [[f32; 2]; 2],
    pub(super) light: [[f32; 2]; 2],
    pub(super) uv_at: &'a dyn Fn(f32, f32) -> [f32; 2],
    // how far each side, -ta, +ta, -tb, +tb, is pulled in for a bevel, see `bevelled_ij`
    pub(super) inset: [f32; 4],
    // how far each corner rises, see `lift_at`
    pub(super) corner_lift: [f32; 4],
    // how far down each corner is cut, (-ta, -tb), (+ta, -tb), (-ta, +tb), (+ta, +tb), with a vertex a bevel's width
    // along each of its two edges, so the chamfers beside it run into the cut
    pub(super) dip: [f32; 4],
    pub(super) reversed: bool,
    // whether it draws a painted material, see `material_uv`
    pub(super) painted: bool,
}

// `textured` is whether faces draw their own tile, not the one their block's colour is read from.
#[derive(Clone, Copy)]
pub(super) struct MeshStyle {
    pub(super) ambient_occlusion: bool,
    pub(super) textured: bool,
    pub(super) inner_leaves: InnerLeaves,
    // whether faces are cut back along convex edges and chamfered, see `bevelled_position`
    pub(super) bevel: bool,
}

// The faces between two leaves, which only show through the leaves' holes. In a forest of big
// crowns they're most of its faces.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum InnerLeaves {
    // left out: leaves drawn solid hide them, as do leaves past `INNER_LEAVES_REACH`, whose
    // holes are filled
    Hidden,
    // meshed apart, into `ChunkMeshes::inner_leaves`, drawn only near the camera
    Apart,
}

pub struct ChunkMeshes {
    pub opaque: Mesh,
    /// Outer foliage separated before upload, while generation retains its original mesh order.
    pub cutout: Option<Mesh>,
    pub water: Option<Mesh>,
    /// Fresh water's spray, see `SprayMaterial`.
    pub spray: Option<Mesh>,
    /// The faces between leaves, drawn only within `INNER_LEAVES_REACH` of the camera.
    pub inner_leaves: Option<Mesh>,
}

impl ChunkMeshes {
    /// Whether there's anything to draw: buried chunks mesh to nothing.
    pub fn has_geometry(&self) -> bool {
        self.opaque.count_vertices() > 0
            || self.cutout.is_some()
            || self.water.is_some()
            || self.inner_leaves.is_some()
            || self.spray.is_some()
    }
}

/// Within this many blocks of the camera leaves show the leaves behind them through their holes.
/// Farther out their holes fill in with those leaves' shade, dithered in over
/// `INNER_LEAVES_BAND` blocks, and the faces between leaves aren't drawn: a leaf's hole is about
/// a pixel by then, and a dense forest's crowns hold several times the faces on their outsides.
pub const INNER_LEAVES_REACH: f32 = 64.0;

pub const INNER_LEAVES_BAND: f32 = 16.0;

// Tints grass by the climate it grows in. Climate drifts over thousands of blocks, so it's
// sampled at the mesh's four corners and blended across it, rather than at every vertex.
pub(super) struct GrassTint {
    // at the mesh's edges, so neighbouring meshes share their tints along each edge: (-x, -z), (+x, -z), (-x, +z), (+x, +z)
    pub(super) corners: [Vec3; 4],
    // the block the mesh's local origin is, so what grows where can follow the world and not repeat per mesh
    pub(super) origin: IVec3,
}

impl GrassTint {
    // `origin` is where the mesh's local origin is in the world, and `scale` how many blocks one local unit is.
    pub(super) fn new(noise: &dyn TerrainSource, origin: Vec3, scale: f32) -> Self {
        let size = CHUNK_SIZE as f32;
        let corners = [(0.0, 0.0), (size, 0.0), (0.0, size), (size, size)].map(|(x, z)| {
            let world = origin + Vec3::new(x - 0.5, 0.0, z - 0.5) * scale;
            grass_tint(noise.climate(world.x as f64, world.z as f64))
        });
        Self {
            corners,
            origin: origin.floor().as_ivec3(),
        }
    }

    pub(super) fn at(&self, local: Vec3) -> Vec3 {
        let size = CHUNK_SIZE as f32;
        let (u, v) = ((local.x + 0.5) / size, (local.z + 0.5) / size);
        let [a, b, c, d] = self.corners;
        a.lerp(b, u).lerp(c.lerp(d, u), v)
    }
}

// Cells wider than this draw their block's mean colour instead of its texture, which tiles a
// cell's width of times over a face, since one stretched over a coarse cell would read as a
// giant block. Past a few blocks a tile is under a pixel and mipmaps down to the mean anyway.
pub(super) const TEXTURE_MAX_CELL_SIZE: i64 = 4;

// Tree boxes keep their leaf texture's holes up to cells this wide. Past it boxes are mostly
// canopy, too wide to see through, and keep the texture with its holes filled.
pub(super) const BOX_TEXTURE_MAX_CELL: i64 = 8;

// A quad's `uvs` scaled to count blocks across it, so a texture isn't stretched over a whole box.
pub(super) fn tiled_uvs(
    positions: &[[f32; 3]; 4],
    uvs: &[[f32; 2]; 4],
    cell: f32,
) -> [[f32; 2]; 4] {
    let length = |axis: usize| {
        let other = |j: usize, k: usize| {
            uvs[j][1 - axis] == uvs[k][1 - axis] && uvs[j][axis] != uvs[k][axis]
        };
        let (j, k) = (0..4)
            .flat_map(|j| (0..4).map(move |k| (j, k)))
            .find(|&(j, k)| other(j, k))
            .unwrap_or((0, 0));
        (Vec3::from(positions[j]) - Vec3::from(positions[k])).length() * cell
    };
    let blocks = [length(0), length(1)];
    std::array::from_fn(|j| [uvs[j][0] * blocks[0], uvs[j][1] * blocks[1]])
}

#[derive(Default, Copy, Clone)]
pub(super) struct Vertex {
    pub(super) position: [f32; 3],
    pub(super) normal: [f32; 3],
    pub(super) uv: [f32; 2],
}

const fn v(position: [f32; 3], normal: [f32; 3], uv: [f32; 2]) -> Vertex {
    Vertex {
        position,
        normal,
        uv,
    }
}

// A unit cube's faces, in `FACE_NORMAL_OFFSET` order. Side faces put v = 0 at their top, as the
// atlas's tiles start from their top row.
pub(super) const CUBE: [[Vertex; 4]; 6] = [
    // front
    [
        v([-0.5, 0.5, -0.5], [0.0, 0.0, 1.0], [0.0, 0.0]),
        v([-0.5, -0.5, -0.5], [0.0, 0.0, 1.0], [0.0, 1.0]),
        v([0.5, 0.5, -0.5], [0.0, 0.0, 1.0], [1.0, 0.0]),
        v([0.5, -0.5, -0.5], [0.0, 0.0, 1.0], [1.0, 1.0]),
    ],
    // right
    [
        v([0.5, 0.5, -0.5], [1.0, 0.0, 0.0], [0.0, 0.0]),
        v([0.5, -0.5, -0.5], [1.0, 0.0, 0.0], [0.0, 1.0]),
        v([0.5, 0.5, 0.5], [1.0, 0.0, 0.0], [1.0, 0.0]),
        v([0.5, -0.5, 0.5], [1.0, 0.0, 0.0], [1.0, 1.0]),
    ],
    // left
    [
        v([-0.5, 0.5, 0.5], [-1.0, 0.0, 0.0], [1.0, 0.0]),
        v([-0.5, -0.5, 0.5], [-1.0, 0.0, 0.0], [1.0, 1.0]),
        v([-0.5, 0.5, -0.5], [-1.0, 0.0, 0.0], [0.0, 0.0]),
        v([-0.5, -0.5, -0.5], [-1.0, 0.0, 0.0], [0.0, 1.0]),
    ],
    // back
    [
        v([0.5, 0.5, 0.5], [0.0, 0.0, -1.0], [1.0, 0.0]),
        v([0.5, -0.5, 0.5], [0.0, 0.0, -1.0], [1.0, 1.0]),
        v([-0.5, 0.5, 0.5], [0.0, 0.0, -1.0], [0.0, 0.0]),
        v([-0.5, -0.5, 0.5], [0.0, 0.0, -1.0], [0.0, 1.0]),
    ],
    // top
    [
        v([-0.5, 0.5, 0.5], [0.0, 1.0, 0.0], [0.0, 1.0]),
        v([-0.5, 0.5, -0.5], [0.0, 1.0, 0.0], [1.0, 1.0]),
        v([0.5, 0.5, 0.5], [0.0, 1.0, 0.0], [0.0, 0.0]),
        v([0.5, 0.5, -0.5], [0.0, 1.0, 0.0], [1.0, 0.0]),
    ],
    // bottom
    [
        v([-0.5, -0.5, -0.5], [0.0, -1.0, 0.0], [0.0, 1.0]),
        v([-0.5, -0.5, 0.5], [0.0, -1.0, 0.0], [1.0, 1.0]),
        v([0.5, -0.5, -0.5], [0.0, -1.0, 0.0], [0.0, 0.0]),
        v([0.5, -0.5, 0.5], [0.0, -1.0, 0.0], [1.0, 0.0]),
    ],
];
