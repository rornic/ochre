use super::{Biome, TreeScale};

/// A biome that sets how thickly trees grow, how big they get and how lush or dry the grass reads: a
/// world map picks it, and the ground keeps the base terrain's blocks.
pub struct Plain {
    name: &'static str,
    trees: f64,
    shift: (f32, f32),
    scale: TreeScale,
}

impl Plain {
    pub fn new(name: &'static str, trees: f64) -> Self {
        Self {
            name,
            trees,
            shift: (0.0, 0.0),
            scale: TreeScale::default(),
        }
    }

    /// Reads `temperature` warmer and `humidity` wetter, in grass and flora tint.
    pub fn shifted(mut self, temperature: f32, humidity: f32) -> Self {
        self.shift = (temperature, humidity);
        self
    }

    /// Grows trees `height`, `spread` and `squash` times the usual.
    pub fn scaled(mut self, height: f64, spread: f64, squash: f64) -> Self {
        self.scale = TreeScale {
            height,
            spread,
            squash,
            giants: self.scale.giants,
        };
        self
    }

    /// Lets `share` of the biome's broadleaf trees grow as emergent giants.
    pub fn with_giants(mut self, share: f64) -> Self {
        self.scale.giants = share;
        self
    }
}

impl Biome for Plain {
    fn name(&self) -> &'static str {
        self.name
    }

    fn tree_density(&self) -> Option<f64> {
        Some(self.trees)
    }

    fn climate_shift(&self) -> (f32, f32) {
        self.shift
    }

    fn tree_scale(&self) -> TreeScale {
        self.scale
    }
}
