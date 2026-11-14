use bevy::math::{DVec3, I64Vec2, I64Vec3, IVec3, Vec2, Vec3};

use super::generator::column_block;
use super::region_store::{HeightSampler, SLOPE_SPAN};
use ochre_base::biome::{BiomeId, Modifier, TreeScale};
use ochre_base::block::BlockType;
use ochre_base::terrain_source::TerrainSource;
use ochre_base::WATER_LEVEL;

/// What trees need to know about the ground to take root, from the full-detail heightmap or
/// from the coarser grid a distant LOD node samples.
pub trait TreeGround {
    fn trees(&mut self, x: i64, z: i64) -> f64;
    fn height(&mut self, x: i64, z: i64) -> f64;
    fn biome(&mut self, x: i64, z: i64) -> BiomeId;

    /// The slope there, as `HeightSampler::gradient` measures it.
    fn gradient(&mut self, x: i64, z: i64) -> (f64, f64) {
        let span = SLOPE_SPAN;
        let across = |a: f64, b: f64| (a - b) * (1.0 / span as f64);
        (
            across(self.height(x + span, z), self.height(x - span, z)),
            across(self.height(x, z + span), self.height(x, z - span)),
        )
    }

    /// Whether no tree with this `chance` can grow at `x`, `z`, known without `trees` there.
    fn rules_out(&mut self, _x: i64, _z: i64, _chance: f64) -> bool {
        false
    }

    /// Whether water stands over the column at `x`, `z`, where nothing is set down.
    fn wet(&mut self, _x: i64, _z: i64) -> bool {
        false
    }

    /// Whether the ground at `x`, `z` is bare rock on top of a relief feature, where nothing grows.
    fn rocky(&mut self, _x: i64, _z: i64) -> bool {
        false
    }

    /// The height a tree's base stands at, which a coarse node sets to the ground it draws, so
    /// trees sit on it, while the rest above stay exact, so the same trees grow at every level.
    fn base_height(&mut self, x: i64, z: i64) -> i64 {
        self.height(x, z).round() as i64
    }
}

impl TreeGround for HeightSampler<'_> {
    fn trees(&mut self, x: i64, z: i64) -> f64 {
        HeightSampler::trees(self, x, z)
    }

    fn height(&mut self, x: i64, z: i64) -> f64 {
        HeightSampler::height(self, x, z)
    }

    fn biome(&mut self, x: i64, z: i64) -> BiomeId {
        HeightSampler::biome(self, x, z)
    }

    fn rocky(&mut self, x: i64, z: i64) -> bool {
        HeightSampler::rocky(self, x, z)
    }

    fn wet(&mut self, x: i64, z: i64) -> bool {
        HeightSampler::water_top(self, x, z) > WATER_LEVEL
    }
}

// Trees stand on a grid of cells this many blocks across, at most one somewhere in each cell.
const CELL: i64 = 10;
/// How far out from its trunk any tree reaches.
pub const TREE_REACH: i64 = 14;
/// How far above its ground any tree reaches.
pub const TREE_HEIGHT: i64 = 48;
// An emergent giant is this many times as tall and as wide in the crown as the biome's trees.
const GIANT_HEIGHT: f64 = 2.3;
const GIANT_SPREAD: f64 = 1.35;
// Colder than this, trees are pines.
// Trees growing where it is cooler than this, at their height, are pines: cold lowlands, and
// the slopes of any range higher up, where the air has cooled.
const PINE_TEMPERATURE: f32 = 0.46;
// Higher than this above the sea, a mountain's trees are pines whatever the climate below.
const ALPINE_PINES_ABOVE: i64 = 120;
// Ground steeper than this (see `terrain_block`) is too steep to take root on.
const MAX_GRADIENT: f64 = 1.5;
// Broadleaf crowns are a big round crown with smaller lobes hanging off it on branches.
const LOBES: usize = 5;
// A broadleaf has this many lobes at the least, up to `LOBES`, and its crown stands off the trunk by up to this share of its spread.
const LOBES_LEAST: u64 = 3;
const CROWN_LEAN: f64 = 0.2;
// A pine puts out this many boughs at the foot of each tier, which stand this many blocks apart up the trunk, each reaching this share of the crown's radius there.
const PINE_BOUGHS: usize = 4;
const PINE_WHORL: f64 = 3.0;
// The first whorl stands this far above the crown's foot, so its boughs start inside the leaves.
const PINE_FIRST_WHORL: f64 = 1.0;
const PINE_BOUGH_REACH: f64 = 0.7;
// Every trunk is one block thick in the voxels, however thick it is drawn, see `Tree::trunk_radius`.
const TRUNK_WIDTH: i64 = 1;
// Cells this wide or wider judge leaves by coverage: a quarter of a 3 x 3 x 3 spread of points
// inside, or small crowns would fall between cell centres.
const COVERAGE_CELL: i64 = 4;
const COVERAGE_POINTS: usize = 7;
/// Cells this wide or wider leave out the trunk, which would stand a pillar far thicker than it
/// is. Meshing draws it as a thin box instead, see `generator::TreeBox`.
pub const THIN_TRUNK_CELL: i64 = 4;
// How far in leaves are nibbled from the edge of a crown.
const NIBBLE: f64 = 0.9;
// How far each tree's leaves stray from the grass's tint, so a forest isn't one flat green:
// brighter or darker by up to this much, and warmer or cooler by up to this much.
const TINT_BRIGHTNESS: f32 = 0.22;
const TINT_WARMTH: f32 = 0.18;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum TreeKind {
    // a broad, lumpy crown on a stout trunk
    Broadleaf,
    // tiers of leaves narrowing up a tall trunk
    Pine,
    // a flat plate of leaves on a tall bare trunk that forks toward it: the savanna's acacia, and the jungle's emergent giant
    Umbrella,
    // a narrow crown on a tall, straight trunk: the jungle's canopy
    Slender,
}

#[derive(Debug, Copy, Clone)]
pub struct Tree {
    pub kind: TreeKind,
    // the trunk's lowest log, at its -x, -z corner when it's thick
    pub base: I64Vec3,
    // logs up the trunk
    pub trunk: i64,
    // how far the leaves spread from the trunk
    spread: f64,
    // roughens the edge of the leaves
    seed: u64,
    // the broadleaf crown, and smaller lobes around it each held up by a branch, the unused ones with no radius
    crown: Blob,
    lobes: [Blob; LOBES],
    // 0..1, how cold and dry the tree's ground is, which leans its trunk further
    exposure: f64,
}

// A ball of leaves, squashed by `squash` vertically.
#[derive(Debug, Copy, Clone)]
struct Blob {
    centre: DVec3,
    radius: f64,
    squash: f64,
}

impl Blob {
    // Whether `point` is inside the blob with its edge drawn in by `nibble` there.
    fn contains(&self, point: DVec3, nibble: impl Fn() -> f64) -> bool {
        let d = (point - self.centre) / DVec3::new(1.0, self.squash, 1.0);
        let distance = d.length_squared();
        if distance > self.radius * self.radius {
            return false;
        }
        let inner = (self.radius - NIBBLE).max(0.0);
        // only the rim gets nibbled, so most points skip working it out
        distance <= inner * inner || distance <= (self.radius - nibble()).powi(2)
    }
}

