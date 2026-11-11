use super::buffers::{GrassTint, MeshBuffers, Vertex, BEVEL, RAMP};
use super::greedy::{cuts, shape_occupied, Edge, Shape, Shapes};
use super::shading::face_light;
use super::source::BlockSource;
use crate::generate::light::SkyLight;
use bevy::math::{IVec3, Vec3};
use ochre_base::block::{BlockType, Face};
use std::ops::Deref;

// A top face's outline along one of its edges, `along` the edge from one end to the other and up, for the block at
// `coord` whose corners at the ends are `corners` and whose sides across them are `sides`: each end drops down the
// chamfer there if it has one, to where that meets the block's side.
fn outline(
    shape: Shape,
    top: f32,
    centre: f32,
    corners: [usize; 2],
    sides: [usize; 2],
) -> Vec<(f32, f32)> {
    let height = |k: usize| {
        top + if shape.raised[k] { RAMP } else { 0.0 } - if shape.dipped[k] { BEVEL } else { 0.0 }
    };
    let mut points = Vec::with_capacity(6);
    for (end, (&k, &side)) in corners.iter().zip(&sides).enumerate() {
        let sign = if end == 0 { -1.0 } else { 1.0 };
        let chamfered = shape.edges[side] == Edge::Convex;
        let top_at = centre + sign * (0.5 - if chamfered { BEVEL } else { 0.0 });
        let foot = (centre + sign * 0.5, top - BEVEL);
        // a cut corner has a point a bevel's width in from it, as high as the corner is raised
        let cut = shape.dipped[k].then_some((centre + sign * (0.5 - BEVEL), height(k) + BEVEL));
        if end == 0 && chamfered {
            points.push(foot);
        }
        if end == 1 {
            points.extend(cut);
        }
        points.push((top_at, height(k)));
        if end == 0 {
            points.extend(cut);
        }
        if end == 1 && chamfered {
            points.push(foot);
        }
    }
    points
}

fn close(p: (f32, f32), q: (f32, f32)) -> bool {
    (p.0 - q.0).abs() < 1e-4 && (p.1 - q.1).abs() < 1e-4
}

fn same(a: &[(f32, f32)], b: &[(f32, f32)]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(&p, &q)| close(p, q))
}

fn area(points: &[(f32, f32)]) -> f32 {
    (0..points.len())
        .map(|i| {
            let (p, q) = (points[i], points[(i + 1) % points.len()]);
            p.0 * q.1 - q.0 * p.1
        })
        .sum::<f32>()
        / 2.0
}

// Triangles over a simple polygon, by clipping its ears.
fn triangulate(points: &[(f32, f32)]) -> Vec<[usize; 3]> {
    let mut order: Vec<usize> = (0..points.len()).collect();
    if area(points) < 0.0 {
        order.reverse();
    }
    let cross = |a: usize, b: usize, c: usize| {
        let (a, b, c) = (points[a], points[b], points[c]);
        (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0)
    };
    let mut triangles = Vec::new();
    while order.len() > 3 {
        let n = order.len();
        let ear = (0..n).find(|&i| {
            let (a, b, c) = (order[(i + n - 1) % n], order[i], order[(i + 1) % n]);
            cross(a, b, c) > 1e-7
                && order.iter().all(|&o| {
                    o == a
                        || o == b
                        || o == c
                        || !(cross(a, b, o) > 1e-7
                            && cross(b, c, o) > 1e-7
                            && cross(c, a, o) > 1e-7)
                })
        });
        // a corner in line with its neighbours has no area to give
        let i = ear.unwrap_or_else(|| {
            (0..n)
                .min_by(|&x, &y| {
                    let at = |i: usize| {
                        cross(order[(i + n - 1) % n], order[i], order[(i + 1) % n]).abs()
                    };
                    at(x).total_cmp(&at(y))
                })
                .unwrap()
        });
        let (a, b, c) = (order[(i + n - 1) % n], order[i], order[(i + 1) % n]);
        if cross(a, b, c) > 1e-7 {
            triangles.push([a, b, c]);
        }
        order.remove(i);
    }
    if order.len() == 3 && cross(order[0], order[1], order[2]) > 1e-7 {
        triangles.push([order[0], order[1], order[2]]);
    }
    triangles
}

