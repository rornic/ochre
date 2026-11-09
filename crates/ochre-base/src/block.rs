#[derive(Debug, Default, PartialEq, Eq, Copy, Clone)]
#[repr(u8)]
pub enum BlockType {
    #[default]
    Air,
    Stone,
    Grass,
    Sand,
    Water,
    Snow,
    Log,
    Leaves,
    Dirt,
    Gravel,
    Ice,
    Sandstone,
    // ground flora, drawn as two crossed quads, see `BlockType::is_flora`
    TallGrass,
    Fern,
    Poppy,
    Dandelion,
    ShortGrass,
    SeedGrass,
    // bigger plants, up to two blocks across, see `mesh_flora`
    Shrub,
    Heath,
    // stone packed hard under the ground, which only explosives break
    CompactedStone,
    // the rocks of the geology, see `TerrainSource::rock`, each with a compacted form
    Limestone,
    Clay,
    Basalt,
    Marble,
    CompactedLimestone,
    CompactedClay,
    CompactedBasalt,
    CompactedMarble,
    CompactedSandstone,
    Granite,
    Shale,
    CompactedGranite,
    CompactedShale,
}

impl BlockType {
    pub fn occlusion_weight(&self) -> f32 {
        match self {
            BlockType::Air => 0.0,
            _ if self.is_flora() => 0.0,
            BlockType::Water => 0.4,
            BlockType::Leaves => 0.3,
            _ => 1.0,
        }
    }

    // How many levels of sky light this block takes from light passing through it: all of
    // it for opaque blocks, see `light.rs`.
    pub fn light_opacity(&self) -> u8 {
        match self {
            BlockType::Air => 0,
            _ if self.is_flora() => 0,
            BlockType::Leaves => 1,
            BlockType::Water => 2,
            BlockType::Ice => 3,
            _ => crate::MAX_LIGHT,
        }
    }

    // Whether the player is kept out of it: grass and flowers are walked through.
    pub fn is_solid(&self) -> bool {
        !matches!(self, BlockType::Air | BlockType::Water) && !self.is_flora()
    }

    // Whether the faces of blocks behind this one are hidden by it. Leaves have holes.
    pub fn is_opaque(&self) -> bool {
        self.light_opacity() == crate::MAX_LIGHT
    }

    // Whether the player can point at it: looking through air and water to what's past them.
    pub fn is_targetable(&self) -> bool {
        !matches!(self, BlockType::Air | BlockType::Water)
    }

    // Whether the player can break it. Compacted rock only gives way to explosives.
    pub fn is_breakable(&self) -> bool {
        self.is_targetable() && !self.is_compacted()
    }

    pub fn is_compacted(&self) -> bool {
        matches!(
            self,
            BlockType::CompactedStone
                | BlockType::CompactedLimestone
                | BlockType::CompactedClay
                | BlockType::CompactedBasalt
                | BlockType::CompactedMarble
                | BlockType::CompactedSandstone
                | BlockType::CompactedGranite
                | BlockType::CompactedShale
        )
    }

    // This rock packed hard, as it lies deep under the ground.
    pub fn compacted(&self) -> BlockType {
        match self {
            BlockType::Stone => BlockType::CompactedStone,
            BlockType::Limestone => BlockType::CompactedLimestone,
            BlockType::Clay => BlockType::CompactedClay,
            BlockType::Basalt => BlockType::CompactedBasalt,
            BlockType::Marble => BlockType::CompactedMarble,
            BlockType::Sandstone => BlockType::CompactedSandstone,
            BlockType::Granite => BlockType::CompactedGranite,
            BlockType::Shale => BlockType::CompactedShale,
            other => *other,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            BlockType::Air => "air",
            BlockType::Stone => "stone",
            BlockType::Grass => "grass",
            BlockType::Sand => "sand",
            BlockType::Water => "water",
            BlockType::Snow => "snow",
            BlockType::Log => "log",
            BlockType::Leaves => "leaves",
            BlockType::Dirt => "dirt",
            BlockType::Gravel => "gravel",
            BlockType::Ice => "ice",
            BlockType::Sandstone => "sandstone",
            BlockType::TallGrass => "tall grass",
            BlockType::Fern => "fern",
            BlockType::Poppy => "poppy",
            BlockType::Dandelion => "dandelion",
            BlockType::ShortGrass => "short grass",
            BlockType::SeedGrass => "seed grass",
            BlockType::Shrub => "shrub",
            BlockType::Heath => "heath",
            BlockType::CompactedStone => "compacted stone",
            BlockType::Limestone => "limestone",
            BlockType::Clay => "clay",
            BlockType::Basalt => "basalt",
            BlockType::Marble => "marble",
            BlockType::CompactedLimestone => "compacted limestone",
            BlockType::CompactedClay => "compacted clay",
            BlockType::CompactedBasalt => "compacted basalt",
            BlockType::CompactedMarble => "compacted marble",
            BlockType::CompactedSandstone => "compacted sandstone",
            BlockType::Granite => "granite",
            BlockType::Shale => "shale",
            BlockType::CompactedGranite => "compacted granite",
            BlockType::CompactedShale => "compacted shale",
        }
    }