impl Tree {
    pub(crate) fn new(
        kind: TreeKind,
        base: I64Vec3,
        trunk: i64,
        spread: f64,
        squash: f64,
        seed: u64,
    ) -> Self {
        let middle = (TRUNK_WIDTH - 1) as f64 / 2.0;
        // a broadleaf's crown stands a little off its trunk, any way, and has three to five lobes
        let (sin, cos) = libm::sincos((hash(seed, 61, 3) % 1000) as f64 / 1000.0 * std::f64::consts::TAU);
        let off = if kind != TreeKind::Pine {
            spread * CROWN_LEAN * ((hash(seed, 67, 5) % 1000) as f64 / 1000.0)
        } else {
            0.0
        };
        // an umbrella's plate stands a block over the trunk's top, so its branches show under it
        let lift = if kind == TreeKind::Umbrella { 1.0 } else { 0.0 };
        let crown = Blob {
            centre: DVec3::new(middle + cos * off, (trunk - 1) as f64 + lift, middle + sin * off),
            radius: spread,
            squash,
        };
        let count = LOBES_LEAST + hash(seed, 53, 7) % (LOBES as u64 - LOBES_LEAST + 1);
        // how far out, how far down, how big and how flat the lobes are, against the crown's spread and squash
        let (out_by, drop_least, drop_range, lobe_size, lobe_squash) = match kind {
            TreeKind::Umbrella => (0.8, 0.0, 0.15, 0.55, squash),
            TreeKind::Slender => (0.6, 0.1, 0.2, 0.55, squash),
            // lobes flatten with the crown
            _ => (0.75, 0.25, 0.25, 0.6, 0.75 * (squash / 0.7)),
        };
        let lobes = std::array::from_fn(|i| {
            if i as u64 >= count {
                return Blob {
                    centre: crown.centre,
                    radius: 0.0,
                    squash,
                };
            }
            let h = hash(seed, i as i64, 17);
            let angle = (i as f64 + (h % 1000) as f64 / 1000.0 * 0.6) * std::f64::consts::TAU
                / count as f64;
            let out = spread * out_by;
            let drop = spread * (drop_least + ((h >> 10) % 100) as f64 / 100.0 * drop_range);
            Blob {
                centre: crown.centre
                    + DVec3::new(libm::cos(angle) * out, -drop, libm::sin(angle) * out),
                radius: spread * lobe_size,
                squash: lobe_squash,
            }
        });
        Self {
            kind,
            base,
            trunk,
            spread,
            seed,
            crown,
            lobes,
            exposure: 0.5,
        }
    }

    /// This tree with how cold and dry its ground is, 0..1, which leans its trunk further.
    pub(crate) fn with_exposure(mut self, exposure: f64) -> Self {
        self.exposure = exposure.clamp(0.0, 1.0);
        self
    }

    /// A far tree's crown as a few big cards over its surface, for levels of detail whose cells are too wide for the voxels: where each is,
    /// in blocks from the trunk's base, which way it faces and how wide it is. A broadleaf's are spread over its ball and, from `detail`
    /// 0.75, each lobe's, a pine's in rings up its cone. `detail` is 1 for 2-block cells and falls as they widen, so fewer, bigger cards.
    pub fn far_cards(&self, detail: f64) -> Vec<FarCard> {
        let mut cards = Vec::new();
        if self.kind == TreeKind::Pine {
            let (bottom, top) = ((self.trunk / 4) as f64, (self.trunk + 1) as f64);
            let height = top - bottom;
            let rings = ((height / PINE_WHORL * detail.max(0.34)).ceil() as usize).max(2);
            let band = height / rings as f64;
            for j in 0..rings {
                let y = bottom + (j as f64 + 0.5) * band;
                let radius = (0.6 + self.spread * (1.0 - (y - bottom) / height)).max(0.8);
                let count = ((radius * 1.6 * detail.max(0.4)).round() as usize).max(3);
                let size = ((std::f64::consts::TAU * radius * band / count as f64).sqrt() * 1.15).min(radius * 1.8).max(1.5);
                for i in 0..count {
                    let (sin, cos) = libm::sincos((i as f64 + 0.5 * j as f64) / count as f64 * std::f64::consts::TAU);
                    cards.push(FarCard {
                        centre: self.axis() + DVec3::new(cos * radius * 0.8, y, sin * radius * 0.8),
                        facing: Vec3::new(cos as f32, 0.6, sin as f32).normalize(),
                        size: size as f32,
                    });
                }
            }
            return cards;
        }
        let lobes = self.lobes.iter().filter(|lobe| lobe.radius > 0.0 && detail >= 0.75);
        for blob in std::iter::once(&self.crown).chain(lobes) {
            let count = (((3.0 + 0.8 * blob.radius) * detail).round() as usize).max(5);
            let area = 4.0 * std::f64::consts::PI * blob.radius * blob.radius * (1.0 + blob.squash) / 2.0;
            let size = ((area / count as f64).sqrt() * 1.1).min(blob.radius * 1.5);
            for i in 0..count {
                // a ball's points spread evenly by the golden angle, but not its underside, which is seen from below only
                let y = 1.0 - 2.0 * (i as f64 + 0.5) / count as f64;
                if y < -0.55 {
                    continue;
                }
                let across = (1.0 - y * y).sqrt();
                let (sin, cos) = libm::sincos(i as f64 * 2.399_963);
                let on = DVec3::new(cos * across, y, sin * across);
                cards.push(FarCard {
                    centre: blob.centre + DVec3::new(on.x, on.y * blob.squash, on.z) * blob.radius * 0.85,
                    facing: Vec3::new(on.x as f32, (on.y / blob.squash) as f32, on.z as f32).normalize(),
                    size: size as f32,
                });
            }
        }
        cards
    }

    /// How big this tree's leaf cards are against others', 0.9 to 1.1.
    pub fn card_scale(&self) -> f32 {
        0.9 + 0.2 * ((hash(self.seed, 71, 13) % 1000) as f32 / 1000.0)
    }

    /// The tree's own tint over the grass's, the same at every level of detail.
    pub fn tint(&self) -> Vec3 {
        let h = hash(self.seed, 31, 59);
        let unit = |shift: u32| ((h >> shift) & 0xffff) as f32 / 65535.0 * 2.0 - 1.0;
        let warm = Vec3::new(1.0, 0.35, -1.0) * TINT_WARMTH * unit(16);
        (1.0 + TINT_BRIGHTNESS * unit(0)) * (Vec3::ONE + warm)
    }

    /// How far the top of the trunk leans from its foot, in blocks: a direction and an amount from the seed.
    pub fn lean(&self) -> Vec2 {
        let h = hash(self.seed, 41, 67);
        let unit = |shift: u32| ((h >> shift) & 0xffff) as f32 / 65535.0;
        let (sin, cos) = libm::sincosf(unit(0) * std::f32::consts::TAU);
        let amount = (0.3 + 0.5 * unit(16)) * (0.8 + 0.6 * self.exposure as f32);
        Vec2::new(cos, sin) * (MAX_LEAN * amount).min(MAX_LEAN)
    }

    /// Whether the crown is over the point `x`, `z` across the world, in blocks, seen from above.
    pub fn shades(&self, x: f64, z: f64) -> bool {
        // from the trunk's base, as `in_leaves` takes it, where a block's index is its middle
        let offset = DVec3::new(
            x - 0.5 - self.base.x as f64,
            0.0,
            z - 0.5 - self.base.z as f64,
        );
        let under = |blob: &Blob| {
            (offset - blob.centre.with_y(0.0)).length_squared() <= blob.radius * blob.radius
        };
        match self.kind {
            // the lowest tier is the widest
            TreeKind::Pine => {
                (offset - self.axis()).length_squared() <= (0.6 + self.spread).powi(2)
            }
            _ => under(&self.crown) || self.lobes.iter().any(under),
        }
    }

    /// The top of the crown, in blocks.
    pub fn crown_top(&self) -> f64 {
        let top = match self.kind {
            TreeKind::Pine => (self.trunk + 1) as f64,
            _ => self.crown.centre.y + self.crown.radius * self.crown.squash,
        };
        self.base.y as f64 + top + 1.0
    }

