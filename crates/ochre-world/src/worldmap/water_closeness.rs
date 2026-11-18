//! How close each cell is to fresh water, so ground colour can show a river's corridor from far off.

use super::climate::closeness_within;
use super::{WaterCell, WorldMap};

// Cells round any river, lake or pond that its green reaches, 3 cells being about 100 blocks.
const REACH: u8 = 3;

/// Fills `map.water_closeness`: 0..1 as a byte, 255 on fresh water and falling to 0 at `REACH` cells, squared so the green hugs the bank.
pub fn run(map: &mut WorldMap) {
    let near = closeness_within(map, REACH, |i| {
        map.water_kind[i] == WaterCell::Lake as u8 || map.water_kind[i] == WaterCell::River as u8
    });
    map.water_closeness = near.iter().map(|&c| (c * 255.0).round() as u8).collect();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worldmap::WorldConfig;

    #[test]
    fn closeness_is_full_on_the_water_and_gone_past_the_reach() {
        let config = WorldConfig { size: 8192, map_cell: 32, ..Default::default() };
        let mut map = WorldMap::empty(1, config);
        let dim = map.dim();
        let water = 100 * dim + 100;
        map.water_kind[water] = WaterCell::River as u8;
        run(&mut map);
        assert_eq!(255, map.water_closeness[water]);
        let at = |d: usize| map.water_closeness[water + d];
        assert!(at(1) > at(2) && at(2) > at(3) && at(3) > 0);
        assert_eq!(0, at(REACH as usize + 1));
        assert_eq!(0, map.water_closeness[100 * dim + 20]);
    }
}
