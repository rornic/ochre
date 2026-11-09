# Climate and biomes

How a cell gets a biome and what grows on it. Landforms are `docs/landforms.md` and read the ground alone; climate reads them, never the other way round.

## The config

A seed draws `warmth`, `humidity`, a temperature `gradient` and its angle, `continentality`, the wind direction and the plate count (`worldmap/character.rs`). `OCHRE_CHARACTER=off` keeps the config as given, and the tools (`worldview`, `worldprobe`, `bench`, the game) all build the world with the drawn character.

## Warmth and moisture (`climate.rs`)

Regional warmth is the baseline plus a warped gradient plus continentality over the distance from the sea; it falls with height at `LAPSE_PER_BLOCK`. Moisture comes off the sea along the wind, drops on rising ground and leaves rain shadows; humidity scales the rain. Both are blurred, rivers and lakes wet and cool their banks (not in cold country), and a Whittaker-style table (`classify`) gives the climate biome: glacier, tundra, taiga, grassland, forest, steppe, savanna, desert, jungle, with swamp on flat, low, wet ground by fresh water. The treeline moves with regional warmth.

## Biomes (`ochre-base/src/biome`)

A biome sets how thick trees grow, how big they get (`TreeScale`: height, spread, squash, a share of emergent giants), how lush or dry the grass reads (`climate_shift`), the blocks it uses, and its relief features. Mountain and volcanic are a `Modifier` on the climate biome, not biomes of their own, so a desert mountain and a taiga mountain differ by what is under them. Jungle trees reach 45 blocks, a tenth of them 2.3 times the trunk of the rest.

## Edges between biomes

The map holds, for each land cell, `biome_alt` (the climate biome across its nearest class boundary) and `biome_margin` (how far off the boundary it is, 255 well inside: the climate offset that changes the class, but never past two cells from a different class, since a slowly changing climate would otherwise scatter patches for hundreds of blocks). At block scale `biome_at` looks up the cell through a wander of 1.5 cells and a slow drift of up to 160 blocks, which bends edges over hundreds of blocks, and near a boundary a block takes the alt biome in patches of 10 to 20 blocks, thickest on the boundary. Cells 16 blocks wide or more use their own biome, so LODs do not change. `worldprobe edges` measures it.

## Trees and flora

Tree density is the map's per-cell density times a grove multiplier from noise at wavelength 160 (0.4 in a clearing to 1 in a grove, filtered for the sample's cell so wide cells see an average). Under trees, ferns and shaded grass are twice as thick in jungle and half as thick in taiga as in forest. Flora thins out in the cold, to none below temperature 0.10 (`COLD` in `flora.rs`).

Shrubs and heath (`flora_at`) grow in the open ground of some biomes, in clusters of about five, one at most in each 16 block square, a carpet of plants whose planes overlap: a cluster is a disc 2 to 3 blocks in radius filled at 50%, and a share of the squares hold one (`shrubs` in `flora.rs`): shrubs in grassland (35% of squares), savanna, steppe and swamp, and heath, low and mauve, in taiga (25%) and tundra (70%). None grow in shade, forest, jungle or desert. The tundra has no poppies or dandelions.

The tundra is bare ground, not snow: the climate would snow every block of it, but the biome thaws the snow below 350 blocks over the sea into tan grass, with gravel on ground steeper than gradient 0.45 (`Plain::with_thaw`), and the snow stays on the heights. Its grass is read from a drier, colder climate, and the cold dry end of the grass palette is tan.
