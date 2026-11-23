//! Ochre, a voxel engine for Bevy, re-exporting its crates: `ochre_base`, `ochre_terrain`, `ochre_world` and `ochre_gameplay`.

pub mod dev;
pub mod plugins;
pub mod settings;

pub use dev::DevEnv;
pub use plugins::{load_world, OchrePlugins};

pub use ochre_base;
pub use ochre_gameplay;
pub use ochre_terrain;
pub use ochre_world;
