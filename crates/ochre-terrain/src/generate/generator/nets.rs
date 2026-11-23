// Ground as one sculpted surface (surface nets, rock by dual contouring), see docs/sculpted-ground.md.
use super::buffers::{stored_normal, GrassTint, MeshBuffers, Vertex};
use super::shading::face_light;
use super::source::BlockSource;
use crate::chunk::CHUNK_SIZE;
use crate::generate::light::SkyLight;
use bevy::math::{IVec3, Mat3, Vec3};
use ochre_base::block::{BlockType, Face, Tile};
use std::ops::Deref;

// Quads left without a diagonal whose triangles both face from solid to air, which the cell constraint makes impossible.
#[cfg(test)]
pub(super) static UNSOUND_QUADS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

// What the surface wraps: logs and leaves are drawn as they were.
pub(super) fn ground(block: BlockType) -> bool {
    block.is_opaque() && !matches!(block, BlockType::Leaves | BlockType::Log)
}

fn soft(block: BlockType) -> bool {
    matches!(
        block,
        BlockType::Grass | BlockType::Sand | BlockType::Dirt | BlockType::Snow | BlockType::Gravel | BlockType::Clay
    )
}

// Per material: relax is each pass's pull to the neighbours' mean, wobble is in blocks, facet runs 0 smooth to 1 flat.
#[derive(Clone, Copy)]
struct Look {
    relax: f32,
    wobble: f32,
    facet: f32,
}

struct Knobs {
    rock: Look,
    soft: Look,
    // soft ground's lumps, a few blocks across, pushed out along the surface
    lump_height: f32,
    lump_width: f32,
    // how far rock looks round each crossing for the plane it lies on; wider gives broader faces
    plane_reach: usize,
    // how far a vertex may stand out from its neighbours' mean, so a lone block isn't a pyramid
    soft_spike: f32,
    rock_spike: f32,
    ao: f32,
    // rock's planes snap to this many directions round and this many steps up from level to straight up
    chisel_round: f32,
    chisel_up: f32,
}

const KNOBS: Knobs = Knobs {
    rock: Look { relax: 0.0, wobble: 0.0, facet: 1.0 },
    soft: Look { relax: 0.6, wobble: 0.18, facet: 0.15 },
    lump_height: 0.9,
    lump_width: 6.0,
    plane_reach: PLANE_REACH,
    soft_spike: 0.3,
    rock_spike: 0.45,
    ao: 2.2,
    chisel_round: 6.0,
    chisel_up: 2.0,
};

const SIZE: i32 = CHUNK_SIZE as i32;
// Blocks read past the chunk's edge, all its neighbourhood has, so its edge vertices come out as its neighbours' do.
const REACH: i32 = 16;
const PLANE_REACH: usize = 2;
const RELAX_PASSES: usize = 2;
// How far a vertex's blend weight strays from its own material: the shader blends only past it, so triangles agree.
const OWN_SIDE: f32 = 0.3;
// Where rock's count starts in a packed count, past the most solid blocks a line's running sum can hold.
const ROCK_COUNT: u32 = 1 << 16;
const SPIKE_PASSES: usize = 1;
const CHISEL_PASSES: usize = 2;
// How far either side of half way between two snapped directions a face slides from one to the other, of the step.
const CHISEL_SOFT: f32 = 0.25;
// How far each chisel pass moves a vertex toward its planes, past all the way so three passes do the work of ten.
const CHISEL_STEP: f32 = 2.0;
// How far, in blocks, an edit can move the sculpted surface: a vertex's placing reads 5 out and every step a cell more.
pub(crate) const EDIT_REACH: i64 = (VERTEX_REACH + PLANE_REACH + 3) as i64;
// Steps that read a vertex's neighbours, each taking any error a cell further in: the passes, the facing and the normals.
const NEIGHBOUR_STEPS: usize = RELAX_PASSES + SPIKE_PASSES + 1 + CHISEL_PASSES + 1;
// Cells past the chunk that get a vertex: one an owned quad uses, and the steps' reach past it, plus the wrong one at the end.
const VERTEX_REACH: usize = 1 + NEIGHBOUR_STEPS + 1;
// A vertex is placed from blocks a gap fill, a blur, a slope and a corner away, so the last right one needs this many.
const _: () = assert!(VERTEX_REACH + PLANE_REACH + 2 <= REACH as usize);
// A LOD node's ring repeated this far out, enough for the counts round its edge; its edge needn't match a neighbour's.
const LOD_REACH: i32 = 4;
// How far, in blocks, a node's surface sits under the ground's, so full-detail chunks drawn over it as it stands in for them win.
const LOD_SINK: f32 = 0.3;
// How far a skirt leans into its node for each cell it runs, so it never lies in the plane a neighbour draws in, see docs/sculpted-ground.md.
pub(super) const SKIRT_LEAN: f32 = 1.0 / 16.0;
// How far, in cells, the lowest vertices of the row past a node's bottom sit in behind the node below's, see docs/sculpted-ground.md.
const ROW_LEAN: f32 = 0.5;
// How far out along its normal a vertex's occlusion is read from the blurred blocks.
const AO_OUT: f32 = 1.5;
// Rock's occlusion: the openness it starts at (flat rock reads about 0.2), the most it takes, and the excess at which it has taken half of that.
const ROCK_AO_FROM: f32 = 0.2;
const ROCK_AO_MAX: f32 = 0.55;
const ROCK_AO_KNEE: f32 = 0.12;
// A vertex's occlusion, 0.2 to 1, rides in its cell's fraction, within the ±0.5 every reader rounds away, see `world.wesl`.
const AO_MID: f32 = 0.6;
const AO_SPREAD: f32 = 1.0;
// Marks a vertex as projected three ways, in its uv's x, less the cell of the material it blends to, see `world.wesl`.
const TRIPLANAR: f32 = -1024.0;
// A vertex's facet, 0 to 1, rides in its mode (`uv_b.y`) as this much of it, under the 0.5 that marks coarse cells.
const FACET_SPAN: f32 = 0.45;