    /// The bottom of the crown, in blocks.
    pub fn crown_bottom(&self) -> f64 {
        (self.base.y + self.leaf_bounds().0.y.max(0)) as f64
    }

    pub fn width(&self) -> i64 {
        TRUNK_WIDTH
    }

    /// How thick the trunk is drawn, as a radius in blocks: a quarter of a block across on the smallest tree, a whole block on the tallest.
    pub fn trunk_radius(&self) -> f32 {
        let size = ((self.trunk - 6) as f32 / 24.0).clamp(0.0, 1.0);
        0.125 + 0.375 * size
    }

    // the middle of the trunk, as an offset from `base`
    fn axis(&self) -> DVec3 {
        let middle = (self.width() - 1) as f64 / 2.0;
        DVec3::new(middle, 0.0, middle)
    }

    // A box round this tree's own crown and trunk, far tighter than `bounds`.
    #[cfg(test)]
    pub(crate) fn extent(&self) -> (I64Vec3, I64Vec3) {
        let reach = (self.spread * 1.5).ceil() as i64 + 2;
        (
            self.base - I64Vec3::new(reach, 0, reach),
            self.base + I64Vec3::new(reach, self.trunk + reach, reach),
        )
    }

    // The lowest and highest block the tree can fill.
    pub fn bounds(&self) -> (I64Vec3, I64Vec3) {
        let reach = I64Vec3::new(TREE_REACH, 0, TREE_REACH);
        let top = I64Vec3::Y * TREE_HEIGHT;
        (
            self.base - reach,
            self.base + reach + top + I64Vec3::new(1, 0, 1),
        )
    }

    // The tree's block in the cell `size` blocks across with `min` its lowest block, if any:
    // leaves if its centre is among them, a log if the trunk or a branch runs through it. Cells
    // one block across are just blocks, where wood runs up through the leaves; in coarser cells
    // the leaves win, or a whole cell's width of trunk would stand in for the crown. Cells this
    // wide or wider hold leaves if a quarter of them would be, or small crowns would fall between
    // cell centres, and from `THIN_TRUNK_CELL` they leave out the trunk.
    pub(crate) fn block_in_cell(&self, min: I64Vec3, size: i64) -> Option<BlockType> {
        let overlaps = |lo: i64, from: i64, to: i64| lo < to && lo + size > from;
        let width = self.width();
        let trunk = overlaps(min.x, self.base.x, self.base.x + width)
            && overlaps(min.z, self.base.z, self.base.z + width)
            && overlaps(min.y, self.base.y, self.base.y + self.trunk);
        let offset = min.as_dvec3() + (size - 1) as f64 / 2.0 - self.base.as_dvec3();
        let leaves = if size >= COVERAGE_CELL {
            self.leaf_coverage(offset, size)
        } else {
            self.in_leaves(offset)
        };
        match (trunk, leaves) {
            (true, true) if size > 1 => Some(BlockType::Leaves),
            (true, _) if size >= THIN_TRUNK_CELL => None,
            (true, _) => Some(BlockType::Log),
            (false, _) if size == 1 && self.in_branch(offset) => Some(BlockType::Log),
            (false, true) => Some(BlockType::Leaves),
            (false, false) => None,
        }
    }

    // Whether enough of the cell centred `offset` from the trunk's base is among the leaves, judged
    // from a 3 x 3 x 3 spread of points through it.
    fn leaf_coverage(&self, offset: DVec3, size: i64) -> bool {
        let step = size as f64 / 3.0;
        let mut inside = 0;
        for x in -1..=1 {
            for y in -1..=1 {
                for z in -1..=1 {
                    let point = DVec3::new(x as f64, y as f64, z as f64) * step;
                    if self.in_leaves(offset + point) {
                        inside += 1;
                    }
                }
            }
        }
        inside >= COVERAGE_POINTS
    }

    /// The crown as `layers` stacked boxes, in blocks from `base`'s lower corner, for cells too
    /// wide to hold it: each layer is as wide as the leaves reach there, so a crown keeps its dome
    /// or tiers instead of being one cuboid.
    pub fn crown_boxes(&self, layers: i64) -> Vec<(DVec3, DVec3)> {
        let (low, high) = self.leaf_bounds();
        // one box is the bounds themselves, and a scan for the leaves' tighter ones is the dearest part of far LODs
        if layers == 1 {
            let (low, high) = (
                low.max(I64Vec3::new(-TREE_REACH, 0, -TREE_REACH)),
                high.min(I64Vec3::new(TREE_REACH, TREE_HEIGHT, TREE_REACH)),
            );
            return vec![(
                (self.base + low).as_dvec3(),
                (self.base + high + I64Vec3::ONE).as_dvec3(),
            )];
        }
        // only each row's first and last leaf, found from its ends, bear on the layers' bounds
        let mut leaves = Vec::new();
        let xs = low.x.max(-TREE_REACH)..=high.x.min(TREE_REACH);
        for y in low.y.max(0)..=high.y.min(TREE_HEIGHT) {
            for z in low.z.max(-TREE_REACH)..=high.z.min(TREE_REACH) {
                let leaf = |x: i64| self.in_leaves(DVec3::new(x as f64, y as f64, z as f64));
                if let Some(first) = xs.clone().find(|&x| leaf(x)) {
                    let last = xs.clone().rev().find(|&x| leaf(x)).unwrap_or(first);
                    leaves.extend([I64Vec3::new(first, y, z), I64Vec3::new(last, y, z)]);
                }
            }
        }
        let (Some(low), Some(high)) = (
            leaves.iter().map(|l| l.y).min(),
            leaves.iter().map(|l| l.y).max(),
        ) else {
            return Vec::new();
        };
        let height = high - low + 1;
        let layer = |y: i64| ((y - low) * layers / height) as usize;
        let mut bounds: Vec<Option<(I64Vec3, I64Vec3)>> = vec![None; layers as usize];
        for leaf in &leaves {
            let (min, max) = bounds[layer(leaf.y)].get_or_insert((*leaf, *leaf));
            *min = min.min(*leaf);
            *max = max.max(*leaf);
        }
        bounds
            .into_iter()
            .flatten()
            .map(|(min, max)| {
                (
                    (self.base + min).as_dvec3(),
                    (self.base + max + I64Vec3::ONE).as_dvec3(),
                )
            })
            .collect()
    }

    // Whole-block offsets from the trunk's base that every point `in_leaves` lies within.
    fn leaf_bounds(&self) -> (I64Vec3, I64Vec3) {
        let (low, high) = match self.kind {
            TreeKind::Pine => {
                let wrap = self.width() as f64 / 2.0 + 1.0;
                let reach = (0.6 + self.spread).max(wrap);
                let axis = self.axis();
                (
                    DVec3::new(axis.x - reach, (self.trunk / 4) as f64, axis.z - reach),
                    DVec3::new(axis.x + reach, (self.trunk + 1) as f64, axis.z + reach),
                )
            }
            _ => std::iter::once(&self.crown)
                .chain(&self.lobes)
                .map(|blob| {
                    let reach = DVec3::new(blob.radius, blob.radius * blob.squash, blob.radius);
                    (blob.centre - reach, blob.centre + reach)
                })
                .fold((DVec3::MAX, DVec3::MIN), |(low, high), (min, max)| {
                    (low.min(min), high.max(max))
                }),
        };
        (low.floor().as_i64vec3(), high.ceil().as_i64vec3())
    }

