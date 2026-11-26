use bevy::{
    asset::{Asset, RenderAssetUsages},
    ecs::system::SystemParam,
    image::{
        CompressedImageFormats, ImageAddressMode, ImageFilterMode, ImageSampler,
        ImageSamplerDescriptor, ImageType,
    },
    pbr::{MaterialPipeline, MaterialPipelineKey},
    prelude::*,
    reflect::TypePath,
    render::{
        mesh::MeshVertexBufferLayoutRef,
        render_resource::{
            AsBindGroup, Extent3d, Face, RenderPipelineDescriptor, SpecializedMeshPipelineError,
            TextureDimension, TextureFormat, TextureViewDescriptor, TextureViewDimension,
        },
        storage::ShaderBuffer,
    },
    shader::{ShaderDefVal, ShaderRef},
};

use crate::fade::FULLY_VISIBLE;
use crate::generate::generator::{INNER_LEAVES_BAND, INNER_LEAVES_REACH};
use crate::settings::DebugSettings;
use ochre_base::block::{game_glossy_slots, game_materials, Tile, MATERIAL_FILES, MATERIAL_LAYERS};
use ochre_base::WATER_LEVEL;

// Sea level, where `fog.wesl` turns from fog across the land to murk all round.
pub fn water_level() -> ShaderDefVal {
    ShaderDefVal::Int("WATER_LEVEL".into(), WATER_LEVEL as i32)
}

// Every stage needs every def its file's imports use, since each is built from the whole file.
pub fn push_shader_defs(descriptor: &mut RenderPipelineDescriptor, defs: &[ShaderDefVal]) {
    descriptor.vertex.shader_defs.extend_from_slice(defs);
    if let Some(fragment) = &mut descriptor.fragment {
        fragment.shader_defs.extend_from_slice(defs);
    }
}

/// Each painted material's square side in texels, see `material_array`.
pub(crate) const MATERIAL_SIZE: u32 = 512;

/// The painted materials as a 2D array: a layer per `MATERIAL_FILES` entry from `<dir>/<name>.png`, then one per game block slot to the last registered (`game_materials`), repeating, mipped.
pub(crate) fn material_array(dir: &str, anisotropy: u16) -> Image {
    let levels = MATERIAL_SIZE.ilog2() + 1;
    let mut data = Vec::new();
    // a slot with no block between registered ones draws the placeholder rock's layer, as it is never drawn
    let games = game_materials().into_iter().map(|m| m.unwrap_or("shale"));
    let names: Vec<&str> = MATERIAL_FILES.into_iter().chain(games).collect();
    for &name in &names {
        let path = format!("{dir}/{name}.png");
        let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("could not read {path}: {e}"));
        let mut layer = Image::from_buffer(
            &bytes,
            ImageType::Extension("png"),
            CompressedImageFormats::NONE,
            true,
            ImageSampler::Default,
            RenderAssetUsages::RENDER_WORLD,
        )
        .unwrap_or_else(|e| panic!("could not decode {path}: {e}"));
        assert_eq!(
            (layer.width(), layer.height()),
            (MATERIAL_SIZE, MATERIAL_SIZE),
            "{path} must be {MATERIAL_SIZE}x{MATERIAL_SIZE}"
        );
        mip_tiles(&mut layer, levels);
        data.extend_from_slice(layer.data.as_ref().unwrap());
    }
    let mut image = Image::default();
    image.texture_descriptor.size = Extent3d {
        width: MATERIAL_SIZE,
        height: MATERIAL_SIZE,
        depth_or_array_layers: names.len() as u32,
    };
    image.texture_descriptor.dimension = TextureDimension::D2;
    image.texture_descriptor.format = TextureFormat::Rgba8UnormSrgb;
    image.texture_descriptor.mip_level_count = levels;
    image.texture_view_descriptor = Some(TextureViewDescriptor {
        dimension: Some(TextureViewDimension::D2Array),
        ..default()
    });
    image.data = Some(data);
    image.asset_usage = RenderAssetUsages::RENDER_WORLD;
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        anisotropy_clamp: anisotropy.clamp(1, 16),
        ..default()
    });
    image
}

