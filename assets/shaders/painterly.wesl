// Painterly lighting for painted faces, see docs/designs/painterly-lighting.md. Identity until the model lands.

fn painterly_light(sun: vec3<f32>, ambient: vec3<f32>, glow: vec3<f32>) -> vec3<f32> {
    return sun + ambient + glow;
}
