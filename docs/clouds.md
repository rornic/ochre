# Clouds

The clouds are marched at a fixed low resolution (below) and painted at display resolution by the dome, `cloud_dome.wesl` and `cloud_paint.wesl`, so they stay crisp at any screen size.

## Painted look

The dome reads the march's target by cubic B-spline, so its contours are smooth at any magnification, and repaints it:

- Cover is cut at `EDGE` with brush strokes (`brush`) moving the cut, so shapes get a crisp, irregular outline. The edge is always at least one pixel soft, whatever the resolution.
- Sun and sky light are rounded to a few bands (`SUN_BANDS`, `SKY_BANDS`) with strokes wobbling each step, and a little grain on top.
- Haze softens what's far off: the more of a pixel's light is haze, the wider its edge and band steps (`HAZE_EDGE`, `HAZE_BAND`) and the thinner its body (`HAZE_THIN`), so distant clouds fade into the horizon rather than staying cut out.
- Thin, even cloud (the cirrus) is a pale wash (`VEIL`) rather than cut away, except at a cumulus's edge, where cover changes fast.
- Strokes live on the clouds' plane and drift with the wind, so they stay put on a cloud. Every constant is at the top of `cloud_paint.wesl`.

The water's reflections still read the unpainted march. The sun and moon's cover reads it through `body` in `cloud_edge.wesl`, the painting's own edge and opacity (fully opaque past the edge: under 1, the moon showed through the cloud's body), so a disc behind a painted cloud is hidden.

`cargo run --release --example cloud_shot -- out.png [width height yaw pitch eye_height]` renders the sky to a PNG with no window and prints the clouds' GPU times, with `OCHRE_SEED` and `OCHRE_TIME`.

## Cloud resolution

The march target is 640 pixels wide, or half the window's width if that is less, with the height following its aspect ratio (640x360 at 1080p, 1440p and 4K). It is not a setting: the painting repaints it at display resolution, and the presets from 320 to 1280 differed little but in the fine detail of far puffs, for a march 2 to 8 times the cost (`TARGET_WIDTH` in `clouds.rs`).

The march target and temporal history use the same calculation on startup and window resize, from the camera's render target. Temporal accumulation blends the march over frames, and the dome paints it up to display resolution.

## Cost

Measured with `cloud_shot` on an RTX 2060 at `medium`, 1080p, over five views of seed 1 to 3 (GPU ms a frame, before to after the work below):

| Pass | Before | After |
|---|---:|---:|
| March | 1.64 | 0.85 |
| Reflections' map (a full redraw, which is now spread over 64 frames) | 1.45 | 0.83 |

- The noise's lattice wrapped every corner of every sample with integer modulos, 32 a noise call; it wraps once per axis now (`wrapped` in `cloud_march.wesl`). The output is identical, and it was most of the saving.
- The march takes 12 steps a pixel (`STEPS`), not 16. In motion, where the history can't make up for them, the image is about 12% further from the converged still (`CLOUD_SPIN` in `cloud_shot`); 10 steps made it 30% and 6 steps 140%.
- The reflections' map is redrawn a band of rows a frame (`REFLECTION_BANDS`, 64, so all of it every second at 60 frames a second), apart from the texels toward the sun and moon, which are every frame: it drifts slowly, and is only read through waves. That is about 0.04 ms a frame, with no spike.
- The temporal blend is `BLEND_STILL` (0.05) while the camera is still and rises to `BLEND` (0.12) as it turns or moves, so distant edges settle: the share of pixels flickering over 3 frames fell from about 0.04% to 0.005% at the horizon, and the image in motion is unchanged.
- The history is resampled by Catmull-Rom (`sharp_history` in `cloud_target.wesl`), not bilinear: bilinear's blur, taken again each frame the view turns, thinned small clouds until they shrank. In motion the image is about 40% closer to the settled still.
- Tried and dropped: marching clear sky one frame in four (3 to 5% saved, in the noise of a warp's mixed pixels), light from one sample (brightens clouds), light reused over alternate steps (a warp that skips can't save, and skipping by step index lit edges wrongly), a swirl by sines in the brush (3% of the dome's cost, a different look), and bilinear in place of the cubic read (10% of the dome's cost at 4K, but square texels show on thin cloud).
- Left: the light's samples are about half the march. Lighting the clouds from their cover alone, with no noise, took another 44% off at the cost of the lumpy shadows inside a cloud (broader, simpler shapes), so it is a look choice, not done here.
