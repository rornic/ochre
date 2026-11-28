# Sculpted ground

The ground is drawn as one sculpted surface, in full-detail chunks and coarse LOD nodes alike: grass and loose ground lumpy and smooth, rock chiselled into broad facets, sharing vertices so one runs into the other with no wall between. Leaves, logs and water draw as cubes. The code is `generate/generator/nets.rs`, and the shader side is in `world.wesl` and `block_texture.wesl`. The cube ground it replaced, with its bevels, ramps and seams, is in the history before this was made the only ground.

## The mesh

Ground is any opaque block but leaves and logs. Each cell between eight block centres that the surface crosses gets one vertex (surface nets), and each crossing between two block centres a quad of the four cells round it. Where two blocks touch only along an edge, the mesh fills the gap between them, as one vertex would otherwise serve two surfaces and pinch.

A vertex takes its cell's material, the highest solid corner with air over it, and is placed by it:

- Loose ground (grass, sand, dirt, snow, gravel, clay) starts at its crossings' mean and is relaxed toward its neighbours.
- Rock sits where its crossings' planes meet (dual contouring), each plane facing out of the blocks counted over 2 each way. A chisel pass then snaps rock triangles' normals to 6 directions round and steps of 45 degrees up, turned by slow noise, and pulls rock vertices onto those planes, so neighbours that snap alike flatten into one face with a crease where they don't.
- No vertex stands more than a set height over its neighbours' mean, and a soft vertex at a lone block's tip or a lone hole's drops onto its neighbours, so a lone block isn't a pyramid.
- Loose ground then lumps, about 6 blocks across and up to 0.9 high, along its normal, and wobbles a little in world space.
- Last, a vertex of a quad that would fold is held 1/64 inside its cell across each axis a crossing in its cell runs along, a round at a time (holding one can fold the quad beside it), and the last round holds every vertex of a quad still folding, and each quad is split again from the final positions along a diagonal whose two triangles both face from solid to air. A quad's four vertices then lie in the four cells round its crossing, so its outline winds once round it and no triangle can fold. Placement past the cell (0.3), the spike cap (0.75) and the chisel (0.3, and it overshoots) all move vertices out of their cells, and before this about 3,500 of 1.6 million triangles on seed 1's steppe peak folded before the chisel and 10,500 after. Along the crossing's own axis a vertex stays free, so soft ground still lumps along its normal. Held everywhere, as at first, it pulled smooth slopes of soft ground back onto their blocks' staircase, which read as terraces in grass: image roughness over mountain grass on seed 1 at 11150,-6600 was 0.139 held everywhere, 0.113 held only where a quad folds and 0.111 never held.

Each material's look (relax, wobble, facet) and the other values are constants in `KNOBS`.

Quads split along the diagonal that folds them least. Normals are smooth. Rock's facets are cut in the fragment shader from screen-space derivatives, so vertices are shared, not split per facet.

## Chunk edges

A chunk owns the quads from its own blocks to the next along each axis, and its edge vertices must come out bit for bit as its neighbour's do.

- Every step that reads a vertex's neighbours takes any error from where the data runs out a cell further in. `NEIGHBOUR_STEPS` counts them. Vertices exist only within `VERTEX_REACH` cells of the chunk, and each step works only as far out as the chunk's own vertices still need. A const assertion keeps the reach within the 16 blocks `ChunkNeighbourhood` holds, so adding a pass fails to compile until something else gives.
- A vertex is kept as its cell plus an offset, and neighbour sums are taken relative to the vertex, so both chunks add the same numbers in the same order. Block counts are whole numbers. Offsets snap to 1/4096 before the cell is added, so the sum is exact.
- Trig in the chisel comes from `libm`, so a chunk meshed on another machine still meets its neighbour.
- A face is drawn by the chunk of the solid block it covers, as the cube mesher's were, unless that chunk holds no ground at all before the gap fill, when the chunk on its other side draws it. So an all-air chunk draws nothing and the loader can skip it. Owning by the low side instead left walls facing -x or -z and overhang undersides on a chunk boundary to an empty chunk, which was never meshed: holes in big cliffs with chunk-straight edges (`test_the_wall_against_an_empty_chunk_is_drawn_by_the_rock`). Nodes keep the low side. The same holds for the block the gap fill adds between two that touch along an edge, which can stand in a chunk of nothing else: when neither the filled block's chunk nor the one beside it holds ground, the first ground block round it takes the face (`test_a_bridge_between_edge_touching_blocks_in_an_empty_chunk_is_drawn`). Before, a chunk of air owned those faces and was never meshed, a hole of a block or two on seed 1's steppe at (8033, 254, -401), on a chunk corner.
- An edit reaches `EDIT_REACH` blocks of mesh each way, so `remesh` meshes every chunk that far from a changed block again, not only its neighbours.

## LOD nodes