    // Plants on the ground: drawn as two quads crossed through the block, not as a cube, and
    // see-through, hiding nothing and shading nothing.
    pub fn is_flora(&self) -> bool {
        matches!(
            self,
            BlockType::TallGrass
                | BlockType::Fern
                | BlockType::Poppy
                | BlockType::Dandelion
                | BlockType::ShortGrass
                | BlockType::SeedGrass
                | BlockType::Shrub
                | BlockType::Heath
        )
    }
}

#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub enum Face {
    Top,
    Side,
    Bottom,
}

impl Face {
    pub fn of_normal_y(y: f32) -> Self {
        if y > 0.5 {
            Face::Top
        } else if y < -0.5 {
            Face::Bottom
        } else {
            Face::Side
        }
    }
}

// The textures in blocks.png, left to right, in `generate.py`'s `TILES` order.
#[derive(Debug, PartialEq, Eq, Copy, Clone)]
#[repr(u8)]
pub enum Tile {
    Stone,
    GrassTop,
    Sand,
    Water,
    Snow,
    LogBark,
    Leaves,
    GrassSide,
    Dirt,
    LogEnd,
    Gravel,
    Ice,
    SandstoneSide,
    SandstoneTop,
    SnowSide,
    CompactedStone,
    Limestone,
    Clay,
    Basalt,
    Marble,
    CompactedLimestone,
    CompactedClay,
    CompactedBasalt,
    CompactedMarble,
    CompactedSandstone,
    Granite,
    Shale,
    CompactedGranite,
    CompactedShale,
    TallGrass,
    Fern,
    Poppy,
    Dandelion,
    ShortGrass,
    SeedGrass,
    Shrub,
    Heath,
}

pub const TILE_COUNT: usize = 37;

// The painted materials, in array layer order: each is `assets/textures/materials/<name>.png`.
pub const MATERIAL_FILES: [&str; 33] = [
    "stone",
    "dirt",
    "grass_top",
    "sand",
    "snow",
    "limestone",
    "clay",
    "basalt",
    "marble",
    "granite",
    "shale",
    "sandstone_top",
    "gravel",
    "ice",
    "log_bark",
    "log_end",
    "compacted_stone",
    "compacted_limestone",
    "compacted_clay",
    "compacted_basalt",
    "compacted_marble",
    "compacted_sandstone",
    "compacted_granite",
    "compacted_shale",
    // RGBA, drawn across a tile's own uv instead of by world position, see `Tile::foliage_layer`
    "leaves",
    "tall_grass",
    "fern",
    "poppy",
    "dandelion",
    "short_grass",
    "seed_grass",
    "shrub",
    "heath",
];
pub const MATERIAL_LAYERS: usize = MATERIAL_FILES.len();

