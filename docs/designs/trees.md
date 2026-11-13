# Painterly low-poly trees

*   **Author:** Rory Nickolls
*   **Status:** Draft
*   **Date:** 2026-11-13

--------------------------------------------------------------------------------

## Overview

Trees are the last cubes in the world. Bevels, painted materials, painted foliage and the painterly lighting have moved terrain, shrubs and grass away from the Minecraft look, and `docs/materials.md` still lists "tree crowns are still cubes" as open (`docs/designs/painterly-lighting.md` says the same of trees). This design changes how Broadleaf and Pine trees are drawn, near and far.

A tree stays a set of `Log` and `Leaves` voxels in the grid. Those voxels remain the truth for collision, baked sky light, the dim forest floor and hit-testing. What changes is the mesh:

*   The trunk is a low-poly shape that bows within about half a block of its voxel column, with a base flare and a seeded lean.
*   The crown is painted foliage cards over the `Leaves` voxels, whose cube faces are no longer drawn.
*   Each tree's shape varies with its seed, position and climate.
*   Far trees are redrawn from each tree's shape, so they match the near ones.

Iteration one is the look only. Felling a tree and its falling animation come after it.

## The Problem

Logs and leaves are full voxels, so a tree reads as a stack of boxes under a box-shaped crown. Nothing else in the world looks like that any more, so trees now give the Minecraft feel away. They cannot just be made rounder in the generator, because the generator can only place whole voxels. The fix is in what is drawn.

Two things make this harder than a mesh swap:

*   The mesher treats a log as an opaque cube. It culls the face of the block under it and beside it, bevels against it, and counts it in light and ambient occlusion. If the log cube is not drawn and the bark mesh is thinner than the cell, those faces stay culled and a gap shows around the trunk. The ground under and beside a tree must keep its faces.
*   Leaves are solid, targetable and breakable. If their cubes are hidden, a player under a crown would break unseen leaf blocks one at a time and never reach the trunk.

Trees must also stay cheap. The shadow pass already costs about 2.9 ms in the jungle spawn measurement, 61% of cascade 0 of it leaves and flora (`docs/shadow-perf.md`), and cards add alpha-cut overdraw and shadow casters.

## Goals