// Appends `levels` - 1 box-filtered mips, averaged in linear light and alpha-weighted; uncovered texels average plain.
fn mip_tiles(image: &mut Image, levels: u32) {
    let (mut width, mut height) = (image.width() as usize, image.height() as usize);
    let data = image.data.get_or_insert_with(Vec::new);
    let mut level = data.clone();
    for _ in 1..levels {
        let (next_width, next_height) = (width / 2, height / 2);
        let mut next = Vec::with_capacity(next_width * next_height * 4);
        for y in 0..next_height {
            for x in 0..next_width {
                let (mut rgb, mut alpha, mut plain) = ([0.0f32; 3], 0.0f32, [0.0f32; 3]);
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let at = ((2 * y + dy) * width + 2 * x + dx) * 4;
                    let texel = &level[at..at + 4];
                    let linear = LinearRgba::from(Srgba::rgb_u8(texel[0], texel[1], texel[2]));
                    let a = texel[3] as f32 / 255.0;
                    rgb[0] += linear.red * a;
                    rgb[1] += linear.green * a;
                    rgb[2] += linear.blue * a;
                    alpha += a;
                    plain = [
                        plain[0] + linear.red,
                        plain[1] + linear.green,
                        plain[2] + linear.blue,
                    ];
                }
                let covered = alpha > 1e-3;
                let (sum, weight) = if covered { (rgb, alpha) } else { (plain, 4.0) };
                let colour = Srgba::from(LinearRgba::rgb(
                    sum[0] / weight,
                    sum[1] / weight,
                    sum[2] / weight,
                ))
                .to_u8_array();
                next.extend_from_slice(&[
                    colour[0],
                    colour[1],
                    colour[2],
                    (alpha / 4.0 * 255.0).round() as u8,
                ]);
            }
        }
        data.extend_from_slice(&next);
        level = next;
        (width, height) = (next_width, next_height);
    }
    image.texture_descriptor.mip_level_count = levels;
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
#[bind_group_data(ChunkMaterialKey)]
pub struct ChunkMaterial {
    #[uniform(0)]
    pub color: LinearRgba,
    // the dither window drawn, see `fade::FadeMaterials`
    #[uniform(3)]
    pub fade: Vec4,
    // the clouds, to shade the land under them, see `CloudLayer`
    #[texture(4)]
    #[sampler(5)]
    pub cloud_pattern: Handle<Image>,
    #[uniform(6)]
    pub cloud_layer: Vec4,
    // what the land bends around, see `curvature::Curvature`
    #[storage(8, read_only)]
    pub curvature: Handle<ShaderBuffer>,
    // the weather round the camera, thickening the clouds whose shadows fall here, see `CloudLayer::weather`
    #[storage(11, read_only)]
    pub cloud_weather: Handle<ShaderBuffer>,
    // the painted materials, see `material_array`
    #[texture(12, dimension = "2d_array")]
    #[sampler(13)]
    pub materials: Option<Handle<Image>>,
    // the prevailing wind's direction in x and z that gusts across leaves and flora travel along, zero until it is set
    #[uniform(14)]
    pub wind: Vec4,
    // compiled into the shaders, see `specialize`
    pub debug: DebugSettings,
    /// No leaf or flora holes. Camera passes still alpha-test LOD fades.
    pub non_cutout: bool,
    /// Blocks before a painted material repeats, compiled into the shaders.
    pub period: u32,
    /// `FloraFade`'s start, mid and end, in blocks, compiled into the shaders, see `FloraFade`.
    pub flora_fade: [u32; 3],
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub struct ChunkMaterialKey {
    pub debug: DebugSettings,
    pub non_cutout: bool,
    pub period: u32,
    pub flora_fade: [u32; 3],
    /// Solid and wholly drawn, so it discards nothing, see `ChunkMaterial::opaque`.
    pub opaque: bool,
}

impl From<&ChunkMaterial> for ChunkMaterialKey {
    fn from(material: &ChunkMaterial) -> Self {
        Self {
            debug: material.debug,
            non_cutout: material.non_cutout,
            period: material.period,
            flora_fade: material.flora_fade,
            opaque: material.opaque(),
        }
    }
}

impl ChunkMaterial {
    /// Whether faces draw wholly solid (no leaf holes or fade) so it can be opaque: a discarding shader loses early Z.
    pub fn opaque(&self) -> bool {
        self.non_cutout && self.fade == crate::fade::FULLY_VISIBLE
    }
}

impl Material for ChunkMaterial {
    // sways leaves in the wind
    fn vertex_shader() -> ShaderRef {
        "shaders/world.wesl".into()
    }

    fn fragment_shader() -> ShaderRef {
        "shaders/world.wesl".into()
    }

    // sways leaves just as the main pass does, so they keep to their shadows
    fn prepass_vertex_shader() -> ShaderRef {
        "shaders/world_prepass.wesl".into()
    }

    // cuts the holes in leaves out of their shadows too
    fn prepass_fragment_shader() -> ShaderRef {
        "shaders/world_prepass.wesl".into()
    }