    // Whether the point `offset` from the trunk's base is among the leaves.
    fn in_leaves(&self, offset: DVec3) -> bool {
        // nibbled at the edges, so crowns aren't perfect balls
        let nibble = || {
            let rounded = offset.round().as_i64vec3();
            if hash(self.seed, rounded.x * 7919 + rounded.y, rounded.z).is_multiple_of(3) {
                NIBBLE
            } else {
                0.0
            }
        };
        match self.kind {
            TreeKind::Pine => {
                let (bottom, top) = ((self.trunk / 4) as f64, (self.trunk + 1) as f64);
                if !(bottom..=top).contains(&offset.y) {
                    return false;
                }
                let t = (offset.y - bottom) / (top - bottom);
                // each tier is widest at its foot and drawn in towards the next
                let tier = 1.0 - 0.3 * ((offset.y.round() as i64).rem_euclid(3) as f64 / 2.0);
                // never too narrow to wrap the trunk, which runs up to the tip
                let wrap = self.width() as f64 / 2.0 + 1.0;
                // narrowing over the last two blocks to a single one at the tip, which the trunk's top stays hidden under
                let wrap = wrap - (offset.y - (top - 2.0)).clamp(0.0, 2.0) * 0.5;
                let radius = ((0.6 + self.spread * (1.0 - t)) * tier - nibble() * 0.5).max(wrap);
                let across = offset - self.axis();
                across.x * across.x + across.z * across.z <= radius * radius
            }
            _ => {
                self.crown.contains(offset, nibble)
                    || self.lobes.iter().any(|lobe| lobe.contains(offset, nibble))
            }
        }
    }

    // Whether the point `offset` from the trunk's base is on a branch: out to a lobe on a broadleaf, a whorl of boughs at each tier on a pine.
    fn in_branch(&self, offset: DVec3) -> bool {
        if self.kind == TreeKind::Pine {
            return self.in_bough(offset);
        }
        // an umbrella's branches leave the trunk's top flat, a broadleaf's from 60% of the way up, rising to its lobes
        let umbrella = self.kind == TreeKind::Umbrella;
        let fork_y = if umbrella {
            self.crown.centre.y - self.crown.radius * self.crown.squash - 0.5
        } else {
            self.trunk as f64 * 0.6
        };
        let fork = self.axis() + DVec3::Y * fork_y;
        self.lobes.iter().filter(|lobe| lobe.radius > 0.0).any(|lobe| {
            let along = if umbrella { (lobe.centre - fork).with_y(0.0) } else { lobe.centre - fork };
            let t = ((offset - fork).dot(along) / along.length_squared()).clamp(0.0, 1.0);
            (offset - (fork + along * t)).length_squared() <= 0.36
        })
    }

    // Whether the point `offset` from the trunk's base is on one of the boughs a pine puts out, drooping, at the foot of each tier, which
    // reach most of the way to the crown's edge there so the inside of the crown is not an empty trunk.
    fn in_bough(&self, offset: DVec3) -> bool {
        let across = offset - self.axis();
        if across.x * across.x + across.z * across.z > (self.spread + 1.6).powi(2) {
            return false;
        }
        // the whorls whose boughs can be near this height, which droop by up to a third of their reach
        let bottom = (self.trunk / 4) as f64 + PINE_FIRST_WHORL;
        let first = ((offset.y - 1.5 - bottom) / PINE_WHORL).ceil().max(0.0) as i64;
        let last = ((offset.y + 0.6 - bottom) / PINE_WHORL).floor() as i64;
        (first..=last).any(|k| {
            let Some(y) = whorl_height(self.trunk, k) else {
                return false;
            };
            let start = self.axis() + DVec3::Y * y;
            (0..PINE_BOUGHS).any(|j| {
                let along = bough_reach(self.seed, self.trunk, self.spread, k, j);
                let t = ((offset - start).dot(along) / along.length_squared()).clamp(0.0, 1.0);
                (offset - (start + along * t)).length_squared() <= 0.36
            })
        })
    }

    // Puts the tree into a grid of `size`-block cells whose cell 0 has `origin` as its lowest
    // block, calling `place` with each cell index in `first..=last` that the tree fills.
    pub fn plant(
        &self,
        origin: I64Vec3,
        size: i64,
        (first, last): (IVec3, IVec3),
        mut place: impl FnMut(IVec3, BlockType),
    ) {
        // the leaves, the trunk, and the branches running between them, within a block of each
        let (leaves_low, leaves_high) = self.leaf_bounds();
        let width = self.width();
        let low = self.base + leaves_low.min(I64Vec3::ZERO) - 1;
        let high = self.base + leaves_high.max(I64Vec3::new(width, self.trunk, width) - 1) + 1;
        let cell_of = |block: I64Vec3| (block - origin).div_euclid(I64Vec3::splat(size)).as_ivec3();
        let (low, high) = (cell_of(low).max(first), cell_of(high).min(last));
        for x in low.x..=high.x {
            for z in low.z..=high.z {
                for y in low.y..=high.y {
                    let cell = IVec3::new(x, y, z);
                    if let Some(block) = self.block_in_cell(origin + cell.as_i64vec3() * size, size)
                    {
                        place(cell, block);
                    }
                }
            }
        }
    }
}

/// How far a trunk's top may lean from its foot, in blocks, which the drawn trunk keeps within.
pub const MAX_LEAN: f32 = 0.5;

/// What the mesher knows of the tree a voxel belongs to, found by which trunk is nearest across.
#[derive(Debug, Clone, Copy)]
pub struct TreeLook {
    axis: Vec2,
    pub tint: Vec3,
    /// The trunk's lowest block, in cells from the origin.
    pub base: Vec3,
    /// Logs up the trunk, in cells.
    pub height: f32,
    /// How many cells across the trunk is.
    pub width: f32,
    /// How thick the trunk is drawn, as a radius in cells.
    pub radius: f32,
    /// Whether it is a pine, whose crown is narrow tiers.
    pub pine: bool,
    seed: u64,
    spread: f32,
    /// How far the top of the trunk leans from its foot, in cells.
    pub lean: Vec2,
    /// How big its leaf cards are against others'.
    pub card_scale: f32,
    // a broadleaf's crown and lobes, in cells from the origin, none on a pine
    crown: [CrownBall; 1 + LOBES],
}

/// One of the few big cards a far tree's crown is drawn as, see `Tree::far_cards`.
#[derive(Debug, Clone, Copy)]
pub struct FarCard {
    pub centre: DVec3,
    pub facing: Vec3,
    pub size: f32,
}

/// A ball of leaves squashed vertically, as `TreeLook` has it.
#[derive(Debug, Clone, Copy, Default)]
struct CrownBall {
    centre: Vec3,
    radius: f32,
    squash: f32,
}

impl TreeLook {
    /// Which way the crown's surface faces at `at`, in cells, as a ball's would: out from the middle of the ball it is nearest the edge of.
    /// Zero for a pine, and at a ball's very middle.
    pub fn crown_normal(&self, at: Vec3) -> Vec3 {
        let nearest = self
            .crown
            .iter()
            .filter(|ball| ball.radius > 0.0)
            .min_by(|a, b| {
                let edge = |ball: &CrownBall| {
                    let d = at - ball.centre;
                    (Vec3::new(d.x, d.y / ball.squash, d.z).length() / ball.radius - 1.0).abs()
                };
                edge(a).total_cmp(&edge(b))
            });
        let Some(ball) = nearest else {
            return Vec3::ZERO;
        };
        let d = at - ball.centre;
        // the gradient of the squashed ball's equation
        Vec3::new(d.x, d.y / (ball.squash * ball.squash), d.z).normalize_or_zero()
    }