A node runs the same passes over its cells (`mesh_nets` with the cell size), with no exact edge to keep, so it reads only its padded ring, repeated 4 cells out (`LOD_REACH`) for the counts round its edge, and every vertex takes every step. Lump and wobble lengths stay in blocks, so lumps stay 6 blocks across, under a cell from 8-block cells, and the noise is read in world blocks.

- A coarse cell is solid however little ground it holds, so a top crossing drops to the ground's real top (`surface_top`), no lower than the cell's middle.
- Its edge vertices sit on its sides, so beside a neighbour or a chunk only a step is left. Each open edge along a side hangs a skirt into the ground, down `skirt_cells` and a cell in from a cliff, never up, and no further along the side than the node's own corners. A skirt leans into its node by `SKIRT_LEAN` (1/16) of each cell it runs, so it never lies in the side's plane: the neighbour hangs its own skirts there and draws its walls across the side there too, and two surfaces in one plane z-fight, flickering on cliffs in the distance. From inside the full-detail area, where the eye is, a chunk's edge that stands higher or further out than the node's shows nothing behind it.
- Stacked nodes' vertices on their shared row don't agree, so each node draws a row past its bottom, under the top of the node below. That row redraws the lower node's top row, so its lowest vertices sit `ROW_LEAN` (half a cell) in along their normals, behind it, and it hangs no skirts, as the node below hangs the same ones.

## The shader

Sculpted vertices carry what the shader needs in attributes they don't otherwise use:

| Where | What |
|---|---|
| `uv.x` | -1024 less the second material's atlas cell, which marks the vertex as sculpted |
| `uv.y` | the weight toward the second material, plus 2 if the second takes the tint or 4 if the first does |
| colour rgb | the tint alone |
| colour alpha | the main material's cell, with occlusion (0.2 to 1) in the fraction every reader rounds away: soft ground takes up to 0.65 of its light from how enclosed it is (2.2 times the openness over 0.25), rock takes up to 0.55 starting at a shallower crease (0.2) and easing off |
| `uv_b.y` | the facet amount (0 to 1) times 0.45, under the 0.5 that marks coarse cells; plus 1 in an untextured node (cells from 8 blocks), whose mode marks it coarse, so the facet is the fraction |

The fragment shader projects each material three ways, or takes its mean colour in an untextured node, blended by the normal, mixes to the second material along a noise-ragged edge, tints only the side that takes the tint, and applies the occlusion. Occlusion comes from the counted blocks 1.5 out along each vertex's normal, so creases and hollows darken without printing block outlines.

## What meets the ground

Each chunk group keeps its sculpted triangles, bucketed by column, as a `Floor` (`floor.rs`). It includes triangles a neighbour owns beside the chunk, so a column on a chunk's edge or top row sees all its ground.

- Plants root on the highest upward face within 2 blocks below and 0.6 above their block's foot, and lean half way to its slope. Trunks sink a block into the ground.
- The player (`contact.rs`) walks the floor with three spheres. Faces steeper than about 45 degrees are walls (a game can lower the limit per place with `MovementEnvironment::walkable`, and the body then walks slower the steeper it stands), which stop the player only above a half-block step unless the ground on top is walkable, and the player slides down them. Grass bares to dirt at the same steepness (`world.wesl`), so what looks like grass can be walked up. Ground comes from a footprint of five height lookups, and a fall lands on any upward face. A move is cut into 0.2-block substeps, and the head checks the faces above it before rising. Without sculpted ground near, it falls back to blocks.
- A water surface reaches up to 1.5 blocks over the shore and is cut along the floor in 4 steps a block, so it meets the bank instead of stopping at block edges.
- Generation lays a bed on any bank within 2 blocks of water and a block of its level (`columns.rs`), so water never meets a grass edge: sand by the sea, and by a lake or river what the ground there is (`terrain::fresh_bed`): mud where grass would grow on flat ground, gravel where it slopes 0.3 or more, gravel for snow, and rock and sand as they are.

## Checking it