    // Camera fades discard pixels even when leaves have no holes; solid faces drawn wholly discard nothing.
    fn alpha_mode(&self) -> AlphaMode {
        if self.opaque() {
            AlphaMode::Opaque
        } else {
            AlphaMode::Mask(0.5)
        }
    }

    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.primitive.cull_mode = Some(Face::Front);
        // the main and shadow passes both sway leaves, see `foliage.wesl`
        let leaf_cell = ShaderDefVal::UInt("LEAF_CELL".into(), Tile::Leaves.atlas_cell() as u32);
        // steep vegetated ground bares to dirt, see `world.wesl`
        let dirt_cell = ShaderDefVal::UInt("DIRT_CELL".into(), Tile::Dirt.atlas_cell() as u32);
        let grass_layer = ShaderDefVal::UInt("GRASS_LAYER".into(), Tile::GrassTop.material_layer().unwrap() as u32);
        let steppe_layer = ShaderDefVal::UInt("STEPPE_LAYER".into(), Tile::SteppeTop.material_layer().unwrap() as u32);
        // flora's sprites are cut out and sway at their tips, see `foliage.wesl`
        let first_flora_cell = ShaderDefVal::UInt(
            "FIRST_FLORA_CELL".into(),
            Tile::FIRST_FLORA.atlas_cell() as u32,
        );
        // shrubs and heath are broad solid sprites; the blur that suits thin grass would draw them as slabs
        let first_big_flora_cell = ShaderDefVal::UInt(
            "FIRST_BIG_FLORA_CELL".into(),
            Tile::Shrub.atlas_cell() as u32,
        );
        // a game's blocks come after the flora and draw their own layers past the engine's, see `Tile::material_layer`
        let first_game_cell = ShaderDefVal::UInt("FIRST_GAME_CELL".into(), Tile::FIRST_GAME.atlas_cell() as u32);
        let first_game_layer = ShaderDefVal::UInt("FIRST_GAME_LAYER".into(), MATERIAL_LAYERS as u32);
        // the game blocks that glint and mirror the sky as ice does, see `ice.wesl`
        let glossy_game_slots = ShaderDefVal::UInt("GLOSSY_GAME_SLOTS".into(), game_glossy_slots());
        // leaves' holes fill in with the shade of the leaves behind them past this reach
        let inner_leaves_reach =
            ShaderDefVal::UInt("INNER_LEAVES_REACH".into(), INNER_LEAVES_REACH as u32);
        let inner_leaves_band =
            ShaderDefVal::UInt("INNER_LEAVES_BAND".into(), INNER_LEAVES_BAND as u32);
        // the RGBA layers leaves and plants draw, see `Tile::foliage_layer`
        let leaves_layer = ShaderDefVal::UInt("LEAVES_LAYER".into(), Tile::LEAVES_LAYER as u32);
        let first_flora_layer =
            ShaderDefVal::UInt("FIRST_FLORA_LAYER".into(), Tile::FIRST_FLORA_LAYER as u32);
        // frost skips what is already snow or ice, see `frost.wesl`
        let snow_cell = ShaderDefVal::UInt("SNOW_CELL".into(), Tile::Snow.atlas_cell() as u32);
        let ice_cell = ShaderDefVal::UInt("ICE_CELL".into(), Tile::Ice.atlas_cell() as u32);
        let mut defs = vec![
            snow_cell,
            ice_cell,
            leaves_layer,
            first_flora_layer,
            leaf_cell,
            dirt_cell,
            grass_layer,
            steppe_layer,
            inner_leaves_reach,
            inner_leaves_band,
            first_flora_cell,
            first_big_flora_cell,
            first_game_cell,
            first_game_layer,
            glossy_game_slots,
            water_level(),
            ShaderDefVal::UInt("MATERIAL_PERIOD".into(), key.bind_group_data.period),
            ShaderDefVal::UInt("FLORA_FADE_START".into(), key.bind_group_data.flora_fade[0]),
            ShaderDefVal::UInt("FLORA_FADE_MID".into(), key.bind_group_data.flora_fade[1]),
            ShaderDefVal::UInt("FLORA_FADE_END".into(), key.bind_group_data.flora_fade[2]),
        ];
        // compiled in or out, so the switches leave no trace of the code they turn off
        if key.bind_group_data.debug.still_leaves {
            defs.push("STILL_LEAVES".into());
        }
        if key.bind_group_data.debug.solid_leaves {
            defs.push("SOLID_LEAVES".into());
        }
        if key.bind_group_data.debug.no_frost {
            defs.push("NO_FROST".into());
        }
        // no discard at all, so the GPU keeps its early depth test, see `ChunkMaterial::opaque`
        if key.bind_group_data.opaque {
            defs.push("OPAQUE_SOLID".into());
        }
        push_shader_defs(descriptor, &defs);
        specialize_shadow(descriptor, key.bind_group_data.non_cutout);
        Ok(())
    }
}

fn specialize_shadow(descriptor: &mut RenderPipelineDescriptor, non_cutout: bool) {
    if !non_cutout {
        return;
    }
    push_shader_defs(descriptor, &["NON_CUTOUT".into()]);
    let defs = &descriptor.vertex.shader_defs;
    // Emulated depth clamping writes frag_depth and therefore still needs a fragment stage.
    if defs.contains(&"VIEW_PROJECTION_ORTHOGRAPHIC".into())
        && !defs.contains(&"UNCLIPPED_DEPTH_ORTHO_EMULATION".into())
    {
        descriptor.fragment = None;
    }
}

/// The materials terrain draws with, before any fading.
#[derive(Clone, Default, Resource)]
pub struct TerrainMaterials {
    pub chunk: Handle<ChunkMaterial>,
    pub solid_chunk: Handle<ChunkMaterial>,
    pub water: Handle<WaterMaterial>,
    pub solid_water: Handle<WaterMaterial>,
    pub spray: Handle<SprayMaterial>,
}

