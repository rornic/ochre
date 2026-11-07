# Bevels, ramps and seams

Full-detail chunks cut the cube's hard edges into chamfers and ramps. LOD meshes keep plain cubes (`MeshStyle.bevel` is off). The rules live in `generate/generator/greedy.rs` and `seam.rs`, and every one reads only the blocks round a cell, never another cell's result. That is what keeps neighbouring chunks agreeing without looking at each other's meshes.

## The rules

- **Chamfer.** A top edge with open air beside it and above-beside is cut back `BEVEL` (0.2) and joined by a 45 degree strip, with the wall's top inset to match (`top_edges`, `wall_edges`). Leaves are never cut (`cuts`).
- **Ramp.** Ground beside a taller wall of the same block rises `RAMP` (0.3) at the wall, and the wall's foot is raised to meet it. The rise is set at each corner of a cell and blended across it (`lift_at`), so two pieces that agree on a corner agree along the whole edge.
- **Where a ramp ends.** A ramp runs to the end of its wall, and on only where the wall goes on or the ground turns away, at a chamfer or a second wall (`Shapes::shape`, `wall_foot`). Level ground that goes on with no wall brings the ramp down to meet it. The outside corner of a wall lifts the ground round it (`corner_lifts`).
- **Corner cuts.** The inside corner of a plus is cut down 0.2 so the chamfers beside it join (`corner_dips`). A wall's raised foot comes down to the chamfer's foot where the ground beside it is chamfered (`wall_dips`).
- **Different blocks never ramp.** The crease stays sharp, and a chamfer that ends against it is closed by a seam.

`Shapes` works out each top's rules once per chunk, since faces and seams both ask.

## Seams

Where two level tops, or a top and a taller plain wall, meet along an edge and their outlines along it differ (a chamfer ends, a ramp comes down), `mesh_seams` closes the gap with one polygon face. The outlines are built from the same rules, so nothing is inferred from the mesh. Two tops are done by the chunk with the lower of them along the edge, and a top with a wall by the chunk with the top. Each reads its neighbour's blocks across the chunk edge, so a chunk seam has exactly one face and needs no special case.

## Textures on tilted faces

Painted materials project onto the plane of a face's dominant axis, so a tilted face would stretch the texture, two fifths on a 45 degree chamfer. `material_uv` in `block_texture.wesl` spreads the coordinates along the way a face tilts, about the nearest block boundary, where the flat faces either side are continuous with it. A chamfer strip that rises at its ends, as the ground does where it climbs a wall, is steeper than the nominal chamfer, so the mesh gives each of its triangles vertices and exact coordinates of its own (`unfolded_uv`, flagged by adding `UNFOLDED` to `x`), spread about one boundary for the whole triangle, and still lights it as a chamfer. Ramps and corner cuts are part of a top, lit and projected as one, so they stay stretched by their slope: at most 13% on a ramp, and up to 54% on the few small facets a corner cut makes.

A painted chamfer's texture starts at the top's edge, so it runs straight on from the top face (`unfolded_uv` takes the strip's top-edge vertex as its anchor). Where a top meets a wall, which is the chamfer's foot and the end of a ramp, the plane the texture is projected in changes. So the mesh marks those vertices (`MeshBuffers::set_blend`, in the mode `uv_b.y`, as `BLEND_SPAN` times the share and `BLEND_Z` when the wall faces along z) and `world.wesl` draws them partly in the wall's projection, with a second sample: all of it at a chamfer's foot, and over the last 40% of a ramp's rise. At the join the texture then matches the wall's own.

## Checking it

`open_edges` in `generator/tests.rs` is the oracle, a closed-surface check that joins edges lying along other edges and checks winding. These must stay at no open edges, with no repair pass behind them.

- Every arrangement of heights in a 3x3 of columns, up to symmetry, in stone, grass and a mix (6,201 meshes).
- Rolling layered ground with a surface block of its own in each column.
- Steep ground a block a step, along axes and diagonals.
- Eight chunks meeting at a corner, and every 3x2 of columns across a chunk seam.
- Stretch, by replaying `material_uv` over the 3x3 meshes: none on chamfers, under 1.6 on tops.

A new cut needs its rule on both sides of every edge it touches. Run the 3x3 test first, since it finds a one-sided rule in seconds.

An earlier design found holes after meshing, by chaining unmatched edges into loops and filling them, with a ring of neighbour faces so loops across chunk seams closed. It cost about 100 ms per 1,944 chunks and could not say why a hole was there. It was replaced by the rules above.
