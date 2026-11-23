use bevy::ecs::component::Component;
use bevy::math::Vec3;
use std::collections::HashMap;

// The sculpted ground's triangles by the columns they cross, in mesh units, for plants and player collision.
#[derive(Component, Default)]
pub struct Floor {
    triangles: Vec<[Vec3; 3]>,
    columns: HashMap<(i32, i32), Vec<usize>>,
}

impl Floor {
    pub fn new(triangles: Vec<[Vec3; 3]>) -> Self {
        let mut columns: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
        for (n, t) in triangles.iter().enumerate() {
            let (low, high) = (t[0].min(t[1]).min(t[2]), t[0].max(t[1]).max(t[2]));
            for x in low.x.round() as i32..=high.x.round() as i32 {
                for z in low.z.round() as i32..=high.z.round() as i32 {
                    columns.entry((x, z)).or_default().push(n);
                }
            }
        }
        Self { triangles, columns }
    }

    // The highest surface crossing `at`'s column from `below` under it to `above` over it, and which way it faces.
    pub fn surface(&self, at: Vec3, below: f32, above: f32) -> Option<(f32, Vec3)> {
        self.surface_facing(at, below, above, 0.0)
    }

    // The same among faces whose normal points up at least `up`, so steep ones can be left out.
    pub fn surface_facing(&self, at: Vec3, below: f32, above: f32, up: f32) -> Option<(f32, Vec3)> {
        let cell = self.columns.get(&(at.x.round() as i32, at.z.round() as i32))?;
        cell.iter()
            .filter_map(|&n| {
                let [a, b, c] = self.triangles[n];
                // upward faces alone, which wind with the cross product pointing down into the ground
                if (b - a).cross(c - a).y >= 0.0 {
                    return None;
                }
                let area = (b.x - a.x) * (c.z - a.z) - (c.x - a.x) * (b.z - a.z);
                if area.abs() < 1e-6 {
                    return None;
                }
                let u = ((b.x - at.x) * (c.z - at.z) - (c.x - at.x) * (b.z - at.z)) / area;
                let v = ((c.x - at.x) * (a.z - at.z) - (a.x - at.x) * (c.z - at.z)) / area;
                let w = 1.0 - u - v;
                let height = a.y * u + b.y * v + c.y * w;
                let facing = (c - a).cross(b - a).normalize_or_zero();
                (u >= -1e-4 && v >= -1e-4 && w >= -1e-4).then_some((height, facing))
            })
            .filter(|&(y, facing)| y >= at.y - below && y <= at.y + above && facing.y >= up)
            .max_by(|p, q| p.0.total_cmp(&q.0))
    }

    // Every triangle crossing a column from `min` to `max` across, once each.
    pub fn near(&self, min: Vec3, max: Vec3) -> Vec<[Vec3; 3]> {
        let mut found: Vec<usize> = Vec::new();
        for x in min.x.round() as i32..=max.x.round() as i32 {
            for z in min.z.round() as i32..=max.z.round() as i32 {
                found.extend(self.columns.get(&(x, z)).into_iter().flatten());
            }
        }
        found.sort_unstable();
        found.dedup();
        found.into_iter().map(|n| self.triangles[n]).collect()
    }

    pub fn is_empty(&self) -> bool {
        self.triangles.is_empty()
    }

    pub fn into_triangles(self) -> Vec<[Vec3; 3]> {
        self.triangles
    }
}

#[cfg(test)]
mod tests {
    use super::Floor;
    use bevy::math::Vec3;

    // A slope rising along x from y = 0 at x = -1 to y = 2 at x = 1, and a flat shelf over it at y = 5.
    fn slope_and_shelf() -> Floor {
        let quad = |y0: f32, y1: f32| {
            let (a, b, c, d) = (Vec3::new(-1.0, y0, -1.0), Vec3::new(1.0, y1, -1.0), Vec3::new(1.0, y1, 1.0), Vec3::new(-1.0, y0, 1.0));
            [[a, b, c], [a, c, d]]
        };
        Floor::new([quad(0.0, 2.0), quad(5.0, 5.0)].concat())
    }

    #[test]
    fn test_the_highest_surface_in_reach_and_its_slope() {
        let floor = slope_and_shelf();
        let height = |at: Vec3, below: f32, above: f32| floor.surface(at, below, above).map(|(h, _)| h);
        assert!((height(Vec3::new(0.0, 0.6, 0.3), 1.0, 1.0).unwrap() - 1.0).abs() < 1e-5);
        assert!((height(Vec3::new(0.5, 1.2, -0.4), 1.0, 1.0).unwrap() - 1.5).abs() < 1e-5);
        // the shelf over the slope, when it's in reach
        assert_eq!(height(Vec3::new(0.0, 4.5, 0.0), 1.0, 1.0), Some(5.0));
        assert!((height(Vec3::new(0.0, 4.5, 0.0), 4.0, 0.0).unwrap() - 1.0).abs() < 1e-5);
        assert_eq!(height(Vec3::new(0.0, 3.0, 0.0), 1.0, 1.0), None);
        assert_eq!(height(Vec3::new(4.0, 1.0, 0.0), 1.0, 1.0), None);
        // the slope faces up and back down it, toward -x
        let (_, facing) = floor.surface(Vec3::new(0.0, 1.0, 0.0), 1.0, 1.0).unwrap();
        assert!(facing.y > 0.0 && facing.x < 0.0 && facing.z.abs() < 1e-5, "{facing}");
    }
}
