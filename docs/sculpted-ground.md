# Sculpted ground

The ground is drawn as one sculpted surface, in full-detail chunks and coarse LOD nodes alike: grass and loose ground lumpy and smooth, rock chiselled into broad facets, sharing vertices so one runs into the other with no wall between. Leaves, logs and water draw as cubes. The code is `generate/generator/nets.rs`, and the shader side is in `world.wesl` and `block_texture.wesl`. The cube ground it replaced, with its bevels, ramps and seams, is in the history before this was made the only ground.

## The mesh

Ground is any opaque block but leaves and logs. Each cell between eight block centres that the surface crosses gets one vertex (surface nets), and each crossing between two block centres a quad of the four cells round it. Where two blocks touch only along an edge, the mesh fills the gap between them, as one vertex would otherwise serve two surfaces and pinch.

A vertex takes its cell's material, the highest solid corner with air over it, and is placed by it:

- Loose ground (grass, sand, dirt, snow, gravel, clay) starts at its crossings' mean and is relaxed toward its neighbours.
- Rock sits where its crossings' planes meet (dual contouring), each plane facing out of the blocks counted over 2 each way. A chisel pass then snaps rock triangles' normals to 6 directions round and steps of 45 degrees up, turned by slow noise, and pulls rock vertices onto those planes, so neighbours that snap alike flatten into one face with a crease where they don't.
- No vertex stands more than a set height over its neighbours' mean, and a soft vertex at a lone block's tip or a lone hole's drops onto its neighbours, so a lone block isn't a pyramid.
- Loose ground then lumps, about 6 blocks across and up to 0.9 high, along its normal, and wobbles a little in world space.

Each material's look (relax, wobble, facet) and the other values are constants in `KNOBS`.

Quads split along the diagonal that folds them least. Normals are smooth. Rock's facets are cut in the fragment shader from screen-space derivatives, so vertices are shared, not split per facet.

## Chunk edges

A chunk owns the quads from its own blocks to the next along each axis, and its edge vertices must come out bit for bit as its neighbour's do.

- Every step that reads a vertex's neighbours takes any error from where the data runs out a cell further in. `NEIGHBOUR_STEPS` counts them. Vertices exist only within `VERTEX_REACH` cells of the chunk, and each step works only as far out as the chunk's own vertices still need. A const assertion keeps the reach within the 16 blocks `ChunkNeighbourhood` holds, so adding a pass fails to compile until something else gives.
- A vertex is kept as its cell plus an offset, and neighbour sums are taken relative to the vertex, so both chunks add the same numbers in the same order. Block counts are whole numbers. Offsets snap to 1/4096 before the cell is added, so the sum is exact.
- Trig in the chisel comes from `libm`, so a chunk meshed on another machine still meets its neighbour.
- An edit reaches `EDIT_REACH` blocks of mesh each way, so `remesh` meshes every chunk that far from a changed block again, not only its neighbours.

## LOD nodes

A node runs the same passes over its cells (`mesh_nets` with the cell size), with no exact edge to keep, so it reads only its padded ring, repeated 4 cells out (`LOD_REACH`) for the counts round its edge, and every vertex takes every step. Lump and wobble lengths stay in blocks, so lumps stay 6 blocks across, under a cell from 8-block cells, and the noise is read in world blocks.

- A coarse cell is solid however little ground it holds, so a top crossing drops to the ground's real top (`surface_top`), no lower than the cell's middle.
- Its edge vertices sit on its sides, so beside a neighbour or a chunk only a step is left. Each open edge along a side hangs a skirt into the ground in the side's plane, down `skirt_cells` and a cell in from a cliff, never up. From inside the full-detail area, where the eye is, a chunk's edge that stands higher or further out than the node's shows nothing behind it.
- Stacked nodes' vertices on their shared row don't agree, so each node draws a row past its bottom, under the top of the node below.

## The shader

Sculpted vertices carry what the shader needs in attributes they don't otherwise use:

