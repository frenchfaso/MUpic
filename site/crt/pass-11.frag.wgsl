diagnostic(off, derivative_uniformity);
struct UBO {
    SourceSize: vec4<f32>,
    MVP: mat4x4<f32>,
}

struct Push {
    OriginalSize: vec4<f32>,
    OutputSize: vec4<f32>,
    FrameCount: u32,
    GAMMA_INPUT: f32,
    downsample_levelx: f32,
    downsample_levely: f32,
    gamma_out: f32,
}

@group(0) @binding(0) 
var<uniform> global: UBO;
@group(0) @binding(4) 
var<uniform> params: Push;
@group(0) @binding(2) 
var NtscPass: texture_2d<f32>;
var<private> vTexCoord_1: vec2<f32>;
@group(0) @binding(3) 
var InterlacePass: texture_2d<f32>;
var<private> FragColor: vec4<f32>;
@group(1) @binding(2) 
var _NtscPass_sampler: sampler;
@group(1) @binding(3) 
var _InterlacePass_sampler: sampler;

fn fetch_pixel_u0028_vf2_u003b(coord: ptr<function, vec2<f32>>) -> vec3<f32> {
    var dx: vec2<f32>;
    var dy: vec2<f32>;
    var d1_: vec2<f32>;
    var d2_: vec2<f32>;
    var sum: f32;
    var result: vec3<f32>;

    let _e33 = global.SourceSize[2u];
    let _e36 = params.downsample_levelx;
    dx = (vec2<f32>(_e33, 0f) * _e36);
    let _e40 = global.SourceSize[3u];
    let _e43 = params.downsample_levely;
    dy = (vec2<f32>(0f, _e40) * _e43);
    let _e45 = dx;
    let _e46 = dy;
    d1_ = (_e45 + _e46);
    let _e48 = dx;
    let _e49 = dy;
    d2_ = (_e48 - _e49);
    sum = 15f;
    let _e51 = (*coord);
    let _e52 = ue2_border_linear(NtscPass, _NtscPass_sampler, _e51);
    let _e55 = (*coord);
    let _e56 = dx;
    let _e58 = ue2_border_linear(NtscPass, _NtscPass_sampler, (_e55 + _e56));
    let _e62 = (*coord);
    let _e63 = dx;
    let _e65 = ue2_border_linear(NtscPass, _NtscPass_sampler, (_e62 - _e63));
    let _e69 = (*coord);
    let _e70 = dy;
    let _e72 = ue2_border_linear(NtscPass, _NtscPass_sampler, (_e69 + _e70));
    let _e76 = (*coord);
    let _e77 = dy;
    let _e79 = ue2_border_linear(NtscPass, _NtscPass_sampler, (_e76 - _e77));
    let _e83 = (*coord);
    let _e84 = d1_;
    let _e86 = ue2_border_linear(NtscPass, _NtscPass_sampler, (_e83 + _e84));
    let _e89 = (*coord);
    let _e90 = d1_;
    let _e92 = ue2_border_linear(NtscPass, _NtscPass_sampler, (_e89 - _e90));
    let _e95 = (*coord);
    let _e96 = d2_;
    let _e98 = ue2_border_linear(NtscPass, _NtscPass_sampler, (_e95 + _e96));
    let _e101 = (*coord);
    let _e102 = d2_;
    let _e104 = ue2_border_linear(NtscPass, _NtscPass_sampler, (_e101 - _e102));
    result = (((((((((_e52.xyz * 3f) + (_e58.xyz * 2f)) + (_e65.xyz * 2f)) + (_e72.xyz * 2f)) + (_e79.xyz * 2f)) + _e86.xyz) + _e92.xyz) + _e98.xyz) + _e104.xyz);
    let _e107 = result;
    let _e108 = sum;
    return (_e107 / vec3(_e108));
}