// `x` drawn to the nearest whole number, sliding over half way, so a face turns with the ground instead of flipping.
fn soft_snap(x: f32) -> f32 {
    let t = x - x.floor();
    let s = ((t - 0.5 + CHISEL_SOFT) / (2.0 * CHISEL_SOFT)).clamp(0.0, 1.0);
    x.floor() + s * s * (3.0 - 2.0 * s)
}

// A block's tile, its grass the steppe's on the steppe.
fn tile_of(source: &impl BlockSource, block: BlockType, face: Face, coord: IVec3) -> Tile {
    match block.tile(face) {
        Tile::GrassTop | Tile::GrassSide if source.steppe(coord.x, coord.z) => Tile::SteppeTop,
        tile => tile,
    }
}

// A vertex's tint, as the cube mesher works it out for a face of `block`, with its tile's cell.
fn colour(source: &impl BlockSource, grass_tint: &GrassTint, block: BlockType, tile: Tile, position: Vec3) -> [f32; 4] {
    let tint = if !tile.tinted() {
        grass_tint.weathered_at(block, position)
    } else if block == BlockType::Grass {
        grass_tint.ground_at(position)
    } else {
        grass_tint.at(position)
    };
    let shade = if block == BlockType::Grass { source.forest_shade(position) } else { 1.0 };
    (tint * shade).extend(tile.atlas_cell()).to_array()
}

// A smooth steady noise in -1..1 over world space.
fn noise(seed: u32, p: Vec3) -> f32 {
    let h = |x: i32, y: i32, z: i32| {
        let mut v = (x as u32).wrapping_mul(0x9E37_79B1)
            ^ (y as u32).wrapping_mul(0x85EB_CA77)
            ^ (z as u32).wrapping_mul(0xC2B2_AE3D)
            ^ seed;
        v ^= v >> 15;
        v = v.wrapping_mul(0x2C1B_3C6D);
        v ^= v >> 12;
        (v & 0xFFFF) as f32 / 32767.5 - 1.0
    };
    let f = p.floor();
    let t = (p - f).map(|t| t * t * (3.0 - 2.0 * t));
    let (x, y, z) = (f.x as i32, f.y as i32, f.z as i32);
    let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
    let face = |dz: i32| {
        lerp(
            lerp(h(x, y, z + dz), h(x + 1, y, z + dz), t.x),
            lerp(h(x, y + 1, z + dz), h(x + 1, y + 1, z + dz), t.x),
            t.y,
        )
    };
    lerp(face(0), face(1), t.z)
}

// `cell_size` blocks to a cell: a chunk's are 1, a LOD node's wider, whose edge hangs a skirt; `beside` gets the triangles just past a chunk's edge that its neighbours draw, exactly, for what stands there.
pub(super) fn mesh_nets(
    source: &impl BlockSource,
    buffers: &mut MeshBuffers,
    grass_tint: &GrassTint,
    light: &impl Deref<Target = SkyLight>,
    cell_size: i64,
    beside: &mut Vec<[Vec3; 3]>,
) {
    if cell_size > 1 {
        let start = buffers.indices.len();
        let past_bottom = sculpt::<LOD_REACH>(source, buffers, grass_tint, light, cell_size, beside);
        hang_skirts(source, buffers, start, past_bottom);
    } else {
        sculpt::<REACH>(source, buffers, grass_tint, light, cell_size, beside);
    }
}