    /// The reach from the trunk's axis of each bough that starts at the trunk's log at `coord`, in blocks: a pine's whorl is at some of its heights.
    pub fn boughs_from(&self, coord: IVec3) -> Vec<Vec3> {
        if !self.pine {
            return Vec::new();
        }
        let trunk = self.height as i64;
        let up = coord.y as i64 - self.base.y as i64 - trunk / 4 - PINE_FIRST_WHORL as i64;
        if up < 0 || up % PINE_WHORL as i64 != 0 {
            return Vec::new();
        }
        let k = up / PINE_WHORL as i64;
        if whorl_height(trunk, k).is_none() {
            return Vec::new();
        }
        (0..PINE_BOUGHS)
            .map(|j| bough_reach(self.seed, trunk, self.spread as f64, k, j).as_vec3())
            .collect()
    }

    /// Whether the log at `coord` is in the trunk's own column, not a branch.
    pub fn in_trunk(&self, coord: IVec3) -> bool {
        let at = coord.as_vec3() - self.base;
        at.x > -0.5
            && at.z > -0.5
            && at.y > -0.5
            && at.x < self.width - 0.5
            && at.z < self.width - 0.5
            && at.y < self.height - 0.5
    }
}

/// Which tree each block belongs to, for blocks placed one at a time, where which tree put
/// them is lost: the one whose trunk is nearest across, which is the one they belong to
/// but where crowns meet.
#[derive(Default, Debug, Clone)]
pub struct TreeTints(Vec<TreeLook>);

impl TreeTints {
    /// Every tree in `trees`, placed by its trunk's axis less `origin`, in cells `cell` blocks
    /// wide whose centres are whole numbers.
    pub fn new<'a>(trees: impl IntoIterator<Item = &'a Tree>, origin: I64Vec3, cell: i64) -> Self {
        let cells = cell as f32;
        Self(
            trees
                .into_iter()
                .map(|t| {
                    let axis = (t.base - origin).as_dvec3() + t.axis();
                    TreeLook {
                        axis: Vec2::new(axis.x as f32, axis.z as f32) / cells
                            - (cell - 1) as f32 / (2.0 * cells),
                        tint: t.tint(),
                        base: (t.base - origin).as_vec3() / cells,
                        height: t.trunk as f32 / cells,
                        width: t.width() as f32 / cells,
                        radius: t.trunk_radius() / cells,
                        pine: t.kind == TreeKind::Pine,
                        seed: t.seed,
                        spread: t.spread as f32,
                        lean: t.lean() / cells,
                        card_scale: t.card_scale(),
                        crown: if t.kind == TreeKind::Pine {
                            [CrownBall::default(); 1 + LOBES]
                        } else {
                            let base = (t.base - origin).as_vec3();
                            std::array::from_fn(|i| {
                                let ball = if i == 0 { &t.crown } else { &t.lobes[i - 1] };
                                CrownBall {
                                    centre: (base + ball.centre.as_vec3()) / cells,
                                    radius: ball.radius as f32 / cells,
                                    squash: ball.squash as f32,
                                }
                            })
                        },
                    }
                })
                .collect(),
        )
    }

    /// The tree the block at `coord`, in cells, belongs to.
    pub fn look_at(&self, coord: IVec3) -> Option<TreeLook> {
        let p = Vec2::new(coord.x as f32, coord.z as f32);
        self.0
            .iter()
            .min_by(|a, b| {
                a.axis
                    .distance_squared(p)
                    .total_cmp(&b.axis.distance_squared(p))
            })
            .copied()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The tint of the leaf at `coord`, in cells.
    pub fn at(&self, coord: IVec3) -> Vec3 {
        self.look_at(coord).map_or(Vec3::ONE, |look| look.tint)
    }

    pub fn heap_size(&self) -> usize {
        self.0.capacity() * size_of::<TreeLook>()
    }
}

// Whether a tree's `block` may go where `existing` is: into air, and logs through leaves.
pub fn grows_into(existing: BlockType, block: BlockType) -> bool {
    existing == BlockType::Air || (existing == BlockType::Leaves && block == BlockType::Log)
}

/// Which tree grows on ground `above_sea` blocks up in the biome called `biome`, where the air is
/// `warmth` warm up there. Pines in the cold, and on a mountain's upper slopes even in the tropics;
/// the hot biomes keep their broadleaf trees however high the ground or cool the water's edge, so no
/// savanna has pines in it.
pub(crate) fn tree_kind(
    biome: Option<&str>,
    mountain: bool,
    above_sea: i64,
    warmth: f32,
) -> TreeKind {
    let hot = matches!(biome, Some("savanna" | "jungle" | "desert"));
    let cold = matches!(biome, Some("taiga" | "tundra" | "glacier"));
    let alpine = mountain && above_sea >= ALPINE_PINES_ABOVE;
    if alpine || (!hot && (cold || warmth < PINE_TEMPERATURE)) {
        TreeKind::Pine
    } else {
        match biome {
            Some("savanna") => TreeKind::Umbrella,
            Some("jungle") => TreeKind::Slender,
            _ => TreeKind::Broadleaf,
        }
    }
}

// Every tree that could reach into the columns `min..=max` (x, z).
pub fn trees_near(
    noise: &dyn TerrainSource,
    heights: &mut impl TreeGround,
    min: I64Vec2,
    max: I64Vec2,
) -> Vec<Tree> {
    let first = (min - TREE_REACH).div_euclid(I64Vec2::splat(CELL));
    let last = (max + TREE_REACH).div_euclid(I64Vec2::splat(CELL));
    let mut trees = Vec::new();
    for cz in first.y..=last.y {
        for cx in first.x..=last.x {
            if let Some(tree) = tree_in_cell(noise, heights, cx, cz) {
                trees.push(tree);
            }
        }
    }
    trees
}

fn tree_in_cell(
    noise: &dyn TerrainSource,
    heights: &mut impl TreeGround,
    cx: i64,
    cz: i64,
) -> Option<Tree> {
    let seed = hash(noise.seed() as u64, cx, cz);
    let x = cx * CELL + ((seed >> 16) % CELL as u64) as i64;
    let z = cz * CELL + ((seed >> 24) % CELL as u64) as i64;
    let chance = (seed & 0xffff) as f64 / 65536.0;
    if heights.rules_out(x, z, chance) || chance >= heights.trees(x, z) {
        return None;
    }

    // the biome before the slope, which samples elsewhere, while the ground here is to hand
    let height = heights.height(x, z).round() as i64;
    // a trunk is more than a column wide, and stands on ground level with its foot: not across a
    // step or a cliff's edge, where its far side would be buried or hang in the air
    for (dx, dz) in [(1, 0), (0, 1), (1, 1)] {
        let beside = heights.height(x + dx, z + dz).round() as i64;
        if !(height - 1..=height).contains(&beside) {
            return None;
        }
    }
    if heights.rocky(x, z) {
        return None;
    }
    let biome = heights.biome(x, z);
    let (gradient_x, gradient_z) = heights.gradient(x, z);
    // meadow clings to a mountain's faces, and trees stand on it
    let gradient = super::generator::block_gradient(
        noise.biomes(),
        biome,
        noise.seed(),
        x,
        z,
        gradient_x.abs() + gradient_z.abs(),
    );
    if gradient > MAX_GRADIENT {
        return None;
    }
    let temperature = noise.temperature(x as f64, z as f64);
    let ground = column_block(
        noise.biomes(),
        biome,
        height - 1,
        height,
        gradient,
        temperature,
        WATER_LEVEL,
    );
    if ground != BlockType::Grass {
        return None;
    }

    // 0..1, how big this one grew
    let growth = ((seed >> 32) % 1000) as f64 / 1000.0;
    // how warm it is up where the tree stands, which falls with height
    let climate = noise.climate(x as f64, z as f64);
    let warmth = climate.temperature;
    let mountain = noise.biomes().modifier(biome) != Modifier::None;
    let kind_here = tree_kind(
        noise.biomes().name(biome),
        mountain,
        height - WATER_LEVEL,
        warmth,
    );
    // an emergent giant stands over the canopy: a few of the biome's broadleaf trees
    let giant = kind_here == TreeKind::Slender
        && ((seed >> 20) % 1000) as f64 / 1000.0 < noise.biomes().tree_scale(biome).giants;
    let (kind, trunk, spread, squash) =
        scaled_tree(kind_here, growth, noise.biomes().tree_scale(biome), giant);
    Some(Tree::new(
        kind,
        I64Vec3::new(x, heights.base_height(x, z), z),
        trunk,
        spread,
        squash,
        seed,
    )
    .with_exposure(
        0.5 * (1.0 - f64::from(warmth)).clamp(0.0, 1.0)
            + 0.5 * (1.0 - f64::from(climate.humidity)).clamp(0.0, 1.0),
    ))
}