| Where | What |
|---|---|
| `uv.x` | -1024 less the second material's atlas cell, which marks the vertex as sculpted |
| `uv.y` | the weight toward the second material, plus 2 if the second takes the tint or 4 if the first does |
| colour rgb | the tint alone |
| colour alpha | the main material's cell, with occlusion (0.2 to 1) in the fraction every reader rounds away |
| `uv_b.y` | the facet amount (0 to 1) times 0.45, under the 0.5 that marks coarse cells; plus 1 in an untextured node (cells from 8 blocks), whose mode marks it coarse, so the facet is the fraction |

The fragment shader projects each material three ways, or takes its mean colour in an untextured node, blended by the normal, mixes to the second material along a noise-ragged edge, tints only the side that takes the tint, and applies the occlusion. Occlusion comes from the counted blocks 1.5 out along each vertex's normal, so creases and hollows darken without printing block outlines.

## What meets the ground

Each chunk group keeps its sculpted triangles, bucketed by column, as a `Floor` (`floor.rs`). It includes triangles a neighbour owns beside the chunk, so a column on a chunk's edge or top row sees all its ground.

- Plants root on the highest upward face within 2 blocks below and 0.6 above their block's foot, and lean half way to its slope. Trunks sink a block into the ground.
- The player (`contact.rs`) walks the floor with three spheres. Faces steeper than 66 degrees are walls, which stop the player only above a 1-block step unless the ground on top is walkable. Ground comes from a footprint of five height lookups, and a fall lands on any upward face. A move is cut into 0.2-block substeps, and the head checks the faces above it before rising. Without sculpted ground near, it falls back to blocks.
- Digging is a brush (`breaking.rs`): it removes blocks within 1.5 of the hit on loose ground, or 1 on rock, a rough 0.3 deep, so a dig doesn't print one block's outline in the surface.
- A water surface reaches up to 1.5 blocks over the shore and is cut along the floor in 4 steps a block, so it meets the bank instead of stopping at block edges.
- Generation lays sand on any bank within 2 blocks of water and a block of its level, lakes and rivers as well as the sea (`columns.rs`), so water never meets a grass edge.

## Checking it

- `test_sculpted_ground_leaves_no_hole` meshes real terrain at 8 spots, as generated and with a pit dug, over 3×3×3 chunks, and finds no open or mis-wound edge in the ground. Trees are drawn open by their own mesher, so edges beside logs and leaves are skipped.
- `test_sculpted_lod_nodes_leave_no_hole` meshes LOD nodes of levels 1 to 4 at 4 spots and finds no open or mis-wound edge in the sculpted ground but at the node's top and bottom and where a skirt ends.
- `worldview <seed> render <x> <z> <half> <out.png>` draws the game's chunk meshes on the CPU, and `OCHRE_RENDER_FLAT=1` shades each triangle flat, as the shader cuts rock. `OCHRE_RENDER_LOD=<level>` draws that level's nodes, and `OCHRE_RENDER_HANDOVER=1` chunks in the node holding the spot, for the seam round it. The renderer culls nothing, so the near side of the seam shows what the eye, inside, never sees.
- `test_a_dig_changes_only_the_sculpted_chunks_meshed_again` digs and checks no chunk outside the remeshed range changes. `contact.rs` tests ledges, sliding along walls, landing, steep slopes and low ceilings.
- bench, seed default: chunk meshing 444 ms against 174 ms for cubes, with 547,972 vertices against 1,249,120. Each chunk does its block work over a 48³ neighbourhood for its edges to match, most of what's left. LOD meshing over levels 1 to 8 takes 98 ms with 76,537 vertices.
- The grid's size is a constant of the margin (`const { }`): worked out from the margin at run time, chunk meshing ran half again as slow.

## Open

- A chunk's edge that lies lower or further in than the node beside it opens a gap seen only from outside the full-detail area.
- Rock's chisel turns over 9 blocks, so from 8-block cells its directions change at every vertex.
- In a blend, the untinted side loses its weathering tint.
- LOD nodes don't read the shore's sand or cut water along the ground, so a shore can change at the hand-over, and trees can grow on shore sand.
- Near-flat banks leave a saw-tooth waterline where the ground lies just under the water.
- Rare holes are still reported in play, not yet found by the tests.