fn sculpt<const MARGIN: i32>(
    source: &impl BlockSource,
    buffers: &mut MeshBuffers,
    grass_tint: &GrassTint,
    light: &impl Deref<Target = SkyLight>,
    cell_size: i64,
    beside: &mut Vec<[Vec3; 3]>,
) -> usize {
    let k = &KNOBS;
    let lod = cell_size > 1;
    let margin = MARGIN;
    // lumps and wobble are in blocks, over this for cells
    let cell_blocks = cell_size as f32;
    // blocks by index, x then z then y as the neighbourhood holds them, index `a` being the chunk's block `a - margin`; a const as the indexing runs half again as slow without
    let n = const { (SIZE + 2 * MARGIN) as usize };
    let index = |i: usize, j: usize, kk: usize| (i * n + kk) * n + j;
    let local = |i: usize, j: usize, kk: usize| IVec3::new(i as i32 - margin, j as i32 - margin, kk as i32 - margin);
    let mut blocks = match source.neighbourhood() {
        Some(all) if all.len() == n * n * n => all.to_vec(),
        _ => {
            let mut blocks = vec![BlockType::Air; n * n * n];
            for i in 0..n {
                for j in 0..n {
                    for kk in 0..n {
                        blocks[index(i, j, kk)] = source.terrain(local(i, j, kk));
                    }
                }
            }
            blocks
        }
    };
    let mut solid: Vec<bool> = blocks.iter().map(|&b| ground(b)).collect();
    // the rows the surface crosses: a row both solid and open, or between a solid row and an open one
    let mut rows = vec![(false, false); n];
    for (at, &s) in solid.iter().enumerate() {
        let row = &mut rows[at % n];
        if s {
            row.0 = true;
        } else {
            row.1 = true;
        }
    }
    let (mut low, mut high) = (n, 0);
    for j in 0..n {
        let mixed = rows[j].0 && rows[j].1;
        if mixed || (j + 1 < n && rows[j] != rows[j + 1] && !(rows[j + 1].0 && rows[j + 1].1)) {
            low = low.min(j);
            high = high.max(if mixed { j } else { j + 1 });
        }
    }
    if low > high {
        return buffers.indices.len();
    }
    // blocks touching only along an edge would pinch the vertex between them, so the mesh (not collision) fills the gap
    let touching = solid.clone();
    let planes = [((1, 0, 0), (0, 1, 0)), ((1, 0, 0), (0, 0, 1)), ((0, 1, 0), (0, 0, 1))];
    for i in 0..n - 1 {
        for j in low.saturating_sub(1)..=high.min(n - 2) {
            for kk in 0..n - 1 {
                for ((ua, ub, uc), (wa, wb, wc)) in planes {
                    let square = [(i, j, kk), (i + ua, j + ub, kk + uc), (i + wa, j + wb, kk + wc), (i + ua + wa, j + ub + wb, kk + uc + wc)];
                    let [p, u, w, uw] = square.map(|(a, b, c)| touching[index(a, b, c)]);
                    if p != uw || u != w || p == u {
                        continue;
                    }
                    let (full, gaps) = if p { ([square[0], square[3]], [square[1], square[2]]) } else { ([square[1], square[2]], [square[0], square[3]]) };
                    let higher = if full[1].1 > full[0].1 { full[1] } else { full[0] };
                    let block = blocks[index(higher.0, higher.1, higher.2)];
                    for (a, b, c) in gaps {
                        solid[index(a, b, c)] = true;
                        blocks[index(a, b, c)] = block;
                    }
                }
            }
        }
    }
    // a fill can reach a row either side
    let (low, high) = (low.saturating_sub(1), (high + 1).min(n - 1));
    let surface_tops = source.has_surface_tops();
    // the chunk owns the crossings from its own blocks to the next along each axis
    let own = margin as usize..(margin + SIZE) as usize;
    // a node draws a row past its bottom too, under the top of the node below, as their vertices there needn't agree
    let own_y = if lod { own.start - 1..own.end } else { own.clone() };
    let steps = [(1, 0, 0), (0, 1, 0), (0, 0, 1)];
    // a chunk with ground just past its edge still works it out, for what stands on it there, see `beside`
    let wide = |r: &std::ops::Range<usize>| r.start - 1..r.end + 1;
    let any_near = wide(&own).any(|i| {
        wide(&own_y).filter(|j| (low..=high).contains(j)).any(|j| {
            wide(&own).any(|kk| {
                let here = solid[index(i, j, kk)];
                steps.iter().any(|&(a, b, c)| solid[index(i + a, j + b, kk + c)] != here)
            })
        })
    });
    if !any_near {
        return buffers.indices.len();
    }

    // blocks counted over `plane_reach` each way, for rock's planes and occlusion: whole counts, so neighbours' sums agree
    let cells = n - 1;
    // farther cells' vertices can't reach the chunk's own, see `VERTEX_REACH`
    let (near_low, near_high) = ((margin as usize).saturating_sub(VERTEX_REACH), ((margin + SIZE) as usize - 1 + VERTEX_REACH).min(cells - 1));
    // the blocks whose counts are read: those cells' corners and a block either side for the slope
    let (used_low, used_high) = (near_low.saturating_sub(1), (near_high + 2).min(n - 1));
    let reach = k.plane_reach;
    let window = (2 * reach + 1).pow(3) as f32;
    let (blur_low, blur_high) = (low.saturating_sub(6), (high + 6).min(n - 1));
    let (wide_low, wide_high) = (blur_low.saturating_sub(reach), (blur_high + reach).min(n - 1));
    // rock's count rides in the top half, so one set of running sums counts both, for how rocky the ground round a vertex is
    let mut counts: Vec<u32> = solid.iter().zip(&blocks).map(|(&s, &b)| s as u32 * (1 + (!soft(b)) as u32 * ROCK_COUNT)).collect();
    let mut line = vec![0u32; n + 1];
    let mut run = |counts: &mut Vec<u32>, start: usize, stride: usize, out: std::ops::RangeInclusive<usize>| {
        line[0] = 0;
        for m in 0..n {
            line[m + 1] = line[m] + counts[start + m * stride];
        }
        for m in out {
            let (from, to) = (m.saturating_sub(reach), (m + reach).min(n - 1));
            counts[start + m * stride] = line[to + 1] - line[from];
        }
    };
    for j in wide_low..=wide_high {
        for kk in used_low.saturating_sub(reach)..=(used_high + reach).min(n - 1) {
            run(&mut counts, index(0, j, kk), n * n, used_low..=used_high);
        }
        for i in used_low..=used_high {
            run(&mut counts, index(i, j, 0), n, used_low..=used_high);
        }
    }
    for i in used_low..=used_high {
        for kk in used_low..=used_high {
            run(&mut counts, index(i, 0, kk), 1, blur_low..=blur_high);
        }
    }
    let blurred = |i: usize, j: usize, kk: usize| (counts[index(i, j, kk)] % ROCK_COUNT) as f32 / window;
    let gradient = |i: usize, j: usize, kk: usize| {
        let at = |i: usize, j: usize, kk: usize| blurred(i.min(n - 1), j.min(n - 1), kk.min(n - 1));
        Vec3::new(
            at(i + 1, j, kk) - at(i.saturating_sub(1), j, kk),
            at(i, j + 1, kk) - at(i, j.saturating_sub(1), kk),
            at(i, j, kk + 1) - at(i, j, kk.saturating_sub(1)),
        )
    };

    // cell and offset kept apart, so a vertex a neighbouring chunk shares comes out of the same sums in both
    let cell_index = |i: usize, j: usize, kk: usize| (i * cells + j) * cells + kk;
    let mut vertex_of = vec![u32::MAX; cells * cells * cells];
    let mut homes: Vec<IVec3> = Vec::new();
    let mut offsets: Vec<Vec3> = Vec::new();
    let mut materials: Vec<BlockType> = Vec::new();
    let mut solid_corners: Vec<u8> = Vec::new();
    // the axes a cell's crossings run along, as bits x, y, z
    let mut crossing_axes: Vec<u8> = Vec::new();
    // how far toward rock each vertex's blend is, kept to its own material's side of a half, and how far it's placed as rock
    let mut rockiness: Vec<f32> = Vec::new();
    let mut rocky: Vec<f32> = Vec::new();
    // whether a cell's corner is lit sky, for its vertex's light: the brightest air corner
    let mut lights: Vec<f32> = Vec::new();
    // where each of a cell's corners is from its first, in `index`
    let corner_step: [usize; 8] = std::array::from_fn(|c| (c & 1) * n * n + (c >> 2) * n + ((c >> 1) & 1));
    for i in near_low..=near_high {
        for j in low.saturating_sub(1).max(near_low)..=high.min(near_high) {
            for kk in near_low..=near_high {
                let corner = |c: usize| (i + (c & 1), j + ((c >> 1) & 1), kk + (c >> 2));
                let base = index(i, j, kk);
                let inside: [bool; 8] = std::array::from_fn(|c| solid[base + corner_step[c]]);
                let count = inside.iter().filter(|&&v| v).count();
                if count == 0 || count == 8 {
                    continue;
                }
                // its material: the highest solid corner with air over it, else any solid one
                let mut material = BlockType::Air;
                for c in (0..8).rev() {
                    let (a, b, d) = corner(c);
                    if inside[c] && material == BlockType::Air {
                        material = blocks[index(a, b, d)];
                    }
                    if inside[c] && (c >> 1) & 1 == 0 && !inside[c | 2] {
                        material = blocks[index(a, b, d)];
                        break;
                    }
                }
                let at = |c: usize| Vec3::new((c & 1) as f32, ((c >> 1) & 1) as f32, (c >> 2) as f32);
                // each crossing at its edge's midpoint, and its plane facing out of the counted blocks
                let (mut sum, mut crossings) = (Vec3::ZERO, 0.0);
                let (mut normal_sum, mut point_sum) = (Mat3::ZERO, Vec3::ZERO);
                for c in 0..8 {
                    for bit in [1, 2, 4] {
                        let d = c | bit;
                        if d == c || inside[c] == inside[d] {
                            continue;
                        }
                        let mut crossing = (at(c) + at(d)) * 0.5;
                        // a coarse cell is solid however little ground it holds, so its top crossing drops toward the ground's real top
                        if bit == 2 && inside[c] && surface_tops {
                            let below = local(corner(c).0, corner(c).1, corner(c).2);
                            if let Some(top) = source.surface_top(below) {
                                crossing.y = (top - below.y as f32).max(0.0);
                            }
                        }
                        sum += crossing;
                        crossings += 1.0;
                        // loose ground takes its crossings' mean, so needs no planes
                        if soft(material) {
                            continue;
                        }
                        let (a, b, e) = corner(c);
                        let (f, g, h) = corner(d);
                        let normal = -(gradient(a, b, e) + gradient(f, g, h)).normalize_or_zero();
                        normal_sum += Mat3::from_cols(normal * normal.x, normal * normal.y, normal * normal.z);
                        point_sum += normal * normal.dot(crossing);
                    }
                }
                let mass = sum / crossings;
                // the share of rock in the ground counted round the cell's corners
                let (all, rock) = (0..8).map(|c| counts[base + corner_step[c]]).fold((0, 0), |(a, r), v| (a + v % ROCK_COUNT, r + v / ROCK_COUNT));
                let share = rock as f32 / all.max(1) as f32;
                // half its own material, half the rock round it, so the blend's cut slides along each edge and curves
                let around = ((share - 0.05) / 0.4).clamp(0.0, 1.0);
                let (toward_rock, as_rock) = if soft(material) {
                    (OWN_SIDE * around, 0.0)
                } else {
                    (1.0 - OWN_SIDE + OWN_SIDE * around, ((share - 0.1) / 0.4).clamp(0.25, 1.0))
                };
                // rock where its planes meet, pulled a little to their mean, and allowed just past its cell to make a real edge
                let offset = if soft(material) {
                    mass
                } else {
                    const PULL: f32 = 0.05;
                    const PAST: f32 = 0.3;
                    let a = normal_sum + Mat3::from_diagonal(Vec3::splat(PULL));
                    let planed = (a.inverse() * (point_sum + mass * PULL)).clamp(Vec3::splat(-PAST), Vec3::splat(1.0 + PAST));
                    // rock with little rock round it is placed partway as loose ground, so the two meet without a jag
                    mass.lerp(planed, as_rock)
                };
                let lit = (0..8)
                    .filter(|&c| !inside[c])
                    .map(|c| {
                        let (a, b, d) = corner(c);
                        face_light(light.level(local(a, b, d)))
                    })
                    .fold(0.0, f32::max);
                vertex_of[cell_index(i, j, kk)] = homes.len() as u32;
                homes.push(IVec3::new(i as i32, j as i32, kk as i32));
                offsets.push(offset);
                materials.push(material);
                solid_corners.push(count as u8);
                crossing_axes.push(
                    [1usize, 2, 4].iter().enumerate().fold(0u8, |bits, (axis, &bit)| {
                        bits | (((0..8).any(|c| c & bit == 0 && inside[c] != inside[c | bit])) as u8) << axis
                    }),
                );
                rockiness.push(toward_rock);
                rocky.push(as_rock);
                lights.push(lit);
            }
        }
    }
    let look = |m: BlockType| if soft(m) { k.soft } else { k.rock };
    let between = |from: usize, to: u32, offsets: &[Vec3]| (homes[to as usize] - homes[from]).as_vec3() + offsets[to as usize];

    // which of this chunk and the six against its faces hold any ground before the gap fill: a face goes to its solid block's chunk unless that has none
    let slot = |c: usize| (c as i32 - margin).div_euclid(SIZE) + 1;
    let has_ground = {
        let mut has = [[[true; 3]; 3]; 3];
        if !lod {
            for (x, y, z) in [(1, 1, 1), (0, 1, 1), (2, 1, 1), (1, 0, 1), (1, 2, 1), (1, 1, 0), (1, 1, 2)] {
                let start = |s: i32| (margin + (s - 1) * SIZE) as usize;
                let (si, sj, sk) = (start(x), start(y), start(z));
                has[x as usize][y as usize][z as usize] = (si..si + SIZE as usize)
                    .any(|i| (sj..sj + SIZE as usize).any(|j| (sk..sk + SIZE as usize).any(|kk| touching[index(i, j, kk)])));
            }
        }
        has
    };
    // quads across every crossing between block centres, of the four cells round it, and whether the chunk owns it
    let mut quads: Vec<([u32; 4], bool)> = Vec::new();
    // in step with `quads`, whether a node's quad is in the row past its bottom
    let mut below: Vec<bool> = Vec::new();
    let mut quad_out: Vec<Vec3> = Vec::new();
    // a quad's ring reaches a cell back on the two axes across it, and only cells near enough have vertices
    for i in near_low.max(1)..=near_high {
        for j in low.saturating_sub(1).max(near_low).max(1)..=high.min(near_high) {
            for kk in near_low.max(1)..=near_high {
                let here = solid[index(i, j, kk)];
                for (axis, &(a, b, c)) in steps.iter().enumerate() {
                    if here == solid[index(i + a, j + b, kk + c)] {
                        continue;
                    }
                    let ring: [(usize, usize, usize); 4] = match axis {
                        0 => [(i, j - 1, kk - 1), (i, j, kk - 1), (i, j, kk), (i, j - 1, kk)],
                        1 => [(i - 1, j, kk - 1), (i - 1, j, kk), (i, j, kk), (i, j, kk - 1)],
                        _ => [(i - 1, j - 1, kk), (i, j - 1, kk), (i, j, kk), (i - 1, j, kk)],
                    };
                    let mut quad = ring.map(|(a, b, d)| vertex_of[cell_index(a, b, d)]);
                    if quad.contains(&u32::MAX) {
                        continue;
                    }
                    // the ring winds with its cross product along +axis; the cube's faces wind with it pointing in
                    if here {
                        quad.reverse();
                    }
                    // a chunk draws the faces over its own solid blocks, as the cube mesher did, so an all-air chunk the loader never meshes draws none; a node keeps the low side, as its edges hang skirts
                    let (sol, other) = if here { ((i, j, kk), (i + a, j + b, kk + c)) } else { ((i + a, j + b, kk + c), (i, j, kk)) };
                    let (oi, oj, ok) = if lod {
                        (i, j, kk)
                    } else if has_ground[slot(sol.0) as usize][slot(sol.1) as usize][slot(sol.2) as usize] {
                        sol
                    } else {
                        other
                    };
                    let owned = own.contains(&oi) && own_y.contains(&oj) && own.contains(&ok);
                    quads.push((quad, owned));
                    below.push(lod && oj < own.start);
                    let dir = Vec3::new(a as f32, b as f32, c as f32);
                    quad_out.push(if here { dir } else { -dir });
                }
            }
        }
    }
    let mut neighbours: Vec<Vec<u32>> = vec![Vec::new(); homes.len()];
    for (q, _) in &quads {
        for e in 0..4 {
            let (a, b) = (q[e], q[(e + 1) % 4]);
            if !neighbours[a as usize].contains(&b) {
                neighbours[a as usize].push(b);
                neighbours[b as usize].push(a);
            }
        }
    }
    let mean_of = |v: usize, offsets: &[Vec3]| {
        neighbours[v].iter().map(|&m| between(v, m, offsets)).sum::<Vec3>() / neighbours[v].len() as f32
    };
    // cells past the chunk per vertex; each neighbour-reading step need only reach a cell less far than the one before
    let edge = margin + SIZE - 1;
    // a node's edge needn't match, so every vertex takes every step
    let past: Vec<i32> = if lod {
        vec![0; homes.len()]
    } else {
        homes.iter().map(|h| h.to_array().map(|c| (margin - c).max(c - edge).max(0)).into_iter().max().unwrap_or(0)).collect()
    };
    let mut steps_left = NEIGHBOUR_STEPS as i32;
    let mut step = || {
        steps_left -= 1;
        1 + steps_left
    };
    for _ in 0..RELAX_PASSES {
        let before = offsets.clone();
        let reach = step();
        for v in (0..homes.len()).filter(|&v| past[v] <= reach && !neighbours[v].is_empty()) {
            let relax = k.soft.relax + (k.rock.relax - k.soft.relax) * rocky[v];
            let moved = before[v].lerp(mean_of(v, &before), if soft(materials[v]) { k.soft.relax } else { relax });
            offsets[v] = moved.clamp(Vec3::ZERO, Vec3::ONE);
        }
    }
    // no pyramids on lone blocks: limit each vertex's rise over its neighbours, and drop soft tips and pits onto them
    for _ in 0..SPIKE_PASSES {
        let before = offsets.clone();
        let reach = step();
        for v in (0..homes.len()).filter(|&v| past[v] <= reach && !neighbours[v].is_empty()) {
            let mean = mean_of(v, &before);
            if soft(materials[v]) && matches!(solid_corners[v], 1 | 7) {
                offsets[v] = mean.clamp(Vec3::splat(-1.0), Vec3::splat(2.0));
                continue;
            }
            let spike = if soft(materials[v]) { k.soft_spike } else { k.rock_spike };
            let off = before[v] - mean;
            if off.length() > spike {
                offsets[v] = (mean + off.normalize() * spike).clamp(Vec3::splat(-0.75), Vec3::splat(1.75));
            }
        }
    }
    // which way each vertex faces before the wobble, from the quads round it
    let mut facing = vec![Vec3::ZERO; homes.len()];
    let facing_reach = step();
    let nearest = |q: &[u32]| q.iter().map(|&m| past[m as usize]).min().unwrap_or(i32::MAX);
    for (q, _) in quads.iter().filter(|(q, _)| nearest(q) <= facing_reach) {
        let base = q[0] as usize;
        let [a, b, c, d] = q.map(|m| between(base, m, &offsets));
        // pointing out, against the winding
        let normal = (d - b).cross(c - a);
        for &m in q {
            facing[m as usize] += normal;
        }
    }
    // lumps, and a steady wobble so no edge runs ruler-straight, both in world space so chunks agree
    let origin = grass_tint.origin;
    let cell_at = |v: usize| origin.as_vec3() + (local(0, 0, 0) + homes[v]).as_vec3() * cell_blocks;
    let world = |v: usize, offsets: &[Vec3]| cell_at(v) + offsets[v] * cell_blocks;
    let before = offsets.clone();
    for v in (0..homes.len()).filter(|&v| past[v] <= facing_reach) {
        let at = world(v, &before);
        let look = look(materials[v]);
        if soft(materials[v]) {
            offsets[v] += facing[v].normalize_or_zero() * noise(4, at / k.lump_width) * k.lump_height / cell_blocks;
        }
        if look.wobble > 0.0 {
            let scale = if soft(materials[v]) { 0.3 } else { 0.45 };
            let wobble = Vec3::new(noise(1, at * scale), noise(2, at * scale), noise(3, at * scale));
            offsets[v] += wobble * look.wobble / cell_blocks;
        }
    }

    // each quad split along the diagonal that folds it least, keeping its winding
    let mut triangles: Vec<([u32; 3], bool)> = Vec::new();
    let chisel_reaches: [i32; CHISEL_PASSES] = std::array::from_fn(|_| step());
    let normals_reach = step();
    for (q, owned) in quads.iter().filter(|(q, _)| nearest(q) <= chisel_reaches[0].max(normals_reach)) {
        let base = q[0] as usize;
        let [a, b, c, d] = q.map(|m| between(base, m, &offsets));
        let fold = |n1: Vec3, n2: Vec3| n1.normalize_or_zero().dot(n2.normalize_or_zero());
        let split = if fold((b - a).cross(c - a), (c - a).cross(d - a)) >= fold((b - a).cross(d - a), (c - b).cross(d - b)) {
            [[q[0], q[1], q[2]], [q[0], q[2], q[3]]]
        } else {
            [[q[0], q[1], q[3]], [q[1], q[2], q[3]]]
        };
        for t in split {
            triangles.push((t, *owned));
        }
    }

    // chisel rock: snap its triangles' normals to a few noise-turned directions and pull its vertices onto those planes
    if k.chisel_round > 0.0 {
        let mut around: Vec<Vec<usize>> = vec![Vec::new(); homes.len()];
        for (f, (t, _)) in triangles.iter().enumerate() {
            for &m in t {
                around[m as usize].push(f);
            }
        }
        let (round, up) = (std::f32::consts::TAU / k.chisel_round, std::f32::consts::FRAC_PI_2 / k.chisel_up.max(1.0));
        let snap = |normal: Vec3, base: usize| {
            let turn = noise(7, cell_at(base) / 9.0) * std::f32::consts::PI;
            let yaw = soft_snap((libm::atan2f(normal.z, normal.x) - turn) / round) * round + turn;
            let pitch = soft_snap(libm::asinf(normal.y.clamp(-1.0, 1.0)) / up) * up;
            Vec3::new(libm::cosf(yaw) * libm::cosf(pitch), libm::sinf(pitch), libm::sinf(yaw) * libm::cosf(pitch))
        };
        for reach in chisel_reaches {
            let before = offsets.clone();
            for v in (0..homes.len()).filter(|&v| past[v] <= reach && !soft(materials[v]) && !around[v].is_empty()) {
                let mut moved = Vec3::ZERO;
                for &f in &around[v] {
                    let t = triangles[f].0;
                    let [a, b, c] = t.map(|m| between(v, m, &before) - before[v]);
                    let normal = (c - a).cross(b - a).normalize_or_zero();
                    if normal == Vec3::ZERO {
                        continue;
                    }
                    let planed = snap(normal, t[0] as usize);
                    moved += planed * planed.dot((a + b + c) / 3.0);
                }
                let pull = moved / around[v].len() as f32 * CHISEL_STEP * rocky[v];
                offsets[v] = (before[v] + pull).clamp(Vec3::splat(-0.3), Vec3::splat(1.3));
            }
        }
    }
    // a node's edge vertices sit on its sides, so its surface meets a neighbour's there with only a step, which the skirt hides
    let boundary = |c: i32| c == margin - 1 || c == edge;
    if lod {
        for (offset, home) in offsets.iter_mut().zip(&homes) {
            if boundary(home.x) {
                offset.x = 0.5;
            }
            if boundary(home.z) {
                offset.z = 0.5;
            }
        }
    }
    // held inside its cell across each axis a crossing runs along, a vertex can't fold a triangle; held only where a quad would fold, a round at a time as holding one can fold its neighbour, since held everywhere smooth slopes read as terraces
    const INSET: f32 = 1.0 / 64.0;
    const HOLD_ROUNDS: usize = 4;
    let hold = |offset: &mut Vec3, axes: u8| {
        for axis in (0..3).filter(|a| axes >> a & 1 == 1) {
            for other in [(axis + 1) % 3, (axis + 2) % 3] {
                offset[other] = offset[other].clamp(INSET, 1.0 - INSET);
            }
        }
    };
    let considered: Vec<usize> = (0..quads.len()).filter(|&qi| nearest(&quads[qi].0) <= chisel_reaches[0].max(normals_reach)).collect();
    let sound_quad = |qi: usize, offsets: &[Vec3]| {
        let q = quads[qi].0;
        let base = q[0] as usize;
        let [a, b, c, d] = q.map(|m| between(base, m, offsets));
        let faces = |t: [Vec3; 3]| (t[2] - t[0]).cross(t[1] - t[0]).dot(quad_out[qi]) > 0.0;
        (faces([a, b, c]) && faces([a, c, d])) || (faces([a, b, d]) && faces([b, c, d]))
    };
    let mut held = vec![false; offsets.len()];
    for round in 0..=HOLD_ROUNDS {
        let unsound: Vec<usize> = considered.iter().copied().filter(|&qi| !sound_quad(qi, &offsets)).collect();
        let mut moved = false;
        for qi in unsound {
            for &m in &quads[qi].0 {
                let m = m as usize;
                // the last round holds every vertex of a quad still folding, which always makes it sound
                if !held[m] || round == HOLD_ROUNDS {
                    held[m] = true;
                    hold(&mut offsets[m], crossing_axes[m]);
                    moved = true;
                }
            }
        }
        if !moved {
            break;
        }
    }
    // split again from the final positions, along a diagonal whose triangles both face from solid to air
    let mut triangles: Vec<([u32; 3], bool)> = Vec::new();
    let mut triangles_below: Vec<bool> = Vec::new();
    for (qi, (q, owned)) in quads.iter().enumerate().filter(|(_, (q, _))| nearest(q) <= chisel_reaches[0].max(normals_reach)) {
        let base = q[0] as usize;
        let [a, b, c, d] = q.map(|m| between(base, m, &offsets));
        let faces = |t: [Vec3; 3]| (t[2] - t[0]).cross(t[1] - t[0]).dot(quad_out[qi]) > 0.0;
        let diagonals = [([q[0], q[1], q[2]], [q[0], q[2], q[3]], [[a, b, c], [a, c, d]]), ([q[0], q[1], q[3]], [q[1], q[2], q[3]], [[a, b, d], [b, c, d]])];
        let fold = |n1: Vec3, n2: Vec3| n1.normalize_or_zero().dot(n2.normalize_or_zero());
        let flatness = [fold((b - a).cross(c - a), (c - a).cross(d - a)), fold((b - a).cross(d - a), (c - b).cross(d - b))];
        let sound = diagonals.map(|(_, _, [t1, t2])| faces(t1) && faces(t2));
        #[cfg(test)]
        if !sound.contains(&true) && nearest(q) == 0 {
            UNSOUND_QUADS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        let pick = match sound {
            [true, false] => 0,
            [false, true] => 1,
            _ => (flatness[1] > flatness[0]) as usize,
        };
        triangles.push((diagonals[pick].0, *owned));
        triangles.push((diagonals[pick].1, *owned));
        triangles_below.extend([below[qi]; 2]);
    }
    let mut smooth_normals = vec![Vec3::ZERO; homes.len()];
    for (t, _) in triangles.iter().filter(|(t, _)| nearest(t) <= normals_reach) {
        let base = t[0] as usize;
        let [p0, p1, p2] = t.map(|m| between(base, m, &offsets));
        // pointing out, against the winding
        let normal = (p2 - p0).cross(p1 - p0);
        for &m in t {
            smooth_normals[m as usize] += normal;
        }
    }
    let sample = |p: Vec3| {
        let p = p.clamp(Vec3::ZERO, Vec3::splat(n as f32 - 1.001));
        let (f, t) = (p.floor(), p - p.floor());
        let (i, j, kk) = (f.x as usize, f.y as usize, f.z as usize);
        let at = |a: usize, b: usize, c: usize| blurred(i + a, j + b, kk + c);
        let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
        let face = |c: usize| lerp(lerp(at(0, 0, c), at(1, 0, c), t.x), lerp(at(0, 1, c), at(1, 1, c), t.x), t.y);
        lerp(face(0), face(1), t.z)
    };

    // vertices shared per material pair: a triangle draws its commonest material and blends to the other, see `world.wesl`
    let tile_for = |m: BlockType, v: usize| {
        tile_of(source, m, if m == BlockType::Grass { Face::Top } else { Face::Side }, homes[v] + local(0, 0, 0))
    };
    // each vertex's copies so far, by the pair of tiles they're drawn in
    let mut shared: Vec<Vec<(u8, u8, u32)>> = vec![Vec::new(); homes.len()];
    // a neighbour's triangles just past the edge, whose vertices every step still reached, so they're where it draws them
    if !lod {
        let at = |v: usize| (homes[v] + local(0, 0, 0)).as_vec3() + (offsets[v] * 4096.0).round() / 4096.0;
        beside.extend(triangles.iter().filter(|(t, owned)| !owned && t.iter().all(|&m| past[m as usize] <= 2)).map(|(t, _)| t.map(|m| at(m as usize))));
    }
    // the row past a node's bottom drawn last, so its skirts can be left to the node below, which hangs the same ones
    let mut past_bottom = usize::MAX;
    let order = (0..triangles.len()).filter(|&t| !triangles_below[t]).chain((0..triangles.len()).filter(|&t| triangles_below[t]));
    for i in order {
        let (t, owned) = &triangles[i];
        if !owned {
            continue;
        }
        if triangles_below[i] && past_bottom == usize::MAX {
            past_bottom = buffers.indices.len();
        }
        let [m0, m1, m2] = t.map(|m| materials[m as usize]);
        let main = if m1 == m2 { m1 } else { m0 };
        let other = [m0, m1, m2].into_iter().find(|&m| m != main).unwrap_or(main);
        let (main_tile, other_tile) = (tile_for(main, t[0] as usize), tile_for(other, t[0] as usize));
        // grass's painting is grey and takes its colour from the tint, so where it blends every vertex carries grass's
        let tinted = [(main, main_tile), (other, other_tile)]
            .into_iter()
            .find(|(_, tile)| main != other && tile.tinted())
            .map(|(m, _)| m);
        for &m in t {
            let v = m as usize;
            let pair = (main_tile as u8, other_tile as u8);
            if let Some(&(_, _, index)) = shared[v].iter().find(|&&(a, b, _)| (a, b) == pair) {
                buffers.indices.push(index);
                continue;
            }
            let index = {
                // on a 1/4096 grid, so the sum is exact and a neighbouring chunk's copy lands on the same point
                let snapped = (offsets[v] * 4096.0).round() / 4096.0;
                let normal = smooth_normals[v].normalize_or_zero();
                // straight down, so edge vertices stay on the node's sides where its skirts hang
                let sunk = if lod { Vec3::Y * (LOD_SINK / cell_blocks) } else { Vec3::ZERO };
                // the row past a node's bottom redraws the top row of the node below, so it leans in behind it from its own row down
                let behind = if lod && homes[v].y == margin - 2 { normal * ROW_LEAN } else { Vec3::ZERO };
                let position = (homes[v] + local(0, 0, 0)).as_vec3() + snapped - sunk - behind;
                let open = sample(homes[v].as_vec3() + offsets[v] + normal * AO_OUT);
                // the blocks round it, and the lie of the land over hundreds of blocks
                let occluded = if soft(materials[v]) {
                    ((open - 0.25).max(0.0) * k.ao).min(0.65)
                } else {
                    // rock starts to darken at a shallower crease, and eases off, so deep ones stay lighter
                    let excess = (open - ROCK_AO_FROM).max(0.0);
                    ROCK_AO_MAX * excess / (excess + ROCK_AO_KNEE)
                };
                let ao = ((1.0 - occluded) * grass_tint.shade_at(position)).clamp(0.2, 1.0);
                // weight toward the second material, plus 2 if the second takes the tint or 4 if the first does
                let toward_other = match (soft(main), soft(other)) {
                    (true, false) => rockiness[v],
                    (false, true) => 1.0 - rockiness[v],
                    _ => (materials[v] == other && other != main) as u8 as f32,
                };
                let tinted_side = match tinted {
                    Some(m) if m == other => 2.0,
                    Some(_) => 4.0,
                    None => 0.0,
                };
                buffers.vertices.push(Vertex {
                    position: position.to_array(),
                    normal: stored_normal(normal),
                    uv: [TRIPLANAR - other_tile.atlas_cell(), toward_other + tinted_side],
                });
                // tinted as its own material, or the blend's tinted one, drawn in the triangle's
                let tint_by = tinted.unwrap_or(materials[v]);
                let [r, g, b, _] = colour(source, grass_tint, tint_by, tile_for(tint_by, v), position);
                // the tint alone, as only one side of a blend takes it, and the occlusion in the cell's spare fraction
                buffers.colors.push([r, g, b, main_tile.atlas_cell() + (ao - AO_MID) / AO_SPREAD]);
                buffers.lights.push(lights[v]);
                buffers.blends.resize(buffers.vertices.len() - 1, 0.0);
                let facet = if soft(materials[v]) { k.soft.facet } else { k.soft.facet + (k.rock.facet - k.soft.facet) * rocky[v] };
                buffers.blends.push(facet * FACET_SPAN);
                buffers.vertices.len() as u32 - 1
            };
            shared[v].push((pair.0, pair.1, index));
            buffers.indices.push(index);
        }
    }
    past_bottom.min(buffers.indices.len())
}

// Hangs a wall into the ground from each open edge along a node's sides, of the surface from index `start` up to the row past its bottom at `past_bottom`, see docs/sculpted-ground.md.
fn hang_skirts(source: &impl BlockSource, buffers: &mut MeshBuffers, start: usize, past_bottom: usize) {
    let side = |c: f32| c == -0.5 || c == SIZE as f32 - 0.5;
    let at = |i: u32| buffers.vertices[i as usize].position;
    let key = |i: u32| at(i).map(f32::to_bits);
    let edges: Vec<(u32, u32)> = buffers.indices[start..].chunks(3).flat_map(|t| (0..3).map(move |e| (t[e], t[(e + 1) % 3]))).collect();
    // by position, as a vertex is drawn once for each pair of materials round it
    let drawn: std::collections::HashSet<_> = edges.iter().map(|&(a, b)| (key(a), key(b))).collect();
    // how far, and which way, a vertex's skirt runs: into the ground, in the plane of the side across `axis`, never up
    let reach = |top: u32, axis: usize| {
        let (p, n) = (at(top), buffers.vertices[top as usize].normal);
        // undoes `stored_normal`
        let mut inward = Vec3::new(-n[0], -n[1], n[2]);
        inward[axis] = 0.0;
        let inward = if inward.length_squared() > 1e-6 { inward.normalize() } else { Vec3::NEG_Y };
        let depth = source.skirt_cells((p[0].round() as i32).clamp(0, SIZE - 1), (p[2].round() as i32).clamp(0, SIZE - 1));
        let mut reach = Vec3::new(inward.x, inward.y.min(0.0) * depth as f32, inward.z);
        // leaning into the node, off the side's plane, where the neighbour's skirt and the walls it draws there lie too
        let into = if p[axis] < 0.0 { 1.0 } else { -1.0 };
        reach[axis] = into * reach.length() * SKIRT_LEAN;
        reach
    };
    // the row past the bottom redraws the node below's top row, which hangs these skirts itself
    let skirts: Vec<(u32, u32, Vec3, Vec3)> = edges
        .into_iter()
        .take(past_bottom - start)
        .filter_map(|(top_a, top_b)| {
            let (a, b) = (at(top_a), at(top_b));
            let axis = [0, 2].into_iter().find(|&axis| side(a[axis]) && a[axis] == b[axis])?;
            let (reach_a, reach_b) = (reach(top_a, axis), reach(top_b, axis));
            (reach_a != Vec3::ZERO && !drawn.contains(&(key(top_b), key(top_a)))).then_some((top_a, top_b, reach_a, reach_b))
        })
        .collect();
    for (top_a, top_b, reach_a, reach_b) in skirts {
        let [low_a, low_b] = [(top_a, reach_a), (top_b, reach_b)].map(|(top, reach)| {
            let top = top as usize;
            let mut vertex = buffers.vertices[top];
            // within the node's own sides, as the neighbour along this side hangs its own skirt in the same plane past them
            let low = Vec3::from(vertex.position) + reach;
            let (inside, far) = (-0.5, SIZE as f32 - 0.5);
            vertex.position = [low.x.clamp(inside, far), low.y, low.z.clamp(inside, far)];
            buffers.vertices.push(vertex);
            buffers.colors.push(buffers.colors[top]);
            buffers.lights.push(buffers.lights[top]);
            buffers.blends.push(buffers.blends[top]);
            buffers.vertices.len() as u32 - 1
        });
        // wound as the surface it carries on from
        buffers.indices.extend([top_b, top_a, low_a, top_b, low_a, low_b]);
    }
}

#[cfg(test)]
mod tests {
    use super::soft_snap;

    #[test]
    fn test_soft_snap_holds_near_whole_numbers_and_slides_without_a_jump() {
        assert_eq!(soft_snap(2.1), 2.0);
        assert_eq!(soft_snap(-0.8), -1.0);
        assert!((soft_snap(1.5) - 1.5).abs() < 1e-5);
        // no step anywhere: neighbouring inputs give neighbouring outputs
        let mut x = -3.0f32;
        while x < 3.0 {
            assert!((soft_snap(x + 1e-3) - soft_snap(x)).abs() < 0.01, "jump at {x}");
            x += 1e-3;
        }
    }
}