impl Tile {
    // The painted material layer a face draws instead of its atlas tile, if it has one.
    pub fn material_layer(self) -> Option<u8> {
        match self {
            Tile::Stone => Some(0),
            Tile::Dirt => Some(1),
            // a grass block's sides are the same grass as its top
            Tile::GrassTop | Tile::GrassSide => Some(2),
            Tile::Sand => Some(3),
            // a snow block is snow on every face, so a snow side is the snow layer
            Tile::Snow | Tile::SnowSide => Some(4),
            Tile::Limestone => Some(5),
            Tile::Clay => Some(6),
            Tile::Basalt => Some(7),
            Tile::Marble => Some(8),
            Tile::Granite => Some(9),
            Tile::Shale => Some(10),
            // a sandstone block's sides are the same rock as its top
            Tile::SandstoneTop | Tile::SandstoneSide => Some(11),
            Tile::Gravel => Some(12),
            Tile::Ice => Some(13),
            Tile::LogBark => Some(14),
            Tile::LogEnd => Some(15),
            // each compacted rock is its rock's layer packed darker, see `materials.py`
            Tile::CompactedStone => Some(16),
            Tile::CompactedLimestone => Some(17),
            Tile::CompactedClay => Some(18),
            Tile::CompactedBasalt => Some(19),
            Tile::CompactedMarble => Some(20),
            Tile::CompactedSandstone => Some(21),
            Tile::CompactedGranite => Some(22),
            Tile::CompactedShale => Some(23),
            _ => None,
        }
    }

    // The RGBA layer a leaf or plant draws instead of its atlas tile, across the tile's own uv. Leaves repeat it every
    // block and a plant's sprite is drawn once across its quad, so neither needs the world-space projection that
    // `material_layer` is for, and the depth and shadow passes, which have no normals, cut them just as the main pass does.
    // The flora layers follow `FIRST_FLORA`'s order.
    pub fn foliage_layer(self) -> Option<u8> {
        match self {
            Tile::Leaves => Some(Self::LEAVES_LAYER),
            flora if flora as u8 >= Self::FIRST_FLORA as u8 => {
                Some(Self::FIRST_FLORA_LAYER + flora as u8 - Self::FIRST_FLORA as u8)
            }
            _ => None,
        }
    }

    pub const LEAVES_LAYER: u8 = 24;
    pub const FIRST_FLORA_LAYER: u8 = 25;

    // What meshes carry in vertex alpha for `block_texture.wesl`.
    pub fn atlas_cell(self) -> f32 {
        self as u8 as f32
    }

    // Greyscale tiles that vertex colours tint to the climate's grass.
    pub fn tinted(self) -> bool {
        matches!(
            self,
            Tile::GrassTop
                | Tile::GrassSide
                | Tile::Leaves
                | Tile::TallGrass
                | Tile::Fern
                | Tile::ShortGrass
                | Tile::SeedGrass
                | Tile::Shrub
                | Tile::Heath
        )
    }

    // The first of the flora sprites, which are the atlas's last tiles, see `BlockType::is_flora`.
    pub const FIRST_FLORA: Tile = Tile::TallGrass;
}

impl BlockType {
    // The tile a face of this block draws. Flora draws its one sprite on both its quads.
    pub fn tile(&self, face: Face) -> Tile {
        match (self, face) {
            (BlockType::Air, _) => Tile::Stone, // never drawn
            (BlockType::Stone, _) => Tile::Stone,
            (BlockType::CompactedStone, _) => Tile::CompactedStone,
            (BlockType::Limestone, _) => Tile::Limestone,
            (BlockType::Clay, _) => Tile::Clay,
            (BlockType::Basalt, _) => Tile::Basalt,
            (BlockType::Marble, _) => Tile::Marble,
            (BlockType::CompactedLimestone, _) => Tile::CompactedLimestone,
            (BlockType::CompactedClay, _) => Tile::CompactedClay,
            (BlockType::CompactedBasalt, _) => Tile::CompactedBasalt,
            (BlockType::CompactedMarble, _) => Tile::CompactedMarble,
            (BlockType::CompactedSandstone, _) => Tile::CompactedSandstone,
            (BlockType::Granite, _) => Tile::Granite,
            (BlockType::Shale, _) => Tile::Shale,
            (BlockType::CompactedGranite, _) => Tile::CompactedGranite,
            (BlockType::CompactedShale, _) => Tile::CompactedShale,
            (BlockType::Grass, Face::Top) => Tile::GrassTop,
            (BlockType::Grass, Face::Side) => Tile::GrassSide,
            (BlockType::Grass, Face::Bottom) => Tile::Dirt,
            (BlockType::Sand, _) => Tile::Sand,
            (BlockType::Water, _) => Tile::Water,
            (BlockType::Snow, _) => Tile::Snow,
            (BlockType::Log, Face::Side) => Tile::LogBark,
            (BlockType::Log, _) => Tile::LogEnd,
            (BlockType::Leaves, _) => Tile::Leaves,
            (BlockType::Dirt, _) => Tile::Dirt,
            (BlockType::Gravel, _) => Tile::Gravel,
            (BlockType::Ice, _) => Tile::Ice,
            (BlockType::Sandstone, Face::Side) => Tile::SandstoneSide,
            (BlockType::Sandstone, _) => Tile::SandstoneTop,
            (BlockType::TallGrass, _) => Tile::TallGrass,
            (BlockType::Fern, _) => Tile::Fern,
            (BlockType::Poppy, _) => Tile::Poppy,
            (BlockType::Dandelion, _) => Tile::Dandelion,
            (BlockType::ShortGrass, _) => Tile::ShortGrass,
            (BlockType::SeedGrass, _) => Tile::SeedGrass,
            (BlockType::Shrub, _) => Tile::Shrub,
            (BlockType::Heath, _) => Tile::Heath,
        }
    }