// The kind, trunk height, crown spread and crown squash of a tree that grew `growth` (0..1) of the way to its
// largest, in a biome that scales trees by `scale`: held to `TREE_HEIGHT` and `TREE_REACH`, which the chunk and
// LOD code size their boxes by. A giant is a jungle's slender tree grown into an umbrella standing over the canopy.
pub(crate) fn scaled_tree(
    kind: TreeKind,
    growth: f64,
    scale: TreeScale,
    giant: bool,
) -> (TreeKind, i64, f64, f64) {
    let giant = giant && kind == TreeKind::Slender;
    let shape = if giant { TreeKind::Umbrella } else { kind };
    // the widest crown a shape may have: an umbrella's lobes reach 1.35 times it from the trunk
    let widest = match shape {
        TreeKind::Pine | TreeKind::Slender => 6.0,
        TreeKind::Broadleaf => 7.0,
        TreeKind::Umbrella if giant => (TREE_REACH - 1) as f64 / 1.35,
        TreeKind::Umbrella => 9.0,
    };
    let (mut trunk, mut spread, squash) = grown(shape, growth);
    if giant {
        trunk = (grown(kind, growth).0 as f64 * GIANT_HEIGHT) as i64;
        spread *= GIANT_SPREAD;
    }
    let spread = (spread * scale.spread).clamp(3.0, widest);
    let squash = squash * scale.squash;
    let tallest = match shape {
        TreeKind::Pine => TREE_HEIGHT - 2,
        // the crown's top stands `spread * squash` over the trunk's
        _ => TREE_HEIGHT - 2 - (spread * squash).ceil() as i64,
    };
    let trunk = ((trunk as f64 * scale.height).round() as i64).clamp(6, tallest);
    (shape, trunk, spread, squash)
}

// The trunk height, crown spread and crown squash of a `kind` that grew `growth` of the way to its largest, in the usual biome.
fn grown(kind: TreeKind, growth: f64) -> (i64, f64, f64) {
    match kind {
        TreeKind::Pine => (16 + (growth * 9.0) as i64, 4.5 + growth * 1.5, 0.7),
        TreeKind::Broadleaf => (10 + (growth * 6.0) as i64, 5.0 + growth * 2.0, 0.7),
        TreeKind::Umbrella => (12 + (growth * 6.0) as i64, 5.5 + growth * 2.5, 0.3),
        TreeKind::Slender => (16 + (growth * 8.0) as i64, 3.5 + growth * 1.5, 0.85),
    }
}

/// How high up a pine's trunk, in blocks, the `k`th whorl of boughs starts, or none above the crown.
pub(crate) fn whorl_height(trunk: i64, k: i64) -> Option<f64> {
    let y = (trunk / 4) as f64 + PINE_FIRST_WHORL + k as f64 * PINE_WHORL;
    (k >= 0 && y <= (trunk + 1) as f64 - 2.0).then_some(y)
}

/// The `j`th bough of the `k`th whorl of a pine: how far it reaches from the trunk's axis, down a third of its length.
pub(crate) fn bough_reach(seed: u64, trunk: i64, spread: f64, k: i64, j: usize) -> DVec3 {
    let (bottom, top) = ((trunk / 4) as f64, (trunk + 1) as f64);
    let y = bottom + PINE_FIRST_WHORL + k as f64 * PINE_WHORL;
    let t = (y - bottom) / (top - bottom);
    let reach = (0.6 + spread * (1.0 - t)) * PINE_BOUGH_REACH;
    let turn = (j as f64 + (hash(seed, 700 + k, j as i64) % 1000) as f64 / 1000.0)
        * std::f64::consts::TAU
        / PINE_BOUGHS as f64;
    let (sin, cos) = libm::sincos(turn);
    DVec3::new(cos * reach, -reach / 3.0, sin * reach)
}

