// 0..1 for the whole-number cell `p` is in. Integer mixing, since the usual fract(sin)
// hash loses its precision at world coordinates in the thousands and turns into stripes.
fn hash2(p: vec2<f32>) -> f32 {
    let cell = vec2<i32>(floor(p));
    var h = (bitcast<u32>(cell.x) * 0x8da6b343u) ^ (bitcast<u32>(cell.y) * 0xd8163841u);
    h = (h ^ (h >> 15u)) * 0x2c1b3c6du;
    h = (h ^ (h >> 12u)) * 0x297a2d39u;
    h = h ^ (h >> 15u);
    return f32(h) / 4294967295.0;
}

// 0..1 for the whole-number cell `cell`, as `hash2` is.
fn hash3(cell: vec3<i32>) -> f32 {
    var h = (bitcast<u32>(cell.x) * 0x8da6b343u) ^ (bitcast<u32>(cell.y) * 0xd8163841u) ^ (bitcast<u32>(cell.z) * 0xcb1ab31fu);
    h = (h ^ (h >> 15u)) * 0x2c1b3c6du;
    h = (h ^ (h >> 12u)) * 0x297a2d39u;
    h = h ^ (h >> 15u);
    return f32(h) / 4294967295.0;
}

// 0..1, smooth between `hash2`'s values at the whole-number corners around `p`.
fn value_noise(p: vec2<f32>) -> f32 {
    let cell = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let bottom = mix(hash2(cell), hash2(cell + vec2<f32>(1.0, 0.0)), u.x);
    let top = mix(hash2(cell + vec2<f32>(0.0, 1.0)), hash2(cell + vec2<f32>(1.0, 1.0)), u.x);
    return mix(bottom, top, u.y);
}
