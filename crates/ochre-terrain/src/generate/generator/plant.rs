//! A lone plant drawn as the mesher draws the ones that grow from blocks, for a game that stands plants off the block grid.
use bevy::math::Vec3;
use bevy::render::mesh::Mesh;
use ochre_base::block::{BlockType, Face};
use ochre_base::terrain_source::TerrainSource;

use super::buffers::{GrassTint, MeshBuffers};
use super::mesh::{push_tuft, Tuft};

/// The vertex tint the mesher gives a plant of `block` standing at (x, z): the climate's, and 1 where the plant keeps its own colour.
pub fn plant_tint(terrain: &dyn TerrainSource, block: BlockType, x: f64, z: f64) -> Vec3 {
    if !block.tile(Face::Side).tinted() {
        return Vec3::ONE;
    }
    GrassTint::new(terrain, Vec3::new(x as f32 + 0.5, 0.0, z as f32 + 0.5), 1.0).cold_at(Vec3::ZERO)
}

/// One plant of `block`, rooted at the origin and `height` tall, drawn across `half_width` either side with `ChunkMaterial`.
///
/// A shrub, heath or big fern is three crossed pairs of cards, turned by `angle`, and any other plant one pair. `tint` is from `plant_tint`.
pub fn plant_mesh(block: BlockType, tint: Vec3, height: f32, half_width: f32, angle: f32) -> Mesh {
    let mut buffers = MeshBuffers::default();
    let big = matches!(block, BlockType::Shrub | BlockType::Heath | BlockType::BigFern);
    let turns: &[f32] = if big { &[0.0, std::f32::consts::FRAC_PI_6, std::f32::consts::FRAC_PI_3] } else { &[0.0] };
    for turn in turns {
        let tuft = Tuft { root: Vec3::ZERO, ground: Vec3::Y, height, half_width, angle: angle + turn, lean: Vec3::ZERO };
        push_tuft(&mut buffers, &tuft, 1.0, block.tile(Face::Side).atlas_cell(), tint, 1.0);
    }
    buffers.build_mesh()
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::render::mesh::VertexAttributeValues;

    #[test]
    fn a_shrub_is_three_crossed_pairs_of_shrub_cards_in_its_tint() {
        let tint = Vec3::new(0.6, 0.9, 0.4);
        let mesh = plant_mesh(BlockType::Shrub, tint, 1.0, 0.5, 0.3);
        let Some(VertexAttributeValues::Float32x4(colors)) = mesh.attribute(Mesh::ATTRIBUTE_COLOR) else { panic!("no colours") };
        assert_eq!(24, colors.len());
        let cell = ochre_base::block::Tile::Shrub.atlas_cell();
        assert!(colors.iter().all(|c| (c[3] - cell).abs() < 0.5 && c[1] >= tint.y * 0.6));
        assert_eq!(8, plant_mesh(BlockType::TallGrass, tint, 1.0, 0.5, 0.0).count_vertices());
    }
}
