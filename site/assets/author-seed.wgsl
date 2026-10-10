// MUpic candidate hints: shared by native wgpu and browser WebGPU.
// This approximate CIEDE2000-bound walk only seeds the exact CPU search.
// It never makes the final palette decision, so GPU f32 differences cannot
// change author-preset pixels. Header/node ABI: AuthorDither::gpu_search_data.
@group(0) @binding(0) var<storage, read> tree: array<u32>;
@group(0) @binding(1) var<storage, read> keys: array<u32>;
@group(0) @binding(2) var<storage, read_write> hints: array<u32>;

fn lab(rgb: vec3<f32>) -> vec3<f32> {
    let xyz = vec3<f32>(
        dot(rgb, vec3<f32>(0.488718, 0.310680, 0.200602)) / 0.664922,
        dot(rgb, vec3<f32>(0.176204, 0.812985, 0.0108109)) / 1.1338698,
        dot(rgb, vec3<f32>(0.0, 0.0102048, 0.989795)) / 1.2012079);
    let f = select(xyz * (841.0 / 108.0) + vec3<f32>(4.0 / 29.0),
        pow(max(xyz, vec3<f32>(0.0)), vec3<f32>(1.0 / 3.0)),
        xyz > vec3<f32>(216.0 / 24389.0));
    return vec3<f32>(116.0 * f.y - 16.0, 500.0 * (f.x-f.y), 200.0 * (f.y-f.z));
}

fn bound(node: u32, q: vec3<f32>, chroma: f32) -> f32 {
    let base = tree[6] + node * 12u;
    let lo = vec3<f32>(bitcast<f32>(tree[base]), bitcast<f32>(tree[base+1u]), bitcast<f32>(tree[base+2u]));
    let hi = vec3<f32>(bitcast<f32>(tree[base+4u]), bitcast<f32>(tree[base+5u]), bitcast<f32>(tree[base+6u]));
    let delta = max(max(lo-q, q-hi), vec3<f32>(0.0));
    let l50 = max(abs((q.x + lo.x) * 0.5 - 50.0), abs((q.x + hi.x) * 0.5 - 50.0));
    let sl = 1.0 + 0.015 * l50 * l50 / sqrt(20.0 + l50 * l50);
    let cp = 0.75 * (chroma + bitcast<f32>(tree[base+10u]));
    let sc = 1.0 + 0.045 * cp;
    return delta.x * delta.x / (sl * sl) + 0.1339745962155614 * dot(delta.yz, delta.yz) / (sc * sc);
}

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let at = gid.x;
    if (at >= arrayLength(&keys)) { return; }
    let key = keys[at];
    let bytes = vec3<u32>((key >> 16u) & 255u, (key >> 8u) & 255u, key & 255u);
    let neutral = tree[7] != 0u && max(max(bytes.x, bytes.y), bytes.z) - min(min(bytes.x, bytes.y), bytes.z) <= 8u;
    let q = lab(vec3<f32>(bytes) / 255.0);
    let chroma = length(q.yz);
    var node = select(tree[2], tree[3], neutral);
    loop {
        let base = tree[6] + node * 12u;
        let a = tree[base+8u];
        let b = tree[base+9u];
        if (a == 0xffffffffu) { break; }
        node = select(b, a, bound(a,q,chroma) <= bound(b,q,chroma));
    }
    let chosen = tree[tree[4] + tree[tree[6] + node * 12u + 3u]];
    // Opaque u32 hint: candidate[17 bits] + global leaf[15 bits]. The CPU
    // validates membership and checks every plan in that leaf exactly.
    hints[at] = chosen | (node << 17u);
}
