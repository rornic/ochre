use std::collections::VecDeque;

use super::generator::FACE_NORMAL_OFFSET;

use bevy::math::IVec3;

pub use ochre_base::MAX_LIGHT;

// Sky light over a box of cells, worked out from how much light each lets through.
pub struct SkyLight {
    min: IVec3,
    size: IVec3,
    levels: Vec<u8>,
}

impl SkyLight {
    /// Lights the box of `size` cells from `min`, whose `light_opacity`s are in `opacity`
    /// in `SkyLight::index` order, with `sky` falling into the top of every column.
    pub fn new(min: IVec3, size: IVec3, sky: u8, opacity: Vec<u8>) -> Self {
        assert_eq!((size.x * size.y * size.z) as usize, opacity.len());
        let mut levels = vec![0; opacity.len()];
        // whether anything light can pass through is shaded, so light has somewhere to spread
        let mut shade = false;
        // each column is a contiguous run of cells, lowest first
        for (column, opacity) in levels
            .chunks_mut(size.y as usize)
            .zip(opacity.chunks(size.y as usize))
        {
            // straight down, it keeps its full strength through open air
            let mut level = sky;
            for (cell, &opacity) in column.iter_mut().zip(opacity).rev() {
                level = level.saturating_sub(opacity);
                *cell = level;
                shade |= opacity < MAX_LIGHT && level < MAX_LIGHT;
            }
        }
        let mut light = Self { min, size, levels };
        if shade {
            light.spread(&opacity);
        }
        light
    }

    pub fn index(size: IVec3, local: IVec3) -> usize {
        ((local.x * size.z + local.z) * size.y + local.y) as usize
    }

    // Floods light from every lit cell into any neighbour it would brighten.
    fn spread(&mut self, opacity: &[u8]) {
        let mut queue: VecDeque<IVec3> = VecDeque::new();
        let (size_x, size_y, size_z) = (
            self.size.x as usize,
            self.size.y as usize,
            self.size.z as usize,
        );
        for x in 0..size_x {
            for z in 0..size_z {
                let base = (x * size_z + z) * size_y;
                let column = &self.levels[base..base + size_y];
                // where the columns beside this one start, those that are inside the box
                let beside = [
                    (x > 0).then(|| base - size_z * size_y),
                    (x + 1 < size_x).then(|| base + size_z * size_y),
                    (z > 0).then(|| base - size_y),
                    (z + 1 < size_z).then(|| base + size_y),
                ];
                for (y, &level) in column.iter().enumerate() {
                    // only light beside something darker has anywhere to go
                    if level <= 1 {
                        continue;
                    }
                    let darker = |next: u8| next + 1 < level;
                    if (y > 0 && darker(column[y - 1]))
                        || (y + 1 < size_y && darker(column[y + 1]))
                        || beside
                            .iter()
                            .flatten()
                            .any(|&start| darker(self.levels[start + y]))
                    {
                        queue.push_back(IVec3::new(x as i32, y as i32, z as i32));
                    }
                }
            }
        }
        while let Some(local) = queue.pop_front() {
            let level = self.levels[Self::index(self.size, local)];
            for offset in FACE_NORMAL_OFFSET {
                let next = local + offset;
                if next.cmplt(IVec3::ZERO).any() || next.cmpge(self.size).any() {
                    continue;
                }
                let cell = Self::index(self.size, next);
                let lit = level.saturating_sub(1 + opacity[cell]);
                if lit > self.levels[cell] {
                    self.levels[cell] = lit;
                    queue.push_back(next);
                }
            }
        }
    }

    /// Gives each of `cells`, which hold something drawn without hiding what is beside it, the light of the brightest cell next to it.
    pub fn open(&mut self, cells: &[IVec3]) {
        let lifted: Vec<u8> = cells
            .iter()
            .map(|&cell| {
                FACE_NORMAL_OFFSET
                    .iter()
                    .map(|&offset| self.level(cell + offset))
                    .max()
                    .unwrap_or(0)
            })
            .collect();
        for (&cell, level) in cells.iter().zip(lifted) {
            let local = cell - self.min;
            if local.cmpge(IVec3::ZERO).all() && local.cmplt(self.size).all() {
                self.levels[Self::index(self.size, local)] = level;
            }
        }
    }

    /// The light at `coord`, or full light outside the box.
    pub fn level(&self, coord: IVec3) -> u8 {
        let local = coord - self.min;
        if local.cmplt(IVec3::ZERO).any() || local.cmpge(self.size).any() {
            return MAX_LIGHT;
        }
        self.levels[Self::index(self.size, local)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ochre_base::block::BlockType;

    fn light(size: IVec3, block: impl Fn(IVec3) -> BlockType) -> SkyLight {
        let mut opacity = vec![0; (size.x * size.y * size.z) as usize];
        for x in 0..size.x {
            for z in 0..size.z {
                for y in 0..size.y {
                    let c = IVec3::new(x, y, z);
                    opacity[SkyLight::index(size, c)] = block(c).light_opacity();
                }
            }
        }
        SkyLight::new(IVec3::ZERO, size, MAX_LIGHT, opacity)
    }

    // a 9x12x9 box with a flat roof of `roof` blocks at y 8, from x and z 2 to 6
    fn under_roof(roof: BlockType) -> SkyLight {
        light(IVec3::new(9, 12, 9), |c| {
            let roofed = (2..=6).contains(&c.x) && (2..=6).contains(&c.z) && c.y == 8;
            if c.y == 0 {
                BlockType::Rock
            } else if roofed {
                roof
            } else {
                BlockType::Air
            }
        })
    }

    #[test]
    fn test_open_sky_is_fully_lit_down_to_the_ground() {
        let light = under_roof(BlockType::Air);
        assert_eq!(MAX_LIGHT, light.level(IVec3::new(4, 1, 4)));
        assert_eq!(0, light.level(IVec3::new(4, 0, 4)));
    }

    #[test]
    fn test_light_creeps_under_a_roof_and_fades_inwards() {
        let light = under_roof(BlockType::Rock);
        let edge = light.level(IVec3::new(2, 4, 4));
        let middle = light.level(IVec3::new(4, 4, 4));
        // the cell under the roof's edge is one step in from open sky
        assert_eq!(MAX_LIGHT - 1, edge);
        assert_eq!(MAX_LIGHT - 3, middle);
        assert_eq!(MAX_LIGHT, light.level(IVec3::new(1, 4, 4)));
    }

    #[test]
    fn test_leaves_shade_less_than_stone() {
        let leaves = under_roof(BlockType::Leaves).level(IVec3::new(4, 4, 4));
        let stone = under_roof(BlockType::Rock).level(IVec3::new(4, 4, 4));
        assert_eq!(MAX_LIGHT - 1, leaves);
        assert!(leaves > stone);
    }

    #[test]
    fn test_deep_water_darkens() {
        let light = light(IVec3::new(1, 20, 1), |c| {
            if c.y < 19 {
                BlockType::Water
            } else {
                BlockType::Air
            }
        });
        let depths: Vec<u8> = (0..20)
            .rev()
            .map(|y| light.level(IVec3::new(0, y, 0)))
            .collect();
        assert!(depths.windows(2).all(|w| w[0] >= w[1]), "{depths:?}");
        assert_eq!(0, light.level(IVec3::ZERO));
    }

    #[test]
    fn test_outside_the_box_is_lit() {
        let light = under_roof(BlockType::Rock);
        assert_eq!(MAX_LIGHT, light.level(IVec3::new(-1, 4, 4)));
    }
}