impl TerrainMaterials {
    pub(crate) fn new(
        debug: DebugSettings,
        period: u32,
        flora_fade: crate::lod::FloraFade,
        inputs: &ShaderInputs,
        assets: &mut TerrainAssets,
    ) -> Self {
        let clouds = &inputs.clouds;
        let chunk = assets.chunk.add(ChunkMaterial {
            color: LinearRgba::WHITE,
            fade: FULLY_VISIBLE,
            cloud_pattern: clouds.pattern.clone(),
            cloud_layer: clouds.layer,
            curvature: inputs.curvature.clone(),
            cloud_weather: clouds.weather.clone(),
            materials: inputs.materials.clone(),
            wind: Vec4::new(inputs.wind.x, inputs.wind.y, 0.0, 0.0),
            debug,
            non_cutout: false,
            period,
            flora_fade: [flora_fade.start, flora_fade.mid, flora_fade.end].map(|d| d as u32),
        });
        let base_chunk = assets.chunk.get(&chunk).unwrap().clone();
        let solid_chunk = assets.chunk.add(ChunkMaterial {
            non_cutout: true,
            ..base_chunk
        });
        let water = WaterMaterial {
            color: LinearRgba::rgb(0.0, 0.85, 0.85),
            fade: FULLY_VISIBLE,
            cloud_pattern: clouds.pattern.clone(),
            cloud_layer: clouds.layer,
            cloud_reflection: clouds.reflection.clone(),
            curvature: inputs.curvature.clone(),
            cloud_weather: clouds.weather.clone(),
            solid: false,
        };
        Self {
            chunk,
            solid_chunk,
            solid_water: assets.water.add(WaterMaterial {
                solid: true,
                ..water.clone()
            }),
            water: assets.water.add(water),
            spray: assets.spray.add(SprayMaterial {
                curvature: inputs.curvature.clone(),
            }),
        }
    }
}

/// The clouds' textures, shared by the cloud, water and land shaders, see `cloud_bindings.wesl`.
#[derive(Resource, Clone, Default)]
pub struct CloudLayer {
    /// How much cloud is over each cell, 0 where clear to 1 where thickest.
    pub pattern: Handle<Image>,
    /// The middle of the layer's height, the pattern's width in blocks, and the wind.
    pub layer: Vec4,
    /// The clouds as seen, unlit, at a fraction of the screen's resolution, see `CloudMarch`.
    pub target: Handle<Image>,
    /// The clouds over the whole sky, unlit, for water to reflect, see `sky_disc` in `cloud_march.wesl`.
    pub reflection: Handle<Image>,
    /// How cloudy, stormy and wet each cell of a grid round the camera is, thickening clouds into storms (`weather_at`).
    pub weather: Handle<ShaderBuffer>,
}

impl CloudLayer {
    /// A layer with no clouds, for the shaders to sample where nothing draws any.
    pub fn clear(images: &mut Assets<Image>, buffers: &mut Assets<ShaderBuffer>) -> Self {
        let black = Image::new_fill(
            Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            &[0; 4],
            TextureFormat::Rgba8Unorm,
            RenderAssetUsages::RENDER_WORLD,
        );
        Self {
            pattern: images.add(black.clone()),
            layer: Vec4::new(1000.0, 1000.0, 0.0, 0.0),
            target: images.add(black.clone()),
            reflection: images.add(black),
            weather: buffers.add(ShaderBuffer::new(vec![0.0f32; 8], RenderAssetUsages::default())),
        }
    }
}

/// What the terrain's shaders sample that other systems own. The default is blank, for a headless run.
#[derive(Clone, Default)]
pub(crate) struct ShaderInputs {
    /// The painted materials, see `material_array`.
    pub materials: Option<Handle<Image>>,
    pub clouds: CloudLayer,
    pub curvature: Handle<ShaderBuffer>,
    /// Prevailing wind direction in x and z, any length: the way gusts cross the land. Zero keeps the old fixed one.
    pub wind: Vec2,
}

#[derive(SystemParam)]
pub(crate) struct TerrainAssets<'w> {
    pub chunk: ResMut<'w, Assets<ChunkMaterial>>,
    pub water: ResMut<'w, Assets<WaterMaterial>>,
    pub spray: ResMut<'w, Assets<SprayMaterial>>,
}

/// White water thrown up where fresh water runs steeply or falls, as particles the shader moves.
#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct SprayMaterial {
    // what the land bends around, see `curvature::Curvature`
    #[storage(8, read_only)]
    pub curvature: Handle<ShaderBuffer>,
}

impl Material for SprayMaterial {
    fn enable_shadows() -> bool {
        false
    }

    fn enable_prepass() -> bool {
        false
    }

    fn vertex_shader() -> ShaderRef {
        "shaders/spray.wesl".into()
    }