// splitmix64 over a seed and two coordinates
pub(super) fn hash(seed: u64, a: i64, b: i64) -> u64 {
    let mut h = seed
        ^ (a as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (b as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
    h = (h ^ (h >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    h = (h ^ (h >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    h ^ (h >> 31)
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::generate::region_store::RegionStore;

    fn tree(kind: TreeKind, trunk: i64, spread: f64) -> Tree {
        Tree::new(kind, I64Vec3::new(100, 500, -40), trunk, spread, 0.7, 7)
    }

    #[test]
    fn a_default_scale_changes_no_tree_and_every_biomes_trees_stay_inside_the_caps() {
        for growth in [0.0, 0.3, 0.999] {
            let (_, trunk, spread, squash) =
                scaled_tree(TreeKind::Pine, growth, TreeScale::default(), false);
            assert_eq!(
                (16 + (growth * 9.0) as i64, 4.5 + growth * 1.5, 0.7),
                (trunk, spread, squash)
            );
            let (_, trunk, spread, squash) =
                scaled_tree(TreeKind::Broadleaf, growth, TreeScale::default(), false);
            assert_eq!(
                (10 + (growth * 6.0) as i64, 5.0 + growth * 2.0, 0.7),
                (trunk, spread, squash)
            );
        }
        let biomes = ochre_base::biome::Biomes::new(1);
        for name in [
            "glacier", "desert", "forest", "tundra", "taiga", "steppe", "savanna", "jungle",
        ] {
            let scale = biomes.tree_scale(biomes.id(name).unwrap());
            for kind in [TreeKind::Pine, TreeKind::Broadleaf, TreeKind::Umbrella, TreeKind::Slender] {
                for growth in [0.0, 0.5, 0.999] {
                    for giant in [false, true] {
                        let (kind, trunk, spread, squash) = scaled_tree(kind, growth, scale, giant);
                        let (low, high) =
                            Tree::new(kind, I64Vec3::ZERO, trunk, spread, squash, 3).bounds();
                        // a thick trunk is two blocks wide, one past the reach on its far side
                        assert!(
                            low.x >= -TREE_REACH
                                && high.x <= TREE_REACH + 1
                                && low.z >= -TREE_REACH
                                && high.z <= TREE_REACH + 1,
                            "{name} {kind:?} {growth}: {low} {high}"
                        );
                        assert!(
                            high.y <= TREE_HEIGHT,
                            "{name} {kind:?} {growth}: top {}",
                            high.y
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn umbrellas_are_flat_and_slender_trees_are_tall_and_narrow_against_a_broadleaf() {
        // the trunk, and the crown's depth and width
        let size = |kind| {
            let (kind, trunk, spread, squash) = scaled_tree(kind, 0.5, TreeScale::default(), false);
            let tree = Tree::new(kind, I64Vec3::ZERO, trunk, spread, squash, 3);
            let (low, high) = tree.leaf_bounds();
            (trunk as f64, tree.crown_top() - tree.crown_bottom(), (high.x - low.x) as f64)
        };
        let (broad_trunk, _, broad_width) = size(TreeKind::Broadleaf);
        let (_, umbrella_depth, umbrella_width) = size(TreeKind::Umbrella);
        let (slender_trunk, _, slender_width) = size(TreeKind::Slender);
        // the umbrella's crown is wider than a broadleaf's and under half as deep as it is wide
        assert!(umbrella_width > broad_width && umbrella_depth < umbrella_width / 2.0);
        // the slender crown is narrower than a broadleaf's, on a taller trunk
        assert!(slender_width < broad_width && slender_trunk > broad_trunk);
    }

    #[test]
    fn jungle_giants_stand_well_over_the_canopy_and_nothing_else_grows_giants() {
        let biomes = ochre_base::biome::Biomes::new(1);
        let jungle = biomes.tree_scale(biomes.id("jungle").unwrap());
        assert!(jungle.giants > 0.0);
        for growth in [0.0, 0.5, 0.999] {
            let (_, canopy, _, _) = scaled_tree(TreeKind::Slender, growth, jungle, false);
            let (kind, giant, wide, _) = scaled_tree(TreeKind::Slender, growth, jungle, true);
            assert_eq!(TreeKind::Umbrella, kind);
            assert!(
                giant >= canopy + 12,
                "giant {giant} over canopy {canopy} at {growth}"
            );
            assert!(giant >= 29 && wide >= 6.5, "giant {giant} wide {wide}");
        }
        // no other biome has any, and a pine is never a giant
        for name in [
            "glacier", "desert", "forest", "tundra", "taiga", "steppe", "savanna",
        ] {
            assert_eq!(
                0.0,
                biomes.tree_scale(biomes.id(name).unwrap()).giants,
                "{name}"
            );
        }
        let flat = scaled_tree(TreeKind::Pine, 0.5, jungle, true);
        assert_eq!(flat, scaled_tree(TreeKind::Pine, 0.5, jungle, false));
    }

    // the smallest and largest each kind grows
    fn extremes() -> [Tree; 4] {
        [
            tree(TreeKind::Broadleaf, 10, 5.0),
            tree(TreeKind::Broadleaf, 15, 7.0),
            tree(TreeKind::Pine, 16, 4.5),
            tree(TreeKind::Pine, 24, 6.0),
        ]
    }

    fn blocks(tree: &Tree) -> Vec<(I64Vec3, BlockType)> {
        let (low, high) = tree.bounds();
        let mut blocks = Vec::new();
        tree.plant(
            low,
            1,
            (IVec3::ZERO, (high - low).as_ivec3()),
            |cell, block| {
                blocks.push((low + cell.as_i64vec3(), block));
            },
        );
        blocks
    }

    #[test]
    fn test_leaves_lie_within_their_bounds() {
        for tree in extremes() {
            let (low, high) = tree.leaf_bounds();
            for y in -2..=TREE_HEIGHT + 2 {
                for x in -TREE_REACH - 2..=TREE_REACH + 2 {
                    for z in -TREE_REACH - 2..=TREE_REACH + 2 {
                        let at = I64Vec3::new(x, y, z);
                        if tree.in_leaves(at.as_dvec3()) {
                            assert!(at.cmpge(low).all() && at.cmple(high).all(), "{at}");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn test_trees_are_a_trunk_under_big_crowns() {
        for t in extremes() {
            let blocks = blocks(&t);
            let in_trunk = |p: I64Vec3| {
                (0..t.width()).contains(&(p.x - t.base.x))
                    && (0..t.width()).contains(&(p.z - t.base.z))
            };
            let trunk_logs = blocks
                .iter()
                .filter(|(p, b)| *b == BlockType::Log && in_trunk(*p))
                .count();
            assert_eq!((t.trunk * t.width() * t.width()) as usize, trunk_logs);
            let branches = blocks
                .iter()
                .filter(|(p, b)| *b == BlockType::Log && !in_trunk(*p))
                .count();
            assert!(branches > 0, "{:?} has no branch logs", t.kind);
            let leaves = blocks
                .iter()
                .filter(|(_, b)| *b == BlockType::Leaves)
                .count();
            assert!(leaves > 200, "{:?} has only {leaves} leaves", t.kind);
            // the trunk's top is hidden in the leaves, all the way round
            let top = t.base + I64Vec3::Y * (t.trunk - 1);
            assert_eq!(
                Some(BlockType::Leaves),
                t.block_in_cell(top + I64Vec3::Y, 1)
            );
            for side in [
                I64Vec3::NEG_X,
                I64Vec3::NEG_Z,
                I64Vec3::X * t.width(),
                I64Vec3::Z * t.width(),
            ] {
                assert_eq!(
                    Some(BlockType::Leaves),
                    t.block_in_cell(top + side, 1),
                    "{:?} trunk bare at {side}",
                    t.kind
                );
            }
        }
    }

    #[test]
    fn test_trees_stay_within_their_reach() {
        for t in extremes() {
            let (low, high) = (
                t.base - I64Vec3::new(TREE_REACH, 0, TREE_REACH),
                t.base + I64Vec3::new(TREE_REACH, TREE_HEIGHT, TREE_REACH),
            );
            // look well past the bounds, so anything poking out is caught
            for x in low.x - 4..=high.x + 4 {
                for z in low.z - 4..=high.z + 4 {
                    for y in low.y - 4..=high.y + 4 {
                        let p = I64Vec3::new(x, y, z);
                        if t.block_in_cell(p, 1).is_some() {
                            assert!(
                                p.cmpge(low).all() && p.cmple(high).all(),
                                "{:?} reaches {p}",
                                t.kind
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn test_coarse_cells_keep_the_trunk_and_crown() {
        let t = tree(TreeKind::Broadleaf, 12, 6.0);
        // 2-block cells aligned so the trunk's column isn't at a cell's centre
        let origin = I64Vec3::new(96, 496, -44);
        let mut seen = Vec::new();
        t.plant(
            origin,
            2,
            (IVec3::splat(-8), IVec3::splat(20)),
            |_, block| seen.push(block),
        );
        assert!(seen.contains(&BlockType::Log));
        assert!(seen.contains(&BlockType::Leaves));
        // from `THIN_TRUNK_CELL` the trunk is drawn as a box of its own, so the cells are crown alone
        let mut wide = Vec::new();
        t.plant(
            origin,
            THIN_TRUNK_CELL,
            (IVec3::splat(-4), IVec3::splat(10)),
            |_, block| wide.push(block),
        );
        assert!(wide.contains(&BlockType::Leaves) && !wide.contains(&BlockType::Log));
    }

    #[test]
    fn test_pines_grow_in_the_cold_and_up_mountains_but_never_on_the_hot_lowlands() {
        // savanna, jungle and desert lowlands, however cool: never pines
        for biome in ["savanna", "jungle", "desert"] {
            let kind = match biome {
                "savanna" => TreeKind::Umbrella,
                "jungle" => TreeKind::Slender,
                _ => TreeKind::Broadleaf,
            };
            for (above, warmth) in [(0, 0.8), (200, 0.3), (300, 0.1), (150, 0.44)] {
                assert_eq!(
                    kind,
                    tree_kind(Some(biome), false, above, warmth),
                    "{biome} {above} {warmth}"
                );
            }
        }
        // cold biomes, cool forest and grassland, and the upper slopes of any mountain, even in the tropics: pines
        for (biome, mountain, above, warmth) in [
            (Some("taiga"), false, 10, 0.9),
            (Some("tundra"), false, 10, 0.9),
            (None, false, 10, 0.4),
            (Some("forest"), false, 100, 0.4),
            (None, true, 150, 0.9),
            (Some("jungle"), true, 150, 0.9),
            (Some("forest"), true, 150, 0.9),
        ] {
            assert_eq!(
                TreeKind::Pine,
                tree_kind(biome, mountain, above, warmth),
                "{biome:?} {mountain} {above} {warmth}"
            );
        }
        // warm forest and a mountain's foot are broadleaf
        assert_eq!(
            TreeKind::Broadleaf,
            tree_kind(Some("forest"), false, 20, 0.7)
        );
        assert_eq!(TreeKind::Broadleaf, tree_kind(None, true, 60, 0.7));
    }

    #[test]
    fn test_logs_grow_through_leaves_but_nothing_into_ground() {
        assert!(grows_into(BlockType::Air, BlockType::Leaves));
        assert!(grows_into(BlockType::Leaves, BlockType::Log));
        assert!(!grows_into(BlockType::Log, BlockType::Leaves));
        assert!(!grows_into(BlockType::Grass, BlockType::Log));
        assert!(!grows_into(BlockType::Water, BlockType::Leaves));
    }

    #[test]
    fn test_trees_stand_on_grass_and_agree_between_lookups() {
        let noise = ochre_world::worldmap::test_terrain(5);
        let regions = RegionStore::new(noise.clone());
        let mut heights = regions.sampler();
        let centre = ochre_world::worldmap::forest_spot(&noise);
        let (min, max) = (centre - I64Vec2::splat(600), centre + I64Vec2::splat(600));
        let trees = trees_near(&*noise, &mut heights, min, max);
        assert!(!trees.is_empty(), "no trees in {min}..{max}");
        for t in &trees {
            let ground = heights.height(t.base.x, t.base.z).round() as i64;
            assert_eq!(ground, t.base.y);
        }
        // a smaller window finds the same trees where they overlap
        let some = trees_near(&*noise, &mut heights, centre, centre + I64Vec2::splat(40));
        for t in &some {
            assert!(trees
                .iter()
                .any(|o| o.base == t.base && o.trunk == t.trunk && o.kind == t.kind));
        }
    }
    // Every cell `size` blocks across the tree fills, from its bounds.
    fn cells(tree: &Tree, size: i64) -> Vec<(I64Vec3, BlockType)> {
        let (low, high) = tree.bounds();
        let origin = I64Vec3::new(
            low.x.div_euclid(size),
            low.y.div_euclid(size),
            low.z.div_euclid(size),
        ) * size;
        let mut cells = Vec::new();
        tree.plant(
            origin,
            size,
            (IVec3::ZERO, ((high - origin) / size).as_ivec3()),
            |cell, block| {
                cells.push((origin + cell.as_i64vec3() * size, block));
            },
        );
        cells
    }

    #[test]
    fn test_every_tree_leaves_a_crown_in_cells_four_wide() {
        for t in extremes() {
            // from eight blocks a cell LODs draw crown boxes, not planted cells, so only cells up to four hold a crown
            let leaves = cells(&t, 4)
                .into_iter()
                .filter(|(_, b)| *b == BlockType::Leaves)
                .count();
            assert!(
                leaves >= 2,
                "{:?} {} trunk: {leaves} leaf cells at four",
                t.kind,
                t.trunk
            );
        }
    }

    #[test]
    fn test_trunks_stand_in_cells_up_to_two_wide_and_not_beyond() {
        let t = tree(TreeKind::Pine, 24, 6.0);
        let logs = |size| {
            cells(&t, size)
                .into_iter()
                .filter(|(_, b)| *b == BlockType::Log)
                .count()
        };
        assert!(logs(1) > 0 && logs(2) > 0);
        assert_eq!(0, logs(THIN_TRUNK_CELL));
        assert_eq!(0, logs(8));
    }

    #[test]
    fn test_coverage_widens_a_crown_by_less_than_a_cell() {
        let t = tree(TreeKind::Broadleaf, 15, 7.0);
        for size in [4, 8] {
            let reach = cells(&t, size)
                .into_iter()
                .filter(|(_, b)| *b == BlockType::Leaves)
                .map(|(p, _)| {
                    (p.x + size / 2 - t.base.x)
                        .abs()
                        .max((p.z + size / 2 - t.base.z).abs())
                })
                .max()
                .unwrap();
            // a crown's lobes hang out to about 1.35 times its spread, and a cell edge can add up to a cell
            assert!(
                reach as f64 <= t.spread * 1.35 + size as f64,
                "reach {reach} at {size}"
            );
        }
    }
}

#[cfg(test)]
mod look_tests {
    use super::*;

    fn thick_tree(seed: u64) -> Tree {
        Tree::new(
            TreeKind::Broadleaf,
            I64Vec3::new(20, 5, 20),
            14,
            4.0,
            0.7,
            seed,
        )
    }

    #[test]
    fn a_trunks_lean_is_within_the_cap_and_not_always_none() {
        let leans: Vec<f32> = (0..200)
            .map(|seed| thick_tree(seed).lean().length())
            .collect();
        assert!(leans.iter().all(|&lean| lean <= MAX_LEAN + 1e-4));
        assert!(leans.iter().any(|&lean| lean > 0.1));
    }

    #[test]
    fn colder_drier_ground_leans_a_trunk_further_and_a_broadleaf_has_three_to_five_lobes() {
        let mut counts = std::collections::HashSet::new();
        for seed in 0..200 {
            let tree = thick_tree(seed);
            let lobes = tree.lobes.iter().filter(|lobe| lobe.radius > 0.0).count();
            assert!((3..=5).contains(&lobes), "{lobes} lobes");
            counts.insert(lobes);
            let (cold, mild) = (tree.with_exposure(1.0).lean(), tree.with_exposure(0.0).lean());
            assert!(cold.length() >= mild.length() - 1e-6 && cold.length() <= MAX_LEAN + 1e-4);
        }
        assert_eq!(3, counts.len());
    }

    #[test]
    fn a_crown_stands_off_its_trunk_but_still_covers_the_top_of_it() {
        for seed in 0..100 {
            let tree = thick_tree(seed);
            let top = tree.base + I64Vec3::Y * tree.trunk;
            assert_eq!(Some(BlockType::Leaves), tree.block_in_cell(top, 1), "seed {seed}");
        }
    }

    #[test]
    fn only_the_logs_of_the_trunk_column_are_in_the_trunk() {
        for (trunk, seed) in [(14, 3), (8, 9)] {
            let tree = Tree::new(
                TreeKind::Broadleaf,
                I64Vec3::new(20, 5, 20),
                trunk,
                4.0,
                0.7,
                seed,
            );
            let tints = TreeTints::new([&tree], I64Vec3::ZERO, 1);
            let mut in_trunk = 0;
            let mut branches = 0;
            tree.plant(
                I64Vec3::ZERO,
                1,
                (IVec3::ZERO, IVec3::splat(48)),
                |coord, block| {
                    if block != BlockType::Log {
                        return;
                    }
                    let look = tints.look_at(coord).unwrap();
                    let width = tree.width() as i32;
                    let column = (20..20 + width).contains(&coord.x)
                        && (20..20 + width).contains(&coord.z)
                        && (5..5 + trunk as i32).contains(&coord.y);
                    assert_eq!(column, look.in_trunk(coord), "{coord}");
                    if column {
                        in_trunk += 1;
                    } else {
                        branches += 1;
                    }
                },
            );
            assert_eq!(trunk as i32 * (tree.width() as i32).pow(2), in_trunk);
            let _ = branches;
        }
    }
}
