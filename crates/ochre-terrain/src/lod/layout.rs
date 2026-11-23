use super::key::LodKey;
use crate::chunk::ChunkCoordinate;
use crate::chunk::CHUNK_SIZE;
use crate::generate::columns::COARSE_CELL;

/// The narrowest level 1's ring may be, in chunks: two nodes, so full detail never steps straight to level 2.
const LEVEL_ONE_MIN_RING: f32 = 4.0;

/// How the world is split into detail levels, set from the renderer settings.
#[derive(Debug, Copy, Clone)]
pub struct LodLayout {
    /// Chunks within this many chunks of the camera are drawn at full detail.
    pub full_detail_distance: f32,
    // Level n reaches this many chunks times `level_scale` to the n; full detail can reach further.
    lod_base: f32,
    /// Each LOD level up to the set render distance reaches this many times as far as the one before.
    pub level_scale: f32,
    // Levels out to the render distance; levels beyond it (high-up views) each reach only twice as far.
    scaled_levels: u8,
    pub max_level: u8,
    render_distance: f32,
}

impl LodLayout {
    /// The fewest LOD levels that draw out to `render_distance` chunks, given the full detail range and `level_scale`.
    pub fn new(full_detail_distance: u32, render_distance: u32, level_scale: u32) -> Self {
        let full_detail_distance = full_detail_distance.max(1) as f32;
        let layout = Self {
            full_detail_distance,
            lod_base: full_detail_distance,
            // below 2, a level's nodes would be wider than the ring it draws
            level_scale: level_scale.max(2) as f32,
            scaled_levels: u8::MAX,
            max_level: 0,
            render_distance: 0.0,
        };
        let layout = layout.with_render_distance(render_distance as f32);
        Self {
            scaled_levels: layout.max_level,
            ..layout
        }
    }

    /// Full detail out to `distance` chunks; only level 1's ring moves, keeping `LEVEL_ONE_MIN_RING`.
    pub fn with_full_detail_distance(self, distance: u32) -> Self {
        let full_detail_distance = (distance.max(1) as f32).min(self.reach(1) - LEVEL_ONE_MIN_RING);
        Self {
            full_detail_distance,
            ..self
        }
        .with_render_distance(self.render_distance)
    }

    /// Draws out to `render_distance` chunks, adding or dropping outer levels; never less than full detail reaches.
    pub fn with_render_distance(self, render_distance: f32) -> Self {
        let mut layout = Self {
            max_level: 0,
            render_distance: render_distance.max(self.full_detail_distance),
            ..self
        };
        while layout.reach(layout.max_level) < layout.render_distance {
            layout.max_level += 1;
        }
        layout
    }

    /// How far, in chunks, terrain is drawn.
    pub fn render_distance(self) -> f32 {
        self.render_distance
    }

    /// How far, in chunks, `level` draws out to - level 0 being full detail.
    pub fn reach(self, level: u8) -> f32 {
        if level == 0 {
            return self.full_detail_distance;
        }
        let scaled = level.min(self.scaled_levels);
        let doubled = level - scaled;
        self.lod_base * self.level_scale.powi(scaled as i32) * 2f32.powi(doubled as i32)
    }

    /// Chunks out to which terrain is built from full-detail regions; coarser nodes sample noise.
    pub fn region_reach(self) -> f32 {
        let fine_levels = (0..=self.max_level).take_while(|&level| (1i64 << level) < COARSE_CELL);
        self.reach(fine_levels.last().unwrap_or(0))
    }

    /// Whether `key` is drawn as its 8 children: it's within reach of the next level down.
    pub fn splits(self, key: LodKey, anchor: ChunkCoordinate) -> bool {
        key.level > 0 && key.distance(anchor) < self.reach(key.level - 1)
    }

    pub fn full_detail(self, chunk: ChunkCoordinate, anchor: ChunkCoordinate) -> bool {
        self.splits(LodKey::containing(chunk, 1), anchor)
    }

    /// How far out, in chunks, the farthest LOD level starts: halfway out with no LOD levels.
    pub fn last_level_start(self) -> f32 {
        match self.max_level {
            0 => self.render_distance / 2.0,
            level => self.reach(level - 1),
        }
    }

    /// How far full-detail chunks reach: a level-1 node in range, plus its diagonal.
    pub fn full_detail_reach(self) -> u32 {
        (self.full_detail_distance + 2.0 * 3f32.sqrt()).ceil() as u32
    }

    /// Where ground flora shrinks away: shortest within full detail, tallest out into level 1, so no edge shows.
    pub fn flora_fade(self) -> FloraFade {
        let chunk = CHUNK_SIZE as f32;
        // a level-1 node in range draws as chunks, and the camera stands anywhere in its own chunk
        let mid = (self.full_detail_distance - 1.0).max(1.0) * chunk;
        // and the farthest level-1 node is a node short of its reach, past which level 2 draws no flora
        let level_one = (self.reach(1) - 2.0) * chunk;
        FloraFade {
            start: mid * FloraFade::START,
            mid,
            end: (mid + (level_one - mid) * FloraFade::END).max(mid),
        }
    }
}

/// How ground flora shrinks into its roots with distance in blocks: all start at `start`, short ones gone by `mid`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FloraFade {
    pub start: f32,
    pub mid: f32,
    pub end: f32,
}

impl FloraFade {
    // shrink start as a share of `mid`; tallest gone at a share of the way from `mid` to level 1's end
    const START: f32 = 0.55;
    const END: f32 = 0.6;
    // plant heights in blocks ranked shortest first; rank of the shortest that lasts past `mid`
    pub const HEIGHTS: (f32, f32) = (0.4, 1.3);
    pub const KEEP: f32 = 0.3;
    // where the shortest are gone, as a share of the way from `start` to `mid`
    pub const FIRST_GONE: f32 = 0.35;

    /// How far from the camera a plant `height` blocks tall has shrunk away to nothing.
    pub fn gone_at(self, height: f32) -> f32 {
        let rank = ((height - Self::HEIGHTS.0) / (Self::HEIGHTS.1 - Self::HEIGHTS.0)).clamp(0.0, 1.0);
        if rank < Self::KEEP {
            let first = self.start + (self.mid - self.start) * Self::FIRST_GONE;
            first + (self.mid - first) * rank / Self::KEEP
        } else {
            self.mid + (self.end - self.mid) * (rank - Self::KEEP) / (1.0 - Self::KEEP)
        }
    }
}