    fn fragment_shader() -> ShaderRef {
        "shaders/spray.wesl".into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Blend
    }

    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        // each particle is turned to face the camera in the shader, whichever way it was wound
        descriptor.primitive.cull_mode = None;
        Ok(())
    }
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct WaterMaterial {
    #[uniform(0)]
    pub color: LinearRgba,
    // the dither window drawn, see `fade::FadeMaterials`
    #[uniform(3)]
    pub fade: Vec4,
    // the clouds, for water to reflect and shade its glint by, see `CloudLayer`
    #[texture(4)]
    #[sampler(5)]
    pub cloud_pattern: Handle<Image>,
    #[uniform(6)]
    pub cloud_layer: Vec4,
    // what the land bends around, see `curvature::Curvature`
    #[storage(8, read_only)]
    pub curvature: Handle<ShaderBuffer>,
    // the weather round the camera, see `CloudLayer::weather`
    #[storage(11, read_only)]
    pub cloud_weather: Handle<ShaderBuffer>,
    // the clouds over the sky, to reflect, see `CloudLayer::reflection`
    #[texture(9)]
    #[sampler(10)]
    pub cloud_reflection: Handle<Image>,
    // drawn with the opaque meshes, which Bevy batches, see `lod::SOLID_WATER_LEVEL`
    pub solid: bool,
}

impl Material for WaterMaterial {
    // light passes through water, so it casts no shadow
    fn enable_shadows() -> bool {
        false
    }

    // its waves would put a prepass's depth off the main pass's
    fn enable_prepass() -> bool {
        false
    }

    fn vertex_shader() -> ShaderRef {
        "shaders/water.wesl".into()
    }

    fn fragment_shader() -> ShaderRef {
        "shaders/water.wesl".into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        if self.solid {
            AlphaMode::Opaque
        } else {
            AlphaMode::Blend
        }
    }

    // fresh water reads a copy of the opaque scene for ripples and reflection, so it draws after it
    fn reads_view_transmission_texture(&self) -> bool {
        !self.solid
    }

    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        // unlike ChunkMaterial, water isn't back-face culled: a swimmer under the lowered surface must still see it
        descriptor.primitive.cull_mode = None;
        push_shader_defs(descriptor, &[water_level()]);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use bevy::color::{LinearRgba, Srgba};
    use ochre_base::block::{Tile, MATERIAL_FILES, MATERIAL_LAYERS};

    // An uncovered mip texel keeps its painted colour so smooth lookups near a sprite's edge don't blend black (dark rim)
    #[test]
    fn test_a_clear_mip_texel_keeps_the_colour_painted_there() {
        use bevy::{
            asset::RenderAssetUsages,
            image::Image,
            render::render_resource::{Extent3d, TextureDimension, TextureFormat},
        };
        // 4 by 4: the left half drawn at 200, the right half clear but painted 150
        let data: Vec<u8> = (0..16)
            .flat_map(|i| if i % 4 < 2 { [200, 200, 200, 255] } else { [150, 150, 150, 0] })
            .collect();
        let mut image = Image::new(
            Extent3d { width: 4, height: 4, depth_or_array_layers: 1 },
            TextureDimension::D2,
            data,
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::RENDER_WORLD,
        );
        super::mip_tiles(&mut image, 2);
        let level_one = &image.data.as_ref().unwrap()[64..];
        // the drawn texel is as it was, and the clear one keeps its colour, with no cover
        assert_eq!([200, 200, 200, 255], level_one[0..4]);
        assert_eq!([150, 150, 150, 0], level_one[4..8]);
    }

    #[test]
    fn only_solid_wholly_drawn_terrain_is_opaque() {
        use bevy::prelude::{AlphaMode, Material};
        let material = |non_cutout, fade| super::ChunkMaterial {
            color: Default::default(),
            fade,
            cloud_pattern: Default::default(),
            cloud_layer: Default::default(),
            curvature: Default::default(),
            cloud_weather: Default::default(),
            materials: None,
            wind: Default::default(),
            debug: Default::default(),
            non_cutout,
            period: 4,
            flora_fade: [0; 3],
        };
        let full = crate::fade::FULLY_VISIBLE;
        let fading = bevy::math::Vec4::new(0.0, 0.5, 0.0, 0.0);
        assert_eq!(AlphaMode::Opaque, material(true, full).alpha_mode());
        assert!(super::ChunkMaterialKey::from(&material(true, full)).opaque);
        // leaves cut holes, and a fade dithers, so both still discard
        for (non_cutout, fade) in [(false, full), (true, fading), (false, fading)] {
            assert_eq!(AlphaMode::Mask(0.5), material(non_cutout, fade).alpha_mode());
            assert!(!super::ChunkMaterialKey::from(&material(non_cutout, fade)).opaque);
        }
    }

