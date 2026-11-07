# Painterly lighting (stage 2)

*   **Author:** Rory Nickolls
*   **Status:** Draft, for hand-off
*   **Date:** 2026-11-07

## Overview

Painted world-space materials (`docs/materials.md`) replaced the 16px atlas tiles on terrain faces. The lighting on them is still physically flavoured: Lambert sun, hard cascaded shadows, a grey-leaning vertex AO multiplier, a cool sky ambient. Painted albedo is low contrast and delit by design, so lighting now carries all of the form, and it reads as a lit render of a hand-painted texture, not as a painting.

The lighting is also not smooth. Every bevel strip is lit with one constant chamfer normal and every ramp as a flat top, so a bevel reads as a band with a hard light step at each edge, and the texture's cross-fade (below) sits under a lighting step that gives the join away. Smoothing the lighting across bevels is the first slice of this change.

This change gives painted cube faces a painterly lighting model: soft wrapped light, hue-shifted shadows, saturated mids, softer shadow shapes and a gentler AO. It is a cube-only spike behind a shader def, compared against the current model on fixed scenes, and the loser is deleted.

Target look: Overwatch-style hand-painted. Warm key light, cool coloured shadows (never grey or black), forms modelled by soft colour gradients rather than hard light/dark steps, no outlines.

## Scope

In: faces that draw a painted material (`painted` in `world.wesl`, `Tile::material_layer().is_some()`), at full-detail chunks.

Out, unchanged:

*   Leaves, flora, water, sky, clouds, fog, LODs, unpainted tiles (compacted rocks, ice, logs).
*   `lighting.wesl`. Water and the sky use its helpers, so the painterly path adds new functions in a new `painterly.wesl` and edits none of the existing ones.
*   Vertex counts and vertex format. `cargo run --release --example bench` counts must match before and after. Smooth bevel lighting (below) needs no new attribute.
*   The shadow and depth-prepass pipelines (`world_prepass.wesl`, shadow map). Only the main fragment changes.

A cube-only change leaves a visible seam where painted ground meets leaves, flora and water. That is accepted for the spike, and the follow-up decision (extend to leaves and flora or not) is the owner's, taken with both in view.

## Where lighting happens today

`assets/shaders/world.wesl`, in `fragment`:

```
tinted = color.rgb * in.color.rgb * drift(...)        // vertex colour = grass tint * ao^AO_GAMMA * shade
shaded = lit_normal(normal, world_position)            // normal flattened toward up past 1 km
light  = sunlight(...) + sky_ambient(sky, shaded) + point_light_glow(...)
color  = fogged(tinted * light, ...)
```

*   `sunlight` = `direct_sun(shaded) * gate * sun_shadow * cloud_shadow + direct_moon`. `direct_sun` is `sun_radiance * max(dot(n, to_sun), 0)`. Shadow and cloud shadow are scalars multiplied in, so shadow is just less sun, and what remains is `sky_ambient`.
*   `sky_ambient` = `CAVE_AMBIENT + open_sky(n) * pow(sky, SKY_LIGHT_CURVE)`, with `open_sky` mixing a ground bounce and `sky_irradiance` by `n.y`.
*   AO is baked into the vertex colour rgb with the tint: `tint * ao.powf(AO_GAMMA) * shade`, `AO_GAMMA = 2.2` (`generator/shading.rs`, `greedy.rs`). The shader cannot separate AO from tint. Chamfer strips are lit with the nominal 45 degree chamfer normal, which already lets bevels catch light differently from the top and wall.
*   Colour pipeline: HDR scene, `Bloom::NATURAL`, tonemapping `SomewhatBoringDisplayTransform`, `dry_color_grading()` (`ochre-world/src/tonemap.rs`). `Msaa::Off`.

## Design

A `PAINTERLY` shader def, set from a new `[renderer] painterly_lighting` setting (default off while comparing, restart to change), carried like `MATERIAL_PERIOD` through `ChunkMaterial`'s key (`ochre-terrain/src/material.rs`, `ShaderDefVal`). In `world.wesl`, when `PAINTERLY` and `painted`, the `light` line is replaced by `painterly_light(...)` from a new `assets/shaders/painterly.wesl`. Everything is a named const at the top of that file so tuning is a one-line edit.