// Where two segments cross, as the point.
fn crossing(p: (f32, f32), q: (f32, f32), r: (f32, f32), s: (f32, f32)) -> Option<(f32, f32)> {
    let (d1, d2) = ((q.0 - p.0, q.1 - p.1), (s.0 - r.0, s.1 - r.1));
    let denominator = d1.0 * d2.1 - d1.1 * d2.0;
    if denominator.abs() < 1e-9 {
        return None;
    }
    let offset = (r.0 - p.0, r.1 - p.1);
    let t = (offset.0 * d2.1 - offset.1 * d2.0) / denominator;
    let u = (offset.0 * d1.1 - offset.1 * d1.0) / denominator;
    let eps = 1e-4;
    (t > eps && t < 1.0 - eps && u > eps && u < 1.0 - eps)
        .then_some((p.0 + d1.0 * t, p.1 + d1.1 * t))
}

// The regions between two outlines that start and end over the same stretch, split wherever they cross or meet so
// each is a simple polygon: the near outline along and the far one back.
fn regions(mut near: Vec<(f32, f32)>, mut far: Vec<(f32, f32)>) -> Vec<Vec<(f32, f32)>> {
    // each outline's points that lie along a segment of the other are put into it
    let weld = |chain: &mut Vec<(f32, f32)>, others: &[(f32, f32)]| {
        for &point in others {
            for i in 0..chain.len() - 1 {
                let (p, q) = (chain[i], chain[i + 1]);
                let (along, off) = ((q.0 - p.0, q.1 - p.1), (point.0 - p.0, point.1 - p.1));
                let length_squared = along.0 * along.0 + along.1 * along.1;
                let t = (off.0 * along.0 + off.1 * along.1) / length_squared.max(1e-12);
                let line =
                    (off.0 * along.1 - off.1 * along.0).abs() / length_squared.sqrt().max(1e-6);
                if t > 1e-3 && t < 1.0 - 1e-3 && line < 1e-4 {
                    chain.insert(i + 1, point);
                    break;
                }
            }
        }
    };
    let others = far.clone();
    weld(&mut near, &others);
    let others = near.clone();
    weld(&mut far, &others);
    // put each crossing into both
    'crossings: loop {
        for i in 0..near.len() - 1 {
            for j in 0..far.len() - 1 {
                if let Some(point) = crossing(near[i], near[i + 1], far[j], far[j + 1]) {
                    near.insert(i + 1, point);
                    far.insert(j + 1, point);
                    continue 'crossings;
                }
            }
        }
        break;
    }
    // the places they share, in order along both, and the ends, which are joined by a drop if they differ
    let mut joins = vec![(0, 0)];
    for (i, &p) in near.iter().enumerate() {
        let last = *joins.last().unwrap();
        if let Some(j) = (last.1..far.len()).find(|&j| close(far[j], p)) {
            if i > last.0 || j > last.1 {
                joins.push((i, j));
            }
        }
    }
    let end = (near.len() - 1, far.len() - 1);
    if *joins.last().unwrap() != end {
        joins.push(end);
    }
    joins
        .windows(2)
        .map(|pair| {
            let ((i0, j0), (i1, j1)) = (pair[0], pair[1]);
            let mut polygon: Vec<(f32, f32)> = near[i0..=i1].to_vec();
            polygon.extend(far[j0..=j1].iter().rev());
            polygon.dedup_by(|p, q| close(*p, *q));
            if polygon.len() > 1 && close(polygon[0], polygon[polygon.len() - 1]) {
                polygon.pop();
            }
            polygon
        })
        .collect()
}