    #[test]
    fn the_opaque_shaders_compile_every_discard_out() {
        // the GPU drops its early depth test for a shader that may discard, so none may be left in
        for (path, open) in [("shaders/world.wesl", "@if(!OPAQUE_SOLID) {"), ("shaders/world_prepass.wesl", "@if(!VIEW_PROJECTION_ORTHOGRAPHIC && !OPAQUE_SOLID) {")] {
            let source = std::fs::read_to_string(asset(path)).unwrap();
            let start = source.find(open).unwrap_or_else(|| panic!("no {open} in {path}"));
            let mut depth = 0;
            let end = start
                + source[start..]
                    .char_indices()
                    .find(|&(_, c)| {
                        depth += (c == '{') as i32 - (c == '}') as i32;
                        c == '}' && depth == 0
                    })
                    .unwrap()
                    .0;
            let inside = source[start..end].matches("discard;").count();
            assert!(inside > 0, "{path}");
            if path.ends_with("world.wesl") {
                assert_eq!(source.matches("discard;").count(), inside, "a discard outside the opaque guard in {path}");
            }
        }
    }

    #[test]
    fn fragment_stage_is_removed_only_for_solid_native_directional_shadows() {
        use bevy::render::render_resource::{FragmentState, RenderPipelineDescriptor};
        for non_cutout in [false, true] {
            for shadow in [false, true] {
                for emulated in [false, true] {
                    let mut descriptor = RenderPipelineDescriptor {
                        fragment: Some(FragmentState::default()),
                        ..Default::default()
                    };
                    if shadow {
                        descriptor
                            .vertex
                            .shader_defs
                            .push("VIEW_PROJECTION_ORTHOGRAPHIC".into());
                    }
                    if emulated {
                        descriptor
                            .vertex
                            .shader_defs
                            .push("UNCLIPPED_DEPTH_ORTHO_EMULATION".into());
                    }
                    super::specialize_shadow(&mut descriptor, non_cutout);
                    assert_eq!(
                        descriptor.fragment.is_none(),
                        non_cutout && shadow && !emulated
                    );
                }
            }
        }
    }

    // the workspace's assets, wherever the tests run from
    fn asset(path: &str) -> String {
        format!("{}/../../assets/{path}", env!("CARGO_MANIFEST_DIR"))
    }

    #[test]
    fn test_material_array_has_a_layer_with_mips_per_file() {
        let array = super::material_array(&asset("textures/materials"), 8);
        let size = super::MATERIAL_SIZE as usize;
        let levels = size.ilog2() as usize + 1;
        assert_eq!(levels as u32, array.texture_descriptor.mip_level_count);
        assert_eq!(
            MATERIAL_LAYERS as u32,
            array.texture_descriptor.size.depth_or_array_layers
        );
        let texels: usize = (0..levels).map(|l| (size >> l) * (size >> l)).sum();
        assert_eq!(
            texels * 4 * MATERIAL_LAYERS,
            array.data.as_ref().unwrap().len()
        );
    }

    // `MATERIAL_LAYER_TABLE`'s entries in block_texture.wesl, in atlas order
    fn shader_layers() -> Vec<i32> {
        let shader = std::fs::read_to_string(asset("shaders/block_texture.wesl")).unwrap();
        let table = shader.split("MATERIAL_LAYER_TABLE: ").nth(1).unwrap();
        table
            .lines()
            .skip(1)
            .take_while(|line| !line.starts_with(')'))
            .map(|line| line.split(',').next().unwrap().trim().parse().unwrap())
            .collect()
    }

    #[test]
    fn test_the_foliage_shader_shrinks_flora_as_flora_fade_says() {
        use crate::lod::FloraFade;
        let shader = std::fs::read_to_string(asset("shaders/foliage.wesl")).unwrap();
        let constant = |name: &str| shader.split(&format!("const {name}: ")).nth(1).unwrap().lines().next().unwrap().to_string();
        let heights = FloraFade::HEIGHTS;
        assert_eq!(format!("vec2<f32> = vec2<f32>({:?}, {:?});", heights.0, heights.1), constant("FLORA_HEIGHTS"));
        assert_eq!(format!("f32 = {:?};", FloraFade::KEEP), constant("FLORA_KEEP"));
        assert_eq!(format!("f32 = {:?};", FloraFade::FIRST_GONE), constant("FLORA_FIRST_GONE"));
        use crate::generate::generator::{FLORA_HEIGHT_CODE, FLORA_HEIGHT_SPAN};
        assert_eq!(format!("f32 = {FLORA_HEIGHT_SPAN:?};"), constant("FLORA_HEIGHT_SPAN"));
        assert_eq!(format!("f32 = {FLORA_HEIGHT_CODE:?};"), constant("FLORA_HEIGHT_CODE"));
    }

    #[test]
    fn test_the_shader_cools_the_neutral_rocks() {
        let shader = std::fs::read_to_string(asset("shaders/world.wesl")).unwrap();
        let line = shader.split("fn is_neutral_rock").nth(1).unwrap();
        let line = line.lines().nth(1).unwrap();
        let mut in_shader: Vec<usize> = line
            .split("layer == ")
            .skip(1)
            .map(|n| n.split(|c: char| !c.is_ascii_digit()).next().unwrap().parse().unwrap())
            .collect();
        in_shader.sort();
        let mut expected: Vec<usize> = MATERIAL_FILES
            .iter()
            .enumerate()
            .filter(|(_, name)| {
                ["stone", "basalt", "granite", "shale"]
                    .iter()
                    .any(|rock| *name == rock || *name == &format!("compacted_{rock}"))
            })
            .map(|(layer, _)| layer)
            .collect();
        expected.sort();
        assert_eq!(expected, in_shader);
    }