fn main_1() {
    var c: vec3<f32>;
    var param: vec2<f32>;
    var intera: f32;
    var gamma_in: f32;

    let _e28 = vTexCoord_1;
    let _e29 = ue2_border_linear(NtscPass, _NtscPass_sampler, _e28);
    c = _e29.xyz;
    let _e32 = params.downsample_levelx;
    let _e34 = params.downsample_levely;
    if ((_e32 + _e34) > 0.025f) {
        let _e37 = vTexCoord_1;
        param = _e37;
        let _e38 = fetch_pixel_u0028_vf2_u003b((&param));
        c = _e38;
    }
    let _e39 = ue2_border_linear(InterlacePass, _InterlacePass_sampler, vec2<f32>(0.75f, 0.75f));
    intera = _e39.w;
    let _e42 = params.GAMMA_INPUT;
    gamma_in = _e42;
    let _e43 = c;
    let _e44 = gamma_in;
    c = pow(_e43, vec3(_e44));
    let _e48 = vTexCoord_1[0u];
    if (_e48 > 0.5f) {
        let _e50 = intera;
        gamma_in = _e50;
    } else {
        let _e51 = gamma_in;
        gamma_in = (1f / _e51);
    }
    let _e53 = c;
    let _e54 = gamma_in;
    FragColor = vec4<f32>(_e53.x, _e53.y, _e53.z, _e54);
    return;
}

@fragment 
fn main(@location(0) vTexCoord: vec2<f32>) -> @location(0) vec4<f32> {
    vTexCoord_1 = vTexCoord;
    main_1();
    let _e3 = FragColor;
    return _e3;
}

@group(0) @binding(5) var ue2_border_coverage:texture_2d<f32>;
@group(1) @binding(5) var ue2_border_coverage_sampler:sampler;

fn ue2_border_weight(dim:vec2<f32>, uv:vec2<f32>) -> f32 {
    let p=uv*dim;
    let left=clamp(p+vec2<f32>(1.0),vec2<f32>(0.5),vec2<f32>(1.5));
    let right=clamp(p-dim+vec2<f32>(2.0),vec2<f32>(1.5),vec2<f32>(2.5));
    let coordinate=select(left,right,p>dim*vec2<f32>(0.5));
    // A black 3x3 texture with one white centre texel delegates the fractional
    // border weight to the GPU's own bilinear unit. Preserve each edge's
    // orientation: reflecting right/bottom changes its quantized phase.
    return textureSampleLevel(ue2_border_coverage,ue2_border_coverage_sampler,coordinate/3.0,0.0).r;
}
fn ue2_border_linear(t: texture_2d<f32>, s: sampler, uv: vec2<f32>) -> vec4<f32> {
    let dim=vec2<f32>(textureDimensions(t,0));
    return textureSample(t,s,uv)*ue2_border_weight(dim,uv);
}
fn ue2_border_nearest(t: texture_2d<f32>, s: sampler, uv: vec2<f32>) -> vec4<f32> {
    return textureSample(t,s,uv)*select(0.0,1.0,all(uv>=vec2<f32>(0.0)) && all(uv<vec2<f32>(1.0)));
}
fn ue2_border_level_linear(t: texture_2d<f32>, s: sampler, uv: vec2<f32>, lod: f32) -> vec4<f32> {
    let level=clamp(lod,0.0,f32(textureNumLevels(t)-1u));
    let low=u32(floor(level)); let high=min(low+1u,textureNumLevels(t)-1u);
    let dl=vec2<f32>(textureDimensions(t,low)); let dh=vec2<f32>(textureDimensions(t,high));
    return mix(textureSampleLevel(t,s,uv,f32(low))*ue2_border_weight(dl,uv),textureSampleLevel(t,s,uv,f32(high))*ue2_border_weight(dh,uv),fract(level));
}
fn ue2_border_level_nearest(t: texture_2d<f32>, s: sampler, uv: vec2<f32>, lod: f32) -> vec4<f32> {
    return textureSampleLevel(t,s,uv,lod)*select(0.0,1.0,all(uv>=vec2<f32>(0.0)) && all(uv<vec2<f32>(1.0)));
}