`sunlight` currently returns shadow already multiplied in. The painterly path needs the pieces, so add `sun_visibility(...)` (shadow times cloud shadow, 0..1) beside `sunlight` as a refactor that leaves `sunlight`'s output bit-identical, and call it from both.

### Smooth bevel lighting (do this first)

Texture projection and lighting both read the stored vertex normal today, so it can't be made smooth in the mesh. `material_uv` picks a face's projection plane from it per vertex, and a normal that grades from up to a wall's direction across a strip would flip the plane mid-strip and smear the uv. So the mesh keeps its faceted normals, and the fragment shader rebuilds a smooth lighting normal from the blend marks the mesher already writes (`MeshBuffers::set_blend`, mode `uv_b.y`, decoded in `world.wesl` as `wall_blend` and `wall_faces_z`).

*   Chamfer strip: `n = normalize(mix(up, side, t))`, where `t` is the blend share (0 at the top's edge, 1 at the foot), `side` is the horizontal direction of the stored chamfer normal (`normalize(stored.xz)`, mind that stored z is reversed), and `up` is `(0, 1, 0)`. The strip then grades from the top's light to the wall's across its 0.28 block width, and meets both exactly.
*   Ramp: the same with `side` pointing away from the wall, capped at about `t = 0.5`, since the join is a concave crease and a full turn would light it as the wall.
*   The mark only carries the wall's axis, not which way it faces. Extend the code to four bands of 0.125 (x+, x-, z+, z-) instead of the two bands of 0.25, in `BLEND_SPAN`, `BLEND_Z` and the decode, so `side` has its sign. Keep the maximum under 0.5, which marks coarse cells in `world.wesl`, `world_prepass.wesl` and `shadow_mesh.rs`.
*   Use the smooth normal for `lit_normal`, sun, sky and moon, and keep the geometric normal for the shadow lookup's bias (`sun_shadow`), so shadow acne doesn't change.
*   Leave AO alone: it is already smooth per vertex.

This applies in the existing lighting model too, so it is worth measuring on its own before the painterly steps: the bevels should stop reading as bands.

Still faceted after it: vertical corners where two walls meet, and ground against a wall of another block (no ramp, "different blocks never ramp"). A flat face has no vertices to grade across. Smoothing those needs a chamfer on vertical corners (a mesher change like `Edge::Convex` on a wall's sides), which can then reuse the same marks. Out of scope here, listed under open questions.

The model, in order:

1.  **Wrapped diffuse.** `wrap = clamp((dot(n, to_sun) + W) / (1 + W), 0, 1)` with `W` about 0.35, then a smoothstep to round the terminator. Faces turned slightly away from the sun still get light, and the light-to-shade change across a chamfer or ramp is a gradient, not a step.
2.  **Soft shadow shapes.** Remap the visibility scalar through `smoothstep(S0, S1, v)` (about 0.1 to 0.9) so cascade edges and cloud shadows read as painted soft shapes. Do not add shadow-map taps in the spike. If the cascades alias, say so in the report instead of fixing it.
3.  **Coloured shadow.** Shadow is not "sun removed". Light = `sun_colour * wrap * v + shadow_fill * (1 - wrap * v)` where `shadow_fill` is the sky ambient pushed toward a cool blue-violet (`SHADOW_TINT`, luminance-preserving) and lifted so shadows never go below a floor (`SHADOW_FLOOR`, about 0.35 of lit luminance at noon). Warm in the light, cool in the shadow, driven by the real sun colour so dusk stays orange.
4.  **Saturation shaping.** After lighting, a luminance-preserving saturation gain (`SAT_LIT` about 1.1, `SAT_SHADOW` about 1.25) on the shaded albedo, so mids are rich and shadows are coloured rather than grey.
5.  **AO response.** Vertex AO arrives multiplied into the albedo with the tint, so the shadow floor, which lifts light, does not soften it and option (a) changes nothing. Built as (b) without the warm tint: painted faces use `AO_GAMMA_PAINTERLY` (1.5, against 2.2) when `painterly_lighting` is on, set through `set_painterly_ao` like `set_packed`, so leaves and unpainted tiles keep `AO_GAMMA`. Owner decides after seeing it.
6.  **Moon and night.** Same structure with the moon as key and a deeper blue fill. Nothing painterly is needed at night beyond not crushing the floor. `point_light_glow` is unchanged.

Not in the spike: rim light, outlines, stylised shadow banding, per-material shading ramps, painted skyboxes. Add them only if the six steps fall short.

## Testing and comparison

*   Scenes (seed `OCHRE_SEED=2070026479`, 1920x1080, `OCHRE_RESOLUTION=1920x1080`): `OCHRE_SPAWN=10832,-3952,5 OCHRE_LOOK=10832,30,-3940` (steep 1-block steps, ramps and chamfers) and `OCHRE_SPAWN=10800,-3830,3 OCHRE_LOOK=10796,35,-3809`. Each at `OCHRE_TIME` noon, mid-morning, golden hour and night, painterly off and on: 16 images, each pair in one side-by-side PNG.
*   Capture passively. Run the game in a nested X server (`Xephyr :7 -screen 1920x1080`, `DISPLAY=:7`) and screenshot with `xwd -root`, so nothing takes focus or input on the owner's desktop. Stop ComfyUI first if it is running, both want the 6GB card.
*   Painterly off must be pixel-identical to the end of slice 2. Capture off-vs-main before touching anything and diff.
*   Perf gate: median frame time at spawn at 1920x1080 on the RTX 2060 stays at 60fps or better (`OCHRE_STATS=1`, three runs, as in `docs/materials.md` "The fps gate"), logged in `docs/terrain-perf.md`. The change adds ALU and no samples or bindings, so a regression means a mistake.
*   `cargo test --release --workspace` and `cargo clippy --workspace --all-targets` clean. Add a test that `painterly.wesl` imports resolve (the material tests that parse `block_texture.wesl` are the pattern) and that the setting defaults and parses in `src/settings.rs`.

## Slices

1.  Baseline: capture the 16-image set and fps with the current model. Plumb `painterly_lighting`, the `PAINTERLY` def and an identity `painterly_light` (returns today's light). Off and on must both match the baseline pixel for pixel.
2.  Smooth bevel lighting, in the current model and behind no setting (it is a fix). Send the owner before and after on the steep-terrain scene.
3.  Wrap, soft shadow remap and coloured shadow (steps 1 to 3). Send the owner the side-by-side set.
4.  Saturation and AO (steps 4 and 5), tuned from the owner's notes.
5.  Owner picks. Delete the losing path and the setting if painterly wins (it becomes the model), or delete `painterly.wesl` if not. Update `docs/materials.md`, `AGENTS.md` and `docs/terrain-perf.md`.

Commit per slice as lowercase conventional commits. Do not push or open a PR until asked.

## Open questions for the owner

*   How far toward flat painted (Zelda-like soft bands) versus soft painted (Overwatch)? The brief assumes soft gradients with no banding.
*   Is a cube-only seam at trees and water acceptable for the spike, or must flora and leaves follow in the same change?
*   Should the painterly model also apply to LOD cells (they draw a mean colour and are far away, so probably not)?
*   AO option (a) or (b).
*   Chamfer vertical wall corners, so they can blend and light smoothly like the tops' bevels? It is a mesher change with a vertex cost, and without it cliff corners stay a hard crease in both texture and light.

## Texture notes carried from stage 1

*   Seams on ramps run diagonally across the texture and don't line up. A square texture meeting itself along a diagonal has no shared pattern to match, so this is accepted. Painterly lighting hides some of it, and a lower-contrast layer hides more. The cross-fade over the last 40% of a ramp's rise (`RAMP_BLEND_FROM` in `buffers.rs`) is the knob if it needs softening.
*   Triplanar blending was considered for the whole world and rejected: faces here are axis-aligned, so it only differs from box projection on tilted surfaces, which the blend marks already cover. Wall corners are the exception, see above.

## Risks

*   The painted layers are neutral and low contrast, so a strong shadow tint can turn them muddy. Tune on stone and grass first, since they cover the most ground.
*   `drift` and the grass tint already move brightness and warmth. Saturation gain may double-count them, so compare on grass.
*   Wrapped light on faces pointing away from the sun lightens cliffs on the shaded side. The `SHADOW_FLOOR` and `W` values interact, tune them together.
*   The smooth normal assumes the 45 degree nominal chamfer and the ramp's rise (`BEVEL`, `RAMP`). If those change, so does the cap above.
*   Refactoring `sunlight` must not change water, leaves or the sky. Water imports lighting helpers, so run the baseline diff on a water scene as well.