    #[test]
    fn test_shader_layer_table_matches_the_tiles() {
        let layers = shader_layers();
        assert_eq!(ochre_base::block::TILE_COUNT, layers.len());
        for (cell, layer) in layers.iter().enumerate() {
            // SAFETY: `Tile` is `repr(u8)` with a variant for every cell below `TILE_COUNT`
            let tile: Tile = unsafe { std::mem::transmute(cell as u8) };
            let expected = tile.material_layer().map_or(-1, i32::from);
            assert_eq!(expected, *layer, "{tile:?}");
        }
    }

    // `block_mean`'s colours in block_texture.wesl, in atlas order
    fn shader_means() -> Vec<[f32; 3]> {
        let shader = std::fs::read_to_string(asset("shaders/block_texture.wesl")).unwrap();
        shader
            .lines()
            .filter_map(|line| line.trim().strip_prefix("vec3<f32>(")?.split(')').next())
            .map(|args| {
                let v: Vec<f32> = args.split(',').map(|a| a.trim().parse().unwrap()).collect();
                [v[0], v[1], v[2]]
            })
            .collect()
    }

    // `GrassTop` as `grass_top`, the name of its file.
    fn snake_case(name: &str) -> String {
        let mut out = String::new();
        for (i, c) in name.chars().enumerate() {
            if c.is_uppercase() && i > 0 {
                out.push('_');
            }
            out.extend(c.to_lowercase());
        }
        out
    }

    // The mean linear colour of the texels an image draws, those the shaders don't cut out.
    fn image_mean(image: &image::RgbaImage, x0: u32, tile: u32) -> [f32; 3] {
        let drawn: Vec<LinearRgba> = (0..tile)
            .flat_map(|y| (0..tile).map(move |x| (x, y)))
            .map(|(x, y)| image.get_pixel(x0 + x, y).0)
            .filter(|p| p[3] >= 128)
            .map(|p| Srgba::rgb_u8(p[0], p[1], p[2]).into())
            .collect();
        let mean = |channel: fn(&LinearRgba) -> f32| {
            drawn.iter().map(channel).sum::<f32>() / drawn.len() as f32
        };
        [mean(|c| c.red), mean(|c| c.green), mean(|c| c.blue)]
    }

    // What each cell's own material averages to, `None` for water and the side variants, which have none.
    fn material_means() -> Vec<Option<[f32; 3]>> {
        (0..ochre_base::block::TILE_COUNT)
            .map(|cell| {
                // SAFETY: as in `test_shader_layer_table_matches_the_tiles`
                let tile_of: Tile = unsafe { std::mem::transmute(cell as u8) };
                // grass sides draw the grass layer but keep their own row, and the placeholder rocks have none and draw shale's
                let placeholder = matches!(tile_of, Tile::Stone | Tile::CompactedStone);
                tile_of
                    .material_layer()
                    .or(tile_of.foliage_layer())
                    .filter(|l| {
                        placeholder
                            || snake_case(&format!("{tile_of:?}")) == MATERIAL_FILES[*l as usize]
                    })
                    .map(|layer| {
                        let name = MATERIAL_FILES[layer as usize];
                        let material =
                            image::open(asset(&format!("textures/materials/{name}.png")))
                                .unwrap()
                                .to_rgba8();
                        image_mean(&material, 0, material.height())
                    })
            })
            .collect()
    }

    fn foliage(tile: Tile) -> image::RgbaImage {
        let name = MATERIAL_FILES[tile.foliage_layer().unwrap() as usize];
        image::open(asset(&format!("textures/materials/{name}.png")))
            .unwrap()
            .to_rgba8()
    }

    fn coverage(image: &image::RgbaImage, region: impl Fn(u32, u32) -> bool) -> f32 {
        let (mut seen, mut drawn) = (0, 0);
        for (x, y, p) in image.enumerate_pixels() {
            if region(x, y) {
                seen += 1;
                drawn += (p[3] >= 128) as u32;
            }
        }
        drawn as f32 / seen as f32
    }

    #[test]
    fn test_leaves_have_holes() {
        let share = coverage(&foliage(Tile::Leaves), |_, _| true);
        assert!((0.6..0.85).contains(&share), "leaves are {share} opaque");
    }