*   Broadleaf and Pine trunks drawn as low-poly shapes with a seeded static lean and a base flare. The drawn centre line stays within 0.5 block of the voxel column, and the trunk surface within 0.4 block of the cell's face.
*   Crowns drawn as painted foliage cards over today's `Leaves` voxels, which stay solid. The baked sky light is identical with the new look on and off, so the forest floor is as dim as today.
*   `Leaves` become non-targetable, and so unbreakable (accepted for now), so a ray reaches the trunk.
*   Faces of the ground under and beside a tree log stay visible, so a thin trunk leaves no holes.
*   Variety in silhouette from the tree seed, position and climate (PR 3): lean, lobes, crown lean, card size and tilt.
*   One description of a tree's shape that planting, the look record and the LOD trees all read, so planting and drawing cannot drift apart.
*   Far trees (lone trees in LOD nodes) drawn from the same shape, so they match near trees and stop looking blocky.
*   Performance held, and tuned in every PR:
    *   chunk vertices at most 1.10 times today's in the forest views;
    *   each LOD level's vertices at most 1.10 times today's in the forest views, and LOD mesh build time at most 1.10 times;
    *   no new draw calls, materials or entities (trees are drawn inside the chunk and LOD meshes that exist today);
    *   shadow pass and median frame time within 0.6 ms of the baseline in the worst-case view (the frost shader's measured cost), with any p99 regression recorded with its reason as `AGENTS.md` requires;
    *   every PR logs its numbers in `docs/terrain-perf.md`. A PR that misses a gate turns the knobs (card density, card size, prism sides, cards per LOD level) until it passes, or records why it cannot.
*   A `RendererSettings` switch to turn the new look off, as `frost` has, so it can be compared with cubes until PR 5.
*   Acceptance by the checks listed in the Verification Plan.

## Non-Goals

*   Felling a tree, the falling animation, a record of which logs belong to which tree, and edits that persist.
*   Trunks that lean in the voxel grid. Lean is drawn, within the 0.5 block offset.
*   A new block type for tree logs or for the leaf core.
*   Wind as an input to tree shape, which would need a new `TerrainSource` method across the crate boundary, and live wind animation as geometry. Lean comes from the tree's hash, and PR 3 adds a bias from the temperature and humidity `tree_in_cell` already has.
*   Redrawing the forest canopy mass past `CANOPY_CELL`. Lone far trees are in scope (PR 4).
*   Special shapes for branches and for trunks wider than one block, which are plain prisms for now. A wobble in the trunk and a taper beyond the flare are also left out.
*   New shapes for jungle, jungle giants and savanna. They take the new Broadleaf look now and are reshaped later.
*   Block placement, inventory, seasons and growth.

## Open Questions

None blocks the design. These numbers are starting values, set by the spikes named in the Detailed Implementation Plan:

*   **Card density and size**: the share of surface leaves that get a card, and card size, set by the PR 2 spike against the gates in Goals.
*   **Far tree detail per LOD level**: prism sides and cards per tree at each level, set by the PR 4 spike from today's measured per-tree vertex counts.
*   **Trunk lean cap**: 0.5 block is about 2.4 degrees over 12 blocks and may look too straight. The owner judges it in PR 1 stills, and raising it is traded against the collision overlap in Risks.

## Terms

*   **Chunk**: 16×16×16 blocks. **Group**: a render batch of 2×2×2 chunks whose meshes are merged and drawn as one entity, with a fixed bounding box (`group_aabb()`) used for culling and shadows.
*   **LOD node**: a coarser cube of cells (2, 4, 8 and more blocks to a cell) drawn beyond full-detail range. **Canopy mass**: the forest-level surface LOD draws past `CANOPY_CELL`, instead of single trees.
*   **Trunk column**: the `Log` voxels directly above a tree's base on its trunk axis. **Branch log**: any other `Log` of the tree.
*   **Look record**: the per-tree entry in `TreeTints` the mesher reads by voxel position: tint, base y, trunk height, lean, seed.
*   **Surface leaf**: a `Leaves` voxel with a face neighbour that is air or flora. **Leaf core**: the `Leaves` voxels that are not surface leaves: solid, dimming, drawn as nothing.
*   **Card**: one painted double-sided quad drawn for a surface leaf.
*   **Opaque and cutout**: `opaque` is the mesh buffer a chunk builds. Cards sit in it, and `shadow_mesh::split` separates cutout geometry (leaves and flora, drawn with holes) when groups merge.

## Technical Plan (Architecture)

Decided with the owner: near shapes are **voxel-driven**. The mesher draws trunk segments and leaf cards from the voxels it already walks, and takes no list of trees. Each chunk draws only its own voxels, so a tree that spans chunks needs no ownership rule, and an edit that removes a block removes what was drawn for it on the next remesh. Far trees are tree-driven (see "Far trees").

```text
trees_near (per chunk, already runs)
   |
   |  Tree  ->  TreeTints record: trunk axis, tint   (+ lean direction, lean amount, seed)
   v
ChunkData.trees  --- read by the mesher via TreeTints::at(coord), as the leaf tint is today
   |
   v
mesh job, per chunk, reads Log and Leaves voxels:
   Log voxel     -> prism, bowed along the tree's lean               -> opaque buffer
   surface Leaves-> one painted card leaning out from the crown      -> opaque buffer
   inner Leaves  -> nothing drawn, still solid, still dims the floor
   |
   v
group merge, then shadow_mesh::split sends cards (the Leaves cell) to the cutout and shadow passes
LOD nodes (PR 4): lone trees drawn from the Tree list by lod_trees.rs
```

### What stays the same

*   The voxels in PRs 1 and 2. `Tree::plant` writes the same `Log` and `Leaves`, so collision, baked sky light and the dim forest floor are unchanged. PR 3 changes the crown (lobes, crown lean), so the voxels, the pinned hashes in `determinism.rs` and the LOD boxes change there and the pins are updated. `GENERATOR_VERSION` does not change, because it keys the world-map cache, not meshes.
*   `Log` stays opaque for light.
*   Far trees until PR 4.
*   The frost shader. Cards carry the `Leaves` atlas cell, which `frost_receives` skips, and logs keep receiving frost on up-facing faces.

### Trunk

Each `Log` voxel is drawn as an eight-sided prism instead of a cube.

*   **Where it bows.** The look record holds the tree's base y, its trunk height and a lean vector. A log in the trunk column is drawn along a centre line that is the voxel column plus the lean times the square of its height above the base over the trunk height. The offset is a function of the log's position and the record, with no counting of voxels, so two chunks that share a trunk agree at their border and breaking a log does not move the logs above it. It never exceeds 0.5 block.
*   **Lean.** Direction and amount come from the tree's seed. PR 3 adds a bias from temperature and humidity.
*   **Radius and flare.** The radius is at most 0.4 block and the offset at most 0.5, so the surface stays within 0.4 block past the cell's face on the lean side. The lowest log on non-log ground flares to a full cell, which closes the footprint. A log that is not in the trunk column (a branch, or a log of a trunk wider than one block) is a plain cell-wide prism along its own axis, with no bow or flare.
*   **Light.** A log's own cell has light 0 (it is opaque), so each prism vertex takes the light of the open cells next to it, as `vertex_light` and `face_light` do for faces today.
*   **Bark.** Prisms carry unfolded uv (u around the circumference, v up the trunk, the convention `material_uv` treats as given when `uv.x` is above 512), so the painted bark has no seams where a projection by world position would switch planes.
*   **Ground beside a trunk.** See "Faces around logs".

### Crown

Today each leaf voxel draws cube faces, with the faces between leaves hidden or drawn only near the camera (`InnerLeaves`). They are replaced by cards.

*   **Which voxels.** A surface leaf draws a card. A leaf core voxel draws nothing. A leaf touching only ground, logs and leaves is a core voxel.
*   **The card.** One painted double-sided quad, its size, spin and offset hashed from the voxel's world position and the tree's seed. Broadleaf cards are 1.6 to 2.2 blocks across. Pine cards are 1.0 to 1.5 across and tilt closer to the outward direction, because a pine's tiers are narrow (a wrap radius of about 2) and wide cards would make fat cones. The plane tilts to lean outward along the sum of the voxel's open directions with an upward bias. Density (the share of surface leaves that get a card) starts at 1 and is lowered by the PR 2 spike if the gates need it.
*   **Shading.** Cards light with a pseudo-normal along the outward direction rather than the quad's own, so the painterly lighting shades a crown as one rounded mass and not as flat sheets. This is the point of the crown's look. Their shadow lookup follows the treatment plants have (`docs/materials.md` line 20), checked in PR 2 stills, so a card does not shadow itself. They keep the per-tree leaf tint and the canopy shade, and the sky light baked at the voxel.
*   **Painting.** The existing leaves layer, with uv scaled to the card so one repeat covers one block, so texel density matches today. New card art is a later option.
*   **Buffer and shadows.** Cards go in the chunk's `opaque` buffer (`mesh_blocks` returns `cutout: None`) with the `Leaves` cell in vertex alpha. `shadow_mesh::split` routes them to the cutout and shadow passes when groups merge, and cut-out holes use the same `block_alpha_at` path as now.
*   **Overhang.** A card reaches up to its half-width plus its hashed offset past its voxel (about 1.4 blocks at most), and shrubs already reach 1.3 to 1.8 blocks, while `group_aabb()` is exactly the 2×2 chunk box. One constant, the larger of the two reaches, widens it for both (file 12).

### Faces around logs

`is_opaque()` (light opacity at its maximum) decides today whether a block hides the faces of the blocks next to it, and also how light and AO treat it. For a log those two now differ: it blocks light, but a thin prism hides nothing around it. `BlockType::hides_faces()` splits them. The mesher uses it for culling a neighbour's face, for bevel and ramp shapes, for AO and for the seam pass, so the ground under and beside a trunk keeps the faces it has with no log there, and a tree on a slope leaves no holes. Light keeps using `is_opaque()`. A log placed by a future player would share this rule until placed logs are told apart from tree logs.

### Far trees (LOD)

Lone trees in LOD nodes are drawn from the `Tree` list the node already builds, one owner node per tree as boxes are today, replacing planted cubes (cells below `BOX_CROWN_CELL`) and trunk and crown boxes. Each gets a low-poly trunk on the same lean function as near trees and a crown of a few cards or a coarse hull built from its crown blob and lobes, with its tint. Detail drops by level. The planted leaf cells stay in the node's cell grid so `canopy_tops` and the canopy shade still see leaves, and draw nothing. The forest canopy mass past `CANOPY_CELL` is unchanged. The LOD crown must bound the near card envelope within one cell, or trees pop at the chunk border.

### Interaction

*   `Leaves` are no longer targetable (`is_targetable`), so the ray at a trunk under a crown reaches the trunk, and they are unbreakable too (`is_breakable` is built on it). Accepted for now: a crown cannot be cleared. They stay solid, so the player is stopped at the crown's edge.
*   The block outline is a full cube at the log's voxel and would show a box around a bowed trunk. A `TreeShapes` resource, modelled on `FrostSwitch` (`terrain/frost.rs`, read in `footsteps.rs`) and inserted by the terrain plugin, carries the setting to gameplay. The outline is hidden when the looked-at block is a `Log` and `TreeShapes` is on. This applies to every log, since tree logs cannot be told from placed logs yet, so breaking a log shows no outline until the felling design.

### Switch, performance and what retires

*   `RendererSettings.tree_shapes`, default on, in `assets/settings.toml`, read at startup (a mesh-affecting setting, so it needs a restart, as `solid_leaves` does). Off draws today's cubes near the player, and far trees as they are until PR 4. Leaves are non-targetable either way.
*   The inner-leaves machinery (`InnerLeaves`, `ChunkMeshes::inner_leaves`, `show_inner_leaves`, the `solid_leaves` debug flag's path) stays until PR 5, because the off state and LOD up to PR 4 use it. PR 5 removes it, and the switch with it, so after PR 5 there is no way to compare against cubes.
*   **Near cost.** A card is a double-sided quad (likely 8 vertices in `push_double_sided_quad`'s layout, which the PR 2 spike counts), pushed per surface leaf and not merged, where cube faces merged. A trunk prism is about 32 vertices per log against about 16 for a whole merged column today. The knobs are card density, card size and prism sides. A last-resort knob is one-vertex-per-card billboards (see Alternatives), which needs its own shader path.
*   **Far cost.** Far trees must not add draw calls, materials or entities: they are built into the LOD node meshes that exist. At box levels (cell 8 and up) a tree uses no more vertices than today's trunk and crown boxes, and gains shape (taper, lean, hull) at the same count. At the finer levels (cells 2 and 4) a tree's budget is today's measured per-tree count, which the PR 4 spike measures with `lod_sim`. Cards appear only at the finest levels and hulls beyond, and shadow casters from LOD trees are checked with `shadow_cull`, since LOD cards must not add casters past the shadow range.
*   **Gates and measurement.** The gates are in Goals and are checked per PR: `bench` for chunk and LOD vertex counts, `lod_sim` for LOD build time, the sandbox with `OCHRE_STATS` for shadow pass and frame time (`shadow_cull` prints triangle and draw counts only, and `flight` is headless). Views are fixed in PR 1 and recorded in `docs/terrain-perf.md`: a taiga spot with pines, a temperate broadleaf forest and a jungle spot (jungle giants are the worst case), each with a named seed and coordinates.

## Risks & Mitigations

*   **Near vertex, overdraw and shadow cost (cards and prisms).** Cards do not merge and overdraw behind alpha discard. Mitigation: the PR 2 spike measures it before the crown is finalised, density and card size are knobs, the switch gives an on and off comparison, and the gates in Goals block the PR.
*   **Far tree cost.** Mitigation: no new draw calls or materials, a per-level vertex budget from measured counts, cards only at the finest LOD levels, and `lod_sim` and `shadow_cull` checks in PR 4.
*   **Holes beside trunks.** Mitigation: `hides_faces()` in the culling, bevel, AO and seam checks, with a slope test.
*   **Trunk surface against collision.** The player collides with the whole cell, so the mesh can overlap the player by up to 0.4 block on the lean side and leave a gap on the other. Mitigation: radius at most 0.4 and offset at most 0.5 bound it, and it is accepted. Raising the lean cap is traded against it.
*   **Group bounds.** Cards and shrubs overhang the fixed group box. Mitigation: one constant from the larger reach, derived and tested over the hash range, and the shadow-caster cost of the larger box checked with `shadow_cull`. PR 2 first checks whether shrubs at group edges are already culled early.
*   **Where two crowns merge**, `TreeTints::at` picks the nearest trunk, so a card near the join may take a neighbour's tint or tilt. It already does this for the tint, and lean is at most 0.5 block, so a wrong pick is small.
*   **Edits.** A removed leaf removes its card on the next remesh. Removing a log does not move the logs above it, because the bow is a function of position and the record.
*   **Card shadow acne.** Mitigation: the plant shadow treatment, checked in PR 2 stills.
*   **Switch needs a restart.** A mesh-affecting setting, as `solid_leaves` is.
*   **Unbreakable leaves.** Accepted for now.
*   **Security and scalability.** No data, network or user input is involved. Scalability is the performance budget above.

## Alternatives Considered

### Leaves: a new `LeafCore` block

*   **Pros**: The core is explicit in the data, so light, collision and targeting say what they mean and need no special rule.
*   **Cons**: `BlockType` is a `repr(u8)` enum matched in many places (`name`, `tile`, `lod_tile`, footsteps, `worldprobe`, `seam`, `greedy`, the light flood, collision, shadow and depth passes). The generator would write a different block, so the pinned chunk hashes and every stored map change.
*   **Reason for Rejection**: The existing `Leaves` block already is the core once its cube faces stop being drawn. Revisit when players can place leaves and need to tell them apart.

### Leaves: cards over a hollow crown

*   **Pros**: Fewest voxels to light and collide.
*   **Cons**: The forest floor goes bright and the player walks into canopy.
*   **Reason for Rejection**: The dim forest floor and a crown that stops the player are part of how a forest feels, and a hollow crown loses both.

### Shape source: tree-driven (near trees)

*   **Pros**: Full control of shape, from the same `Tree` the LOD boxes read.
*   **Cons**: Cards and trunk come from a separate record of trees, so a tree that spans chunks needs an ownership and clipping rule, an overhang past the owner's group bounds is culled, and deleted voxels would leave cards floating until felling exists.
*   **Reason for Rejection**: The voxel-driven version follows edits and chunk borders for free and gives the look wanted. Far trees do use a tree-driven shape, because LOD nodes already hold the `Tree` list and an owner rule.

### Shape source: hybrid (the `Tree` list threaded to the mesher, cards on voxels)

*   **Pros**: Edits and chunk-local meshing hold, and trunk and variety follow the tree.
*   **Cons**: The `Tree` list must be threaded through `chunk.rs`, the mesh job and the LOD path.
*   **Reason for Rejection**: Widening the existing `TreeTints` record with base y, height, lean and seed gives the mesher what it needs without threading the list.

### Per-tree entities

*   **Pros**: Each tree is a self-contained mesh that can animate on its own.
*   **Cons**: A draw call per tree, which breaks `shadow_cull` accounting and the 260 fade-variant materials.
*   **Reason for Rejection**: The cost is paid every frame by every forest. A falling tree needs one entity, and that is a later stage.

### Trunk: a voxelised spine with a capped lean of 2 to 3 blocks

*   **Pros**: The spine is the single truth, so what you see is what you hit, and lean can reach 2 to 3 blocks.
*   **Cons**: Changes `Tree::plant`, the voxel output, the pinned hashes, the LOD trunk boxes, `shades`, `crown_boxes`, `test_trees_are_a_trunk_under_big_crowns`, and needs a `STEP_HEIGHT` check so a diagonal log is not stair-climbed.
*   **Reason for Rejection**: A drawn lean within 0.5 block gives the look at a fraction of the change, and PR 3 already updates the pins for the crown. Revisit if the bow looks too straight in PR 1 stills.

### Log identity: a `TreeLog` block, or a per-tree record

*   **Pros**: Separates tree logs from future placed logs, which felling needs.
*   **Cons**: A new block touches every match above; a record needs storing with each chunk. Neither is needed to draw a tree.
*   **Reason for Rejection**: Felling is out of iteration one. Decide this in the felling design.
### Rounded cubes: extend the bevel system to trees

*   **Pros**: Reuses chamfers and ramps, and trees stay in the greedy mesher.
*   **Cons**: A crown of chamfered cubes still has a boxy silhouette, and a trunk of chamfered cubes still has square steps.
*   **Reason for Rejection**: The silhouette is the problem, and bevels do not change it.

### A smoothed hull over the crown

*   **Pros**: Few vertices and a closed mesh.
*   **Cons**: Loses the leaf clusters and painted holes the shrubs and flora have, lights as one solid mass, and needs a surface-extraction step.
*   **Reason for Rejection**: Cards match the shrubs and keep the painterly foliage.

### Vertex-pulled billboards for cards (one vertex per card)

*   **Pros**: Cuts card vertex cost by a factor of 4 to 8.
*   **Cons**: A new vertex shader path, and the depth prepass, shadow pass and cutout split would each need it.
*   **Reason for Rejection**: Not needed until the card cost fails the gate. It is the last-resort knob.

### A trunk bent in the vertex shader

*   **Pros**: A smooth bend on a subdivided trunk.
*   **Cons**: Needs per-tree data in the shader and subdivided trunks.
*   **Reason for Rejection**: The look record on the CPU gives the same offset with no shader change.

### Far trees: leave the LOD as it is

*   **Pros**: No LOD work, and far forests keep today's cost.
*   **Cons**: The mid-distance ring (cells of 2 and 4 blocks) stays cubes, so trees look good near and blocky further out, and chunks swap to cubes at the border. The cube path for logs and leaves in `greedy.rs` must stay.
*   **Reason for Rejection**: The owner wants far trees redone as part of this work, so near and far trees match.

### Far trees: voxel-driven cards and prisms at fine LOD cells

*   **Pros**: One shape source (voxels) at every fine level, so the swap at the chunk border changes less.
*   **Cons**: A card on a 2 to 4 block cell is 3 to 9 blocks wide, so crowns read as big plates unless clamped. LOD vertex counts rise where trees are densest. Boxes and the canopy past cell 8 stay blocky, so far trees remain boxes.
*   **Reason for Rejection**: It does not fix the far trees the owner wants fixed, which are mostly the box levels.

### Do nothing: keep cube trees

*   **Pros**: No work, no cost, and no new behaviour for leaves or logs.
*   **Cons**: Trees remain the last cubes in an otherwise non-cubic world.
*   **Reason for Rejection**: This is the problem the design exists to fix.

--------------------------------------------------------------------------------

## Detailed Implementation Plan

Five PRs against `main`, each titled in lowercase conventional-commit form. PRs 1 to 3 add the near look behind `tree_shapes`. PR 4 redraws far trees. PR 5 removes the old cube path once the owner accepts the look. An issue is opened first (`CONTRIBUTING.md`). No PR is raised until the owner says so.

| PR | Title | What changes on screen |
| --- | --- | --- |
| 1 | `feat: bowed low-poly tree trunks` | Trunks, and the ground beside them keeps its faces. Crowns still cubes. |
| 2 | `feat: painted foliage cards for tree crowns` | Near crowns. Leaves non-targetable. |
| 3 | `feat: tree shape variety from seed and climate` | Lean, lobes and crown lean, card tilt. Crown voxels change, pins updated. |
| 4 | `feat: low-poly far trees` | Lone trees in LOD nodes match the near ones. |
| 5 | `refactor: drop the cube tree path` | Nothing, after acceptance. |

PR 1 fixes the three measurement views and records baselines. PR 2 begins with a throwaway spike that measures cards at several densities against the gates. PR 4 begins with a spike that measures today's per-tree LOD vertex counts and sets the per-level budgets.

### Target Files to Create/Modify

#### 1. `crates/ochre-terrain/src/generate/generator/trees_mesh.rs` (Create)

*   **Type / Scope**: Rust module in the mesher, PRs 1 and 2.
*   **Proposed Changes**: One entry point, `mesh_trees(non_air, buffers, grass_tint, source, light)`, called for chunk meshes only (cell size 1). Everything it pushes goes into the `opaque` buffer (`mesh_blocks` has no cutout buffer, `ChunkMeshes.cutout` is `None`), and `shadow_mesh::split` separates cards by their `Leaves` cell later.
    *   **Trunk (PR 1).** Each `Log` voxel draws an eight-sided prism. A log in a tree's trunk column (looked up through `tree_look`) runs from its cell's bottom to its top along the bowed centre line, which is the voxel column plus a lean offset that grows with height above the tree's base (the fraction of the trunk's height, squared), at most 0.5 block. The lowest log on ground flares to a full cell. A log that is not in a trunk column (a branch, or a log of a trunk wider than one block) is drawn as a plain cell-wide prism along its own axis, with no bow or flare. Vertices take sky light from the open cells around them, by `vertex_light` or `face_light`, because a log's own cell has light 0. Bark projection follows the open question on seams.
    *   **Cards (PR 2).** Each `Leaves` voxel with a neighbour that is air or flora (a surface leaf) draws one double-sided card, with `push_double_sided_quad`. Size, spin and offset hash from the world position and the tree's seed. The card's plane tilts to lean along the sum of its open directions with an upward bias, and carries a pseudo-normal along that direction for lighting. Size ranges and density follow the Crown section (Pine cards are smaller). Cards carry the `Leaves` atlas cell, the per-tree tint, the canopy shade and the light baked at the voxel. Their shadow lookup uses the treatment plants already have (`docs/materials.md` line 20), checked in PR 2.
*   **Why**: `mesh.rs` is long and trunk and crown share the hash helpers, so they live in one new module. It reuses `trees::hash` and makes `hash01`, `between` and `smoothstep` in `mesh.rs` `pub(super)` rather than adding a second hash.

#### 2. `crates/ochre-terrain/src/generate/trees.rs` (Modify)

*   **Type / Scope**: Tree data, PR 1 (record, lean) and PR 3 (variety).
*   **Proposed Changes**: Widen the `TreeTints` entry from tint to a look record: tint, the trunk's base y and height in blocks (from `Tree::base` and `trunk`), a lean vector (at most 0.5 block long), and a seed. `TreeTints::new` fills it from each `Tree`. Add `TreeTints::look_at(coord)`, which `at` keeps as a thin wrapper over. The lean comes from the seed, biased by the temperature and humidity that `tree_in_cell` has: it reads `noise.temperature` and `noise.climate()`, and `Climate` carries `humidity`, so no new `TerrainSource` method is needed. `Tree` gains a `lean` field set there, and `Tree::new` callers (`trees.rs` 625, 687, 716, `tests.rs` 1957, 1965) take it as a new argument. Any new trig uses `libm`. Update `heap_size`. The LOD path builds `TreeTints` too (`lod.rs` 442, 467, 582), so the wider record applies there, which is accepted. PR 3 changes the crown (lobes and crown lean) and so the voxels `Tree::plant` writes, and the pinned hashes are updated.
*   **Why**: The mesher already reaches `TreeTints` through `BlockSource`. Nothing else carries a tree's seed, base and height to it.

#### 3. `crates/ochre-terrain/src/generate/generator/source.rs` (Modify)

*   **Type / Scope**: `BlockSource` trait and its impls, PR 1.
*   **Proposed Changes**: Add `tree_look(coord)` beside `leaf_tint`, with a default returning a plain look (no lean, seed 0). `ChunkNeighbourhood` forwards to `chunk.trees.look_at`, and the wrapper impl forwards to its inner source.
*   **Why**: `mesh_blocks` is generic over the source, and LOD sources must keep compiling.

#### 4. `crates/ochre-terrain/src/generate/generator/mesh.rs` (Modify)

*   **Type / Scope**: Mesher entry, PRs 1 and 2.
*   **Proposed Changes**: After `mesh_flora` in the `cell_size == 1` branch, call `trees_mesh::mesh_trees` when `style.tree_shapes`. Declare the new module in the generator `mod`.
*   **Why**: Flora is drawn here the same way and for the same reason.

#### 5. `crates/ochre-terrain/src/generate/generator/greedy.rs` (Modify)

*   **Type / Scope**: Cube face emission and neighbour tests, PRs 1 and 2.
*   **Proposed Changes**: When `style.tree_shapes`, emit no faces for `Log` (PR 1) or `Leaves` (PR 2), and skip the chamfer for logs. Replace `is_opaque()` with `hides_faces()` at the face, bevel, ramp and AO neighbour tests (`greedy.rs` lines 155, 188, 280, 336, 390, 415, 432, 463 in `face_span`, and 603), so the ground under and beside a trunk keeps its faces. Update the destructure of `MeshStyle` at lines 511-516, which names every field. The `InnerLeaves` branch is untouched until PR 5 because LOD still uses it until PR 4.
*   **Why**: The cube must stop being drawn, and the blocks around it must not be culled against it.

#### 6. `crates/ochre-terrain/src/generate/generator/seam.rs` (Modify)

*   **Type / Scope**: Bevel seam pass, PR 1.
*   **Proposed Changes**: Add the tree-shapes flag to `mesh_seams` (it takes no `style` today, and is called at `mesh.rs:317`). Skip a seam where `coord` is a log (`top(coord)` holds for a log with air above, and `cuts(Log)` is true) and where `other` or `beside` is a log (a trunk beside ground counts as a "wall" the seam closes against), using `hides_faces` for the neighbour tests.
*   **Why**: Seams close gaps between chamfered cubes. There is no cube left to close against, and seams must not hide or add faces beside a trunk.

#### 7. `crates/ochre-terrain/src/generate/generator/buffers.rs` (Modify)

*   **Type / Scope**: `MeshStyle`, push helpers, PR 1.
*   **Proposed Changes**: Add `tree_shapes: bool` to `MeshStyle`. Add whatever prism and card push helpers `trees_mesh.rs` needs beside `push_double_sided_quad`, and keep tiles, tint and `lit` in the vertex layout the shaders already read. Leaves stay in the layout the cutout split routes by cell.
*   **Why**: `MeshStyle` is how the flag reaches the mesher without more arguments.

#### 8. `crates/ochre-terrain/src/generate/generator/chunk.rs` (Modify)

*   **Type / Scope**: Chunk mesh entry, PR 1.
*   **Proposed Changes**: `generate_chunk_mesh` takes `tree_shapes: bool` beside `solid_leaves` and sets `MeshStyle::tree_shapes`.
*   **Why**: It builds the `MeshStyle`.

#### 9. `crates/ochre-terrain/src/generate/generator/lod.rs` (Modify)

*   **Type / Scope**: LOD mesh style and tree drawing, PRs 1 and 4.
*   **Proposed Changes**: PRs 1 to 3: set `tree_shapes: false` where it builds a `MeshStyle`, so LOD output does not change. PR 4: lone trees in LOD nodes are drawn by `lod_trees.rs` (below) in place of planted cubes (cell below `BOX_CROWN_CELL`) and trunk and crown boxes (`TreeBox`). The planting into the cell grid stays only so `canopy_tops` and the canopy shade still find the leaves, and those cells draw nothing.
*   **Why**: The compiler needs the field set, and far trees are redrawn in PR 4.

#### 9b. `crates/ochre-terrain/src/generate/generator/lod_trees.rs` (Create)

*   **Type / Scope**: Rust module, PR 4.
*   **Proposed Changes**: For each lone tree a LOD node owns (the node its base is in, as for boxes today, for cells below `CANOPY_CELL`), emit a low-poly trunk prism and a low-poly crown built from the same `Tree` (its crown blob and lobes, trunk height, lean and seed). The trunk follows the same offset function as near trees. The crown is a few cards or a coarse hull sized from the blobs, with the tree's tint. Detail drops by level: at the finest LOD cells the prism and cards are close to the near ones, past that a prism and one hull. Vertices per tree stay within the per-level budget in "Switch, performance and what retires". A test checks the LOD crown bounds the near card envelope within one cell.
*   **Why**: Far trees must match near ones, and the node already has the `Tree` list and an owner rule, so no new data is needed.

#### 10. `crates/ochre-terrain/src/chunk_loader.rs`, `remesh.rs`, `lib.rs`, `pack.rs`, `generator/mod.rs`, `examples/bench.rs`, `group.rs` tests (Modify)

*   **Type / Scope**: Plumbing and call sites, PR 1.
*   **Proposed Changes**: `tree_shapes` goes in as a new `ChunkLoader::new` parameter from `RendererSettings` in `insert_terrain_resources` (`lib.rs:135-166`), not through `DebugSettings` as `solid_leaves` does, and on into `generate_chunk_mesh` and `mesh_group`. Call sites to change: `pack.rs:166`; `remesh.rs` at 94, 192, 306, 454, 466 and the `ChunkLoader::new` callers in its tests (496, 589); `group.rs` tests (382, 445); about eleven in `generator/tests.rs` (681, 1328, 1342, 1527, 3099, 3173, 3292, 3781, 4028, 4078) and the `style()` helper at line 30; the `MeshStyle` literals in `generator/chunk.rs` and `lod.rs`. `examples/bench.rs:122` reads an environment variable `BENCH_TREE_SHAPES` (default on) so before and after runs compare. `generator/mod.rs` declares `trees_mesh` (PR 1) and `lod_trees` (PR 4). A change to the setting takes effect on restart.
*   **Why**: The first mesh of a chunk and every remesh must use the same style, or an edit would flip a tree's look, and `bench` measures nothing unless it can switch the setting.

#### 11. `crates/ochre-terrain/src/settings.rs`, `assets/settings.toml`, `src/settings.rs` (Modify)

*   **Type / Scope**: Settings, PR 1.
*   **Proposed Changes**: `RendererSettings.tree_shapes` (default true) with a one-line comment, a `tree_shapes = true` line in `settings.toml`, and the default checked in the existing settings test in `src/settings.rs`.
*   **Why**: Frost has the same switch, and PR 4 removes it.

#### 12. `crates/ochre-terrain/src/group.rs` (Modify)

*   **Type / Scope**: Group bounds, PR 2.
*   **Proposed Changes**: Widen `group_aabb()` by the largest reach of a card or shrub past its voxel, one named constant beside it, worked out from the card size, spin, tilt and hashed offset ranges, with a test over the full hash range that a card at a group's edge lies inside. Say in the PR whether the larger shadow-caster box on every group is accepted (`docs/shadow-perf.md`). `group_aabb()` is used at `group.rs:168,220,231,242,254` and `remesh.rs:235`.
*   **Why**: A card past the bounds would be frustum-culled while still on screen.

#### 13. `crates/ochre-base/src/block.rs` (Modify)

*   **Type / Scope**: Block rules, PR 1 (`hides_faces`) and PR 2 (targeting).
*   **Proposed Changes**: Add `BlockType::hides_faces()`: true where `is_opaque()` is, except `Log`. The mesher uses it for culling a neighbour's face, bevel and ramp shapes, AO and seams, while `is_opaque()` stays the rule for light. `Leaves` is no longer targetable, which also makes it unbreakable (`is_breakable` is built on `is_targetable`, `block.rs:85`), so a crown cannot be cleared. `Leaves` stay solid with `light_opacity` 1. A new test covers `Leaves` (the existing tests at lines 564-574 never name it).
*   **Why**: Faces of the ground under and beside a trunk must stay visible once the log is a thin prism. A ray under a crown must reach the trunk. The untargetable rule holds even with `tree_shapes` off, because `is_targetable` has no access to settings, so the switch does not restore breaking leaves.

#### 14. `crates/ochre-gameplay/src/breaking.rs`, `crates/ochre-terrain/src/settings.rs` (Modify)

*   **Type / Scope**: Target outline, PR 1.
*   **Proposed Changes**: A `TreeShapes(bool)` resource, defined beside `RendererSettings` in `settings.rs` and modelled on `FrostSwitch`, inserted by `insert_terrain_resources` from `RendererSettings.tree_shapes`. `show_outline` (`breaking.rs:128-145`) hides the outline when `LookedAt`'s `BlockType` is `Log` and `TreeShapes` is on. A test covers the hidden and shown cases. Breaking still deletes one voxel at a time.
*   **Why**: `breaking.rs` has no other route to the setting, and the outline is a full cube at the log's voxel.

#### 15. `generator/tests.rs`, `trees.rs` tests, `trees_mesh.rs` tests, `group.rs` tests, `lod` tests (Modify)

*   **Type / Scope**: Tests, with each PR.
*   **Proposed Changes**:
    *   `trunk_centre(record, y)`, a pure function in `trees_mesh.rs`: zero offset at the base, never above 0.5 block, rising with height, and equal when called for the same log from two chunk views (extends `test_trees_across_chunk_borders_are_whole`) (PR 1).
    *   Ground under and beside a tree log has the same faces as with the log replaced by air, on flat ground and on a slope of gradient 1.5 (PR 1).
    *   Prism vertices are lit above zero in daylight, so trunks are not black (PR 1).
    *   A leaf core voxel adds no vertices, and a surface leaf adds at least one card, counted by quads whose vertex alpha is the `Leaves` cell (PR 2).
    *   Baked sky light for a chunk is identical with `tree_shapes` on and off (PR 2).
    *   The largest reach of a card past its voxel over 1000 hashed voxels is at most the group constant (PR 2).
    *   `open_edges` tests keep passing, and new tree tests exempt cards by their `Leaves` cell, since cards sit in `opaque` (PR 2).
    *   Voxel output and `determinism.rs` pins unchanged in PRs 1 and 2. PR 3 changes crown voxels, so the pins are updated there with the reason recorded. `test_trees_are_a_trunk_under_big_crowns` is reviewed in PR 3.
    *   An LOD tree's crown bounds the near card envelope within one cell, and a tree's vertex count at each LOD level is at most its budget (PR 4).
*   **Why**: These are the properties the design depends on.

#### 16. Docs and READMEs (Modify, Create)

*   **Type / Scope**: Documentation, with each PR.
*   **Proposed Changes**:
    *   Promote this draft to the design of record (it is already at `docs/designs/trees.md`) with an Implementation Notes section filled per PR, in the format of `docs/designs/snow-and-frost.md`.
    *   `docs/materials.md` line 48: close "tree crowns are still cubes". The matching open item in `docs/designs/painterly-lighting.md` too.
    *   `docs/climate.md` (trees and flora): how a tree is drawn. `docs/world-plan.md`: glossary entries trunk column, leaf core, foliage card.
    *   `docs/terrain-perf.md`: a dated entry per PR with chunk vertices, shadow pass cost and median frame time, tree_shapes on against off.
    *   READMEs: `crates/ochre-terrain/src/generate/generator/README.md` (the new module and the `generate_chunk_mesh` signature), `crates/ochre-terrain/src/generate/README.md` (`TreeTints`), `crates/ochre-terrain/src/README.md` (the setting), `crates/ochre-base/src/README.md` (leaves not targetable), `crates/ochre-gameplay/src/README.md` (the outline).
*   **Why**: The repo's docs are the record of why, and the README contracts must match the signatures.

#### 17. PR 5 cleanup (Modify)

*   **Type / Scope**: Removal, after the owner accepts the look.
*   **Proposed Changes**: Once PR 4 stops LOD from drawing planted `Log` and `Leaves` cells, delete the cube path for logs and leaves in `greedy.rs`, `InnerLeaves` (both `Apart` and `Hidden`), `ChunkMeshes::inner_leaves`, `InnerLeafFaces` and `show_inner_leaves` (`group.rs`, `lib.rs`), the `solid_leaves` plumbing (`chunk_loader.rs`, `remesh.rs`, `lib.rs`, `generator/chunk.rs`, `settings.rs`, `material.rs`, `settings.toml`, `src/settings.rs`), the inner-leaves constants in `buffers.rs` and `material.rs` (`INNER_LEAVES_REACH`, `INNER_LEAVES_BAND`, `leaf_holes_filled`), the shader code in `world.wesl` (`FAR_HOLE_SHADE` and line 260), `world_prepass.wesl` (line 94) and `foliage.wesl` (line 20), the inner-leaves tests (`tests.rs` around 1293-1349, `group.rs` 452-473), and `docs/shadow-perf.md` lines 24 and 96, then `tree_shapes` itself. After this PR there is no switch and no way to compare against cubes. `bench` vertex counts differ by design, recorded as an expected diff.
*   **Why**: Two leaf paths side by side is the maintenance cost the owner asked to avoid.

### Verification Plan

*   **Compilation/Build**: `cargo build --release --workspace --all-targets`.
*   **Unit Tests**: `cargo test --release --workspace`. `determinism.rs` unchanged.
*   **Static Analysis & Linting**: `cargo clippy --workspace --all-targets` clean.
*   **Performance**: `cargo run --release --example bench` before and after each PR, with `examples/bench.rs` changed to pass `tree_shapes` so on and off can be compared. Chunk vertex counts in a forest view stay within 1.10 times in PRs 1 to 3. LOD vertex counts are identical in PRs 1 to 3 and may change in PR 4, where each LOD level in a forest view stays within 1.10 times. Shadow pass cost and frame time come from the sandbox with `OCHRE_STATS` (`shadow_cull` prints triangle and draw counts only, and `flight` is headless), against the `docs/materials.md` fps gate (median 16.7 ms at 1080p, three runs), on a named forest seed and spot and a jungle spot, `tree_shapes` on against off. Ceilings and the card cap are set after the PR 2 spike, see Open Questions.
*   **E2E/Integration**: A headed run, since there is no shader compile test, and the confirm-latest-build check (one process started after the last build and no panic in the log). Passive screenshots on the three recorded views (taiga with pines, temperate broadleaf, jungle) with `tree_shapes` on and off, at three distances: 4 blocks from a trunk, 30 blocks from a crown, and 100 blocks or more where LOD takes over. Pass rules: no right-angle step along a trunk's vertical edge at 4 blocks; no straight horizontal or vertical crown edge longer than 2 blocks at 30 blocks; the light-equality test passes and the forest floor matches in the side-by-side stills; the player stops at the outermost leaf voxel (collision is unchanged, so the existing tests cover it, plus a walk-in); a far tree shows no change of silhouette at the chunk to LOD border. The owner signs off on the stills.

## Implementation Notes

### PR 1 (`feat/painterly-trees`)

Differences from the plan above, found while building it:

*   **No `hides_faces()` on `BlockType`.** `mesh_blocks` wraps its source in `TreeFaces` (`source.rs`), which reads tree logs as air for the greedy pass, seams, bevels and AO, and keeps the log positions for `trees_mesh.rs`. It needs no change at the dozen `is_opaque()` sites in `greedy.rs`, and light still comes from the real voxels, so baked light is unchanged. A log is air in the greedy view, so the ground under it keeps its top face and no flare is needed to hide anything.
*   **A thick trunk bows too.** All four columns of a 2×2 trunk are in the trunk column and take the same offset, with a radius of 0.56 so the prisms overlap. Only branch logs are plain prisms.
*   **A card is 4 vertices**, not 8: `push_double_sided_quad` is one set of vertices with both windings.
*   **Prism ends** are capped only where they do not run into another log or into ground.
*   **`bench` defaults to cubes.** `BENCH_TREE_SHAPES=1` turns the prisms on, so the default counts stay comparable with the baseline.
*   **Not yet done in PR 1:** the three measurement views, and a look at the result in the game (stills at 4, 30 and 100 blocks).

--------------------------------------------------------------------------------

*Drafted with EGM*