    // The one tile coarse LODs draw all of a block's faces with: grass stays green on hillsides.
    pub fn lod_tile(&self) -> Tile {
        match self {
            BlockType::Grass => Tile::GrassTop,
            _ => self.tile(Face::Side),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{BlockType, Face, Tile, MATERIAL_FILES, MATERIAL_LAYERS, TILE_COUNT};

    #[test]
    fn test_tile_count_matches_the_last_tile() {
        assert_eq!(TILE_COUNT, Tile::Heath as usize + 1);
    }

    // `GrassTop` as `grass_top`.
    fn file_name(tile: Tile) -> String {
        let mut name = String::new();
        for (i, c) in format!("{tile:?}").chars().enumerate() {
            if c.is_uppercase() && i > 0 {
                name.push('_');
            }
            name.extend(c.to_lowercase());
        }
        name
    }

    #[test]
    fn test_material_layers_follow_the_file_list() {
        for tile in [
            Tile::Stone,
            Tile::Dirt,
            Tile::GrassTop,
            Tile::Sand,
            Tile::Snow,
            Tile::Limestone,
            Tile::Clay,
            Tile::Basalt,
            Tile::Marble,
            Tile::Granite,
            Tile::Shale,
            Tile::SandstoneTop,
            Tile::Gravel,
            Tile::Ice,
            Tile::LogBark,
            Tile::LogEnd,
            Tile::CompactedStone,
            Tile::CompactedLimestone,
            Tile::CompactedClay,
            Tile::CompactedBasalt,
            Tile::CompactedMarble,
            Tile::CompactedSandstone,
            Tile::CompactedGranite,
            Tile::CompactedShale,
        ] {
            let layer = tile.material_layer().unwrap() as usize;
            assert_eq!(file_name(tile), MATERIAL_FILES[layer]);
        }
        assert_eq!(
            Tile::GrassTop.material_layer(),
            Tile::GrassSide.material_layer()
        );
        assert_eq!(
            Tile::SandstoneTop.material_layer(),
            Tile::SandstoneSide.material_layer()
        );
        assert_eq!(Tile::Snow.material_layer(), Tile::SnowSide.material_layer());
        assert!(Tile::Dirt.material_layer().unwrap() < MATERIAL_LAYERS as u8);
        for tile in [
            Tile::Leaves,
            Tile::Water,
            Tile::TallGrass,
            Tile::SeedGrass,
        ] {
            assert_eq!(None, tile.material_layer());
        }
    }

    #[test]
    fn test_foliage_layers_follow_the_file_list() {
        for tile in [
            Tile::Leaves,
            Tile::TallGrass,
            Tile::Fern,
            Tile::Poppy,
            Tile::Dandelion,
            Tile::ShortGrass,
            Tile::SeedGrass,
            Tile::Shrub,
            Tile::Heath,
        ] {
            let layer = tile.foliage_layer().unwrap() as usize;
            assert_eq!(file_name(tile), MATERIAL_FILES[layer]);
        }
        assert_eq!(None, Tile::Water.foliage_layer());
        assert_eq!(None, Tile::Stone.foliage_layer());
        // every layer is a world-space material or a foliage layer, so none is orphaned
        assert_eq!(
            MATERIAL_LAYERS,
            Tile::FIRST_FLORA_LAYER as usize + (Tile::Heath as usize - Tile::FIRST_FLORA as usize) + 1
        );
    }

    #[test]
    fn test_blocks_pick_their_tile_by_face() {
        assert_eq!(Tile::GrassTop, BlockType::Grass.tile(Face::Top));
        assert_eq!(Tile::GrassSide, BlockType::Grass.tile(Face::Side));
        assert_eq!(Tile::Dirt, BlockType::Grass.tile(Face::Bottom));
        assert_eq!(Tile::LogEnd, BlockType::Log.tile(Face::Bottom));
        assert_eq!(Tile::LogBark, BlockType::Log.tile(Face::Side));
        assert_eq!(Tile::GrassTop, BlockType::Grass.lod_tile());
        assert_eq!(Tile::LogBark, BlockType::Log.lod_tile());
    }

    #[test]
    fn test_snow_is_full_snow_on_every_face_not_snow_over_dirt() {
        for face in [Face::Top, Face::Side, Face::Bottom] {
            assert_eq!(Tile::Snow, BlockType::Snow.tile(face));
        }
        assert_eq!(Tile::Snow, BlockType::Snow.lod_tile());
    }

    #[test]
    fn test_ice_lets_light_through_but_is_solid() {
        assert!(!BlockType::Ice.is_opaque());
        assert!(BlockType::Ice.light_opacity() > BlockType::Water.light_opacity());
        assert_eq!(1.0, BlockType::Ice.occlusion_weight());
    }

    #[test]
    fn test_flora_hides_and_shades_nothing() {
        for flora in [
            BlockType::TallGrass,
            BlockType::Fern,
            BlockType::Poppy,
            BlockType::Dandelion,
            BlockType::ShortGrass,
            BlockType::SeedGrass,
            BlockType::Shrub,
            BlockType::Heath,
        ] {
            assert!(flora.is_flora() && !flora.is_opaque());
            assert_eq!(0, flora.light_opacity());
            assert_eq!(0.0, flora.occlusion_weight());
            assert!(flora.tile(Face::Side) as u8 >= Tile::FIRST_FLORA as u8);
        }
        assert!(!BlockType::Grass.is_flora() && !BlockType::Leaves.is_flora());
        // the shaders take every tile from the first flora's on as a plant's, see `foliage.wesl`
        assert_eq!(8, TILE_COUNT - Tile::FIRST_FLORA as usize);
    }

    #[test]
    fn test_occlusion_weights() {
        assert_eq!(0.0, BlockType::Air.occlusion_weight());
        assert_eq!(1.0, BlockType::Stone.occlusion_weight());
        let w = BlockType::Water.occlusion_weight();
        assert!(w > 0.0 && w < 1.0);
    }

    #[test]
    fn test_compacted_stone_air_and_water_cannot_be_broken() {
        assert!(BlockType::Stone.is_breakable());
        assert!(BlockType::Poppy.is_breakable());
        assert!(!BlockType::CompactedStone.is_breakable());
        // it can still be pointed at, to see what it is
        assert!(BlockType::CompactedStone.is_targetable());
        for passed_through in [BlockType::Air, BlockType::Water] {
            assert!(!passed_through.is_targetable() && !passed_through.is_breakable());
        }
    }

    #[test]
    fn test_every_rock_has_a_compacted_form_that_cannot_be_broken() {
        for rock in [
            BlockType::Stone,
            BlockType::Limestone,
            BlockType::Clay,
            BlockType::Basalt,
            BlockType::Marble,
            BlockType::Sandstone,
            BlockType::Granite,
            BlockType::Shale,
        ] {
            let packed = rock.compacted();
            assert!(rock.is_breakable() && !rock.is_compacted());
            assert!(packed.is_compacted() && !packed.is_breakable() && packed.is_targetable());
            assert_ne!(rock.tile(Face::Side), packed.tile(Face::Side));
        }
    }

    #[test]
    fn test_only_solid_blocks_are_opaque() {
        assert!(BlockType::Stone.is_opaque());
        assert!(BlockType::Log.is_opaque());
        assert!(!BlockType::Leaves.is_opaque());
        assert!(!BlockType::Water.is_opaque());
        assert!(!BlockType::Air.is_opaque());
    }
}