    #[test]
    fn test_sprites_stand_on_the_ground_and_stay_off_the_other_edges() {
        for tile in [
            Tile::TallGrass,
            Tile::Fern,
            Tile::Poppy,
            Tile::Dandelion,
            Tile::ShortGrass,
            Tile::SeedGrass,
            Tile::FeatherGrass,
            Tile::SteppeFlower,
        ] {
            let sprite = foliage(tile);
            let (w, h) = (sprite.width(), sprite.height());
            let share = coverage(&sprite, |_, _| true);
            assert!((0.04..0.5).contains(&share), "{tile:?} is {share} opaque");
            // a sprite is clamped, not repeated: its sides and top are clear, so no edge bleeds in
            assert_eq!(
                0.0,
                coverage(&sprite, |x, y| y == 0 || x == 0 || x == w - 1),
                "{tile:?} touches its top or sides"
            );
            assert!(
                coverage(&sprite, |_, y| y == h - 1) > 0.02,
                "{tile:?} doesn't stand on the ground"
            );
        }
    }

    // The shader cuts shrubs and heath at mip 5, coverage 0.3 at most (`block_alpha_at`); a blurrier cut draws solid.
    #[test]
    fn test_big_plants_keep_their_silhouette_at_the_blur_they_are_cut_at() {
        for tile in [Tile::Shrub, Tile::Heath] {
            let layer = foliage(tile);
            let size = layer.width() as usize;
            let alpha: Vec<f32> = layer.pixels().map(|p| p[3] as f32 / 255.0).collect();
            let truth = alpha.iter().filter(|&&a| a >= 0.5).count() as f32 / alpha.len() as f32;
            let texel = 1usize << 5;
            let (mut passed, mut texels) = (0, 0);
            for ty in 0..size / texel {
                for tx in 0..size / texel {
                    let sum: f32 = (0..texel)
                        .flat_map(|y| (0..texel).map(move |x| (x, y)))
                        .map(|(x, y)| alpha[(ty * texel + y) * size + tx * texel + x])
                        .sum();
                    let mean = sum / (texel * texel) as f32;
                    passed += (mean * 0.5 / 0.3 >= 0.5) as usize;
                    texels += 1;
                }
            }
            let drawn = passed as f32 / texels as f32;
            assert!(drawn < truth + 0.12, "{tile:?} is drawn {drawn:.2} solid at its blurriest, against {truth:.2} painted");
        }
    }

    // Replays `block_alpha_at` on a layer at mip `level`: the share of the sprite drawn, after the lowered cut.
    fn drawn_at(tile: Tile, level: u32) -> f32 {
        let layer = foliage(tile);
        let size = layer.width() as usize;
        let texel = 1usize << level;
        let cut = 0.5 + (0.1 - 0.5) * (level as f32 / 5.0).min(1.0);
        let alpha: Vec<f32> = layer.pixels().map(|p| p[3] as f32 / 255.0).collect();
        let (mut passed, mut texels) = (0, 0);
        for ty in 0..size / texel {
            for tx in 0..size / texel {
                let sum: f32 = (0..texel)
                    .flat_map(|y| (0..texel).map(move |x| (x, y)))
                    .map(|(x, y)| alpha[(ty * texel + y) * size + tx * texel + x])
                    .sum();
                passed += ((sum / (texel * texel) as f32) * 0.5 / cut >= 0.5) as usize;
                texels += 1;
            }
        }
        passed as f32 / texels as f32
    }

    // Strands under ~12 texels wide blob together at distance in the shader: hair-thin ones made feather grass lumpy.
    #[test]
    fn test_thin_plants_stay_apart_at_the_blur_they_are_cut_at() {
        for tile in [Tile::FeatherGrass, Tile::TallGrass] {
            let (sharp, blurred) = (drawn_at(tile, 0), drawn_at(tile, 3));
            assert!(blurred < sharp * 1.4, "{tile:?} is {sharp:.2} drawn sharp and {blurred:.2} at mip 3, a blob");
        }
    }

    #[test]
    fn test_tinted_foliage_is_greyscale_at_the_mean_the_tint_assumes() {
        for (tile, mean) in [
            (Tile::Leaves, 0.25),
            (Tile::TallGrass, 0.5),
            (Tile::ShortGrass, 0.5),
            (Tile::SeedGrass, 0.5),
            (Tile::FeatherGrass, 0.62),
            (Tile::Fern, 0.3),
            (Tile::Shrub, 0.27),
        ] {
            assert!(tile.tinted());
            let layer = foliage(tile);
            assert!(
                layer
                    .pixels()
                    .all(|p| p[3] < 128 || p[0] == p[1] && p[1] == p[2]),
                "{tile:?} must be greyscale, the climate tint multiplies it"
            );
            let got = image_mean(&layer, 0, layer.height())[0];
            assert!(
                (got - mean).abs() < 0.015,
                "{tile:?} averages {got}, the tint assumes {mean}"
            );
        }
    }

    #[test]
    fn test_block_means_match_the_textures() {
        let (shader, textures) = (shader_means(), material_means());
        assert_eq!(textures.len(), shader.len(), "one mean per cell");
        for (cell, (s, a)) in shader.iter().zip(&textures).enumerate() {
            let Some(a) = a else { continue };
            for i in 0..3 {
                assert!(
                    (s[i] - a[i]).abs() < 0.001,
                    "cell {cell}: shader has {s:?}, its texture averages {a:?}"
                );
            }
        }
    }
}