// Where a level top meets another, or a taller block that's a plain wall, and their outlines along the edge between
// them differ, as a chamfer ends against one that has none or a ramp comes down to level ground, the gap between is
// closed with a face. It's done for each edge by the chunk with the lower of the two blocks along it, so a seam
// between chunks has just the one.
pub(super) fn mesh_seams(
    non_air: &[(IVec3, BlockType)],
    buffers: &mut MeshBuffers,
    grass_tint: &GrassTint,
    source: &impl BlockSource,
    light: &impl Deref<Target = SkyLight>,
    shapes: &Shapes,
) {
    let opaque = |c: IVec3| shape_occupied(source.block(c));
    let top = |c: IVec3| opaque(c) && !opaque(c + IVec3::Y);
    for &(coord, block) in non_air {
        if !top(coord) || !cuts(block) {
            continue;
        }
        // each way along an axis, and the side of the top that is
        for (direction, axis, side) in [
            (IVec3::X, 0usize, 1usize),
            (-IVec3::X, 0, 0),
            (IVec3::Z, 1, 3),
            (-IVec3::Z, 1, 2),
        ] {
            let beside = coord + direction;
            let positive = direction.max_element() > 0;
            let other = source.block(beside);
            let wall = opaque(beside) && opaque(beside + IVec3::Y) && other != BlockType::Leaves;
            if !wall && !(top(beside) && cuts(other) && positive) {
                continue;
            }
            let own = shapes.shape(source, coord);
            // a wall of the same block is met by a ramp, see `ramp_corners`
            if wall && own.edges[side] == Edge::Concave {
                continue;
            }
            // the corners at the ends of the edge, and the sides across it, along it from -z (or -x) to +z (or +x)
            let (lower, upper, sides, centre) = if axis == 0 {
                ([1, 3], [0, 2], [2, 3], coord.z as f32)
            } else {
                ([2, 3], [0, 1], [0, 1], coord.x as f32)
            };
            let level = coord.y as f32 + 0.5;
            let plain = vec![(centre - 0.5, level), (centre + 0.5, level)];
            let (lo, hi) = if positive {
                (coord, beside)
            } else {
                (beside, coord)
            };
            let outline_of = |cell: IVec3, corners: [usize; 2]| {
                if cell == coord {
                    outline(own, level, centre, corners, sides)
                } else if wall {
                    plain.clone()
                } else {
                    outline(shapes.shape(source, cell), level, centre, corners, sides)
                }
            };
            let near = outline_of(lo, lower);
            let far = outline_of(hi, upper);
            if same(&near, &far) {
                continue;
            }
            let (lo_block, hi_block) = if positive {
                (block, other)
            } else {
                (other, block)
            };
            let along = if axis == 0 { IVec3::X } else { IVec3::Z };
            let plane = if axis == 0 {
                lo.x as f32 + 0.5
            } else {
                lo.z as f32 + 0.5
            };
            let at = |p: (f32, f32)| {
                if axis == 0 {
                    Vec3::new(plane, p.1, p.0)
                } else {
                    Vec3::new(p.0, p.1, plane)
                }
            };
            for polygon in regions(near, far) {
                let signed = area(&polygon);
                if polygon.len() < 3 || signed.abs() < 1e-6 {
                    continue;
                }
                // above the near outline, the gap is a face of the block beyond, looking back along; below, the
                // near block's, looking on
                let (owner, cell, facing) = if signed > 0.0 {
                    (hi_block, hi, -along)
                } else {
                    (lo_block, lo, along)
                };
                let outward = facing.as_vec3();
                let tile = owner.tile(Face::Side);
                let lit = face_light(light.level(cell + IVec3::Y));
                for [a, b, c] in triangulate(&polygon) {
                    let mut corners = [at(polygon[a]), at(polygon[b]), at(polygon[c])];
                    // the cube's faces wind with the cross product pointing in
                    if (corners[1] - corners[0])
                        .cross(corners[2] - corners[0])
                        .dot(outward)
                        > 0.0
                    {
                        corners.swap(1, 2);
                    }
                    let start = buffers.vertices.len() as u32;
                    for position in corners {
                        let tint = if tile.tinted() {
                            if owner == BlockType::Grass {
                                grass_tint.ground_at(position)
                            } else {
                                grass_tint.at(position)
                            }
                        } else {
                            Vec3::ONE
                        };
                        let shade = match owner {
                            BlockType::Grass => source.forest_shade(position),
                            _ => 1.0,
                        };
                        buffers.vertices.push(Vertex {
                            position: position.to_array(),
                            // the cube's faces along z store their normals reversed
                            normal: [outward.x, outward.y, -outward.z],
                            uv: [position.x + position.z, -position.y],
                        });
                        buffers
                            .colors
                            .push((tint * shade).extend(tile.atlas_cell()).to_array());
                        buffers.lights.push(lit);
                    }
                    buffers.indices.extend([start, start + 1, start + 2]);
                }
            }
        }
    }
}