- `test_sculpted_ground_leaves_no_hole` meshes real terrain at 8 spots, as generated and with a pit dug, over 3×3×3 chunks, and finds no open or mis-wound edge in the ground. Trees are drawn open by their own mesher, so edges beside logs and leaves are skipped.
- `test_sculpted_ground_never_folds_a_triangle` scrambles real terrain into thin walls, lone blocks and diagonal touches and finds every quad has a sound diagonal. Without the cell constraint it finds 1,164 that have none. An open-edge check cannot see a folded triangle, as every edge of one has a neighbour.
- `test_sculpted_lod_nodes_leave_no_hole` meshes LOD nodes of levels 1 to 4 at 4 spots and finds no open or mis-wound edge in the sculpted ground but at the node's top and bottom and where a skirt ends.
- `worldview <seed> render <x> <z> <half> <out.png>` draws the game's chunk meshes on the CPU, and `OCHRE_RENDER_FLAT=1` shades each triangle flat, as the shader cuts rock. `OCHRE_RENDER_LOD=<level>` draws that level's nodes, and `OCHRE_RENDER_HANDOVER=1` chunks in the node holding the spot, for the seam round it. The renderer culls nothing, so the near side of the seam shows what the eye, inside, never sees (`OCHRE_RENDER_CULL=out` culls back faces). `OCHRE_RENDER_ZFIGHT=<blocks>` counts the pixels where a second triangle, sharing no corner with the front one, lies within that depth of it, paints them magenta and sorts them by whether each lies in a node's side plane; `OCHRE_RENDER_ZFIGHT_SAMPLES=1` prints some pairs with their meshes' origins.
- Z-fighting in nodes, `OCHRE_RENDER_LOD=<level> OCHRE_RENDER_ZFIGHT=0.001 OCHRE_RENDER_CULL=out worldview <seed> render <x> <z> 384`, levels 1, 2 and 3: on seed 4's seracs (-6076, 5236) 22,854, 21,114 and 20,924 pixels (about 5% of those drawn) before skirts leaned, 49, 113 and 106 after; on seed 1's mountains (10816, -6784) 1,748, 1,984 and 3,989 before, 117, 102 and 76 after. What's left is the stacked row near its top, where it meets the row below, and its flat crossings, which lie in one cell row and can't lean.
- Openings, drawn as the game draws (`OCHRE_RENDER_CULL=out`) against the same view in chunks alone as the truth: `OCHRE_RENDER_LOD=0 OCHRE_RENDER_CULL=out OCHRE_RENDER_DUMP=truth.bin worldview <seed> render <x> <z> 200 truth.png <azimuth>`, then the same with `OCHRE_RENDER_LOD=auto OCHRE_RENDER_LAYOUT=4,5,2 OCHRE_RENDER_GAPS=truth.bin` in place of the dump (`OCHRE_RENDER_LIFT=<blocks>` lifts the spot the nodes are chosen round, as when the eye is in the air). It counts the pixels the truth covers that show no ground with ground within 8 pixels either side of them, paints them cyan and says what lies at their two ends (a chunk against a node, a node against another, a mesh against itself) and whether they lie on the far half of the area from the eye, which an eye inside it sees across, or the near half, which only an eye outside sees. `OCHRE_RENDER_GAPS_SAMPLES=1` prints some with the triangles at their ends. Seed 1's mountains (10816, -6784), at azimuths 45, 135, 225 and 315 and 25 degrees down, with the spot on the ground and again 40 blocks over it, of 337,000 to 831,000 pixels covered: a chunk against a node 0 to 1,532 pixels, all but 0 to 37 of them on the near half; a node against a node 4 to 193 and a mesh against itself 0 to 120, some on the far half. On seed 4's seracs (-6076, 5236), 702 to 4,194 of 218,000 to 884,000, most of them one node against another or against itself: slabs of ice thinner than a coarse cell, which the nodes can't hold, are in the truth and not in them, so those are not all seams.
- Nodes of different levels, drawn as the game chooses them round a spot (`OCHRE_RENDER_LOD=auto`, with `OCHRE_RENDER_LAYOUT=<full>,<base>,<scale>` in chunks, say `2,3,2`, to fit every level into the view; the game's is `12,16,2`): between cells of different sizes, chunks beside nodes included, 3 pixels z-fight on seed 1 and 1 on seed 4, of about a million drawn.
- `test_a_dig_changes_only_the_sculpted_chunks_meshed_again` digs and checks no chunk outside the remeshed range changes. `contact.rs` tests ledges, sliding along walls, landing, steep slopes and low ceilings.
- bench, seed default: chunk meshing 444 ms against 174 ms for cubes, with 547,972 vertices against 1,249,120. Each chunk does its block work over a 48³ neighbourhood for its edges to match, most of what's left. LOD meshing over levels 1 to 8 takes 98 ms with 76,537 vertices.
- The grid's size is a constant of the margin (`const { }`): worked out from the margin at run time, chunk meshing ran half again as slow.

## Open

- A chunk's edge that stands higher than the node beside it, whose edge is `LOD_SINK` and its coarse cells under it, opens a slit seen only from outside the full-detail area, see `docs/issues.md`.
- Stacked nodes still draw the flat crossings of their shared row twice, in one cell row, so those can z-fight. Making the two nodes' vertices on that row agree, as chunks' do, would end it and the row past the bottom with it, but a node reads only its padded ring, not the `REACH` of blocks a chunk reads for its edges to agree.
- Rock's chisel turns over 9 blocks, so from 8-block cells its directions change at every vertex.
- In a blend, the untinted side loses its weathering tint.
- LOD nodes don't cut water along the ground, so a waterline can change at the hand-over. Nodes of cells 1 and 2 lay the shore's bed as chunks do; coarser cells are wider than the beach.
- Near-flat banks leave a saw-tooth waterline where the ground lies just under the water.
- The constraint's corner trim was not judged by eye.
- The constraint trims the sharpest corners, which could stand 0.3 past their cell.
