diagnostic(off, derivative_uniformity);
struct Push {
    SourceSize: vec4<f32>,
    OriginalSize: vec4<f32>,
    OutputSize: vec4<f32>,
    FrameCount: u32,
    SIZEV: f32,
    SIGMA_V: f32,
    FINE_GLOW: f32,
}

struct UBO {
    MVP: mat4x4<f32>,
}

var<private> FINE_GLOW: f32;
@group(0) @binding(3) 
var<uniform> params: Push;
var<private> invsqrsigma: f32;
var<private> vTexCoord_1: vec2<f32>;
@group(0) @binding(2) 
var Source: texture_2d<f32>;
var<private> FragColor: vec4<f32>;
@group(0) @binding(0) 
var<uniform> global: UBO;
@group(1) @binding(2) 
var _Source_sampler: sampler;

fn gaussian_u0028_f1_u003b(x: ptr<function, f32>) -> f32 {
    let _e24 = (*x);
    let _e26 = (*x);
    let _e28 = invsqrsigma;
    return exp(((-(_e24) * _e26) * _e28));
}

fn main_1() {
    var local: f32;
    var SourceSize1_: vec4<f32>;
    var f: f32;
    var tex: vec2<f32>;
    var color: vec3<f32>;
    var dy: vec2<f32>;
    var wsum: f32;
    var n: f32;
    var pixel: vec3<f32>;
    var w: f32;
    var param: f32;

    let _e35 = params.FINE_GLOW;
    if (_e35 > 0.5f) {
        let _e38 = params.FINE_GLOW;
        local = _e38;
    } else {
        let _e40 = params.FINE_GLOW;
        local = mix(0.75f, 0.5f, -(_e40));
    }
    let _e43 = local;
    FINE_GLOW = _e43;
    let _e45 = params.SIGMA_V;
    let _e48 = params.SIGMA_V;
    invsqrsigma = (1f / ((2f * _e45) * _e48));
    let _e53 = params.SourceSize[0u];
    let _e56 = params.OriginalSize[1u];
    let _e59 = params.SourceSize[2u];
    let _e62 = params.OriginalSize[3u];
    let _e64 = FINE_GLOW;
    let _e65 = FINE_GLOW;
    let _e67 = FINE_GLOW;
    SourceSize1_ = (vec4<f32>(_e53, _e56, _e59, _e62) * vec4<f32>(_e64, _e64, (1f / _e65), (1f / _e67)));
    let _e72 = SourceSize1_[1u];
    let _e74 = vTexCoord_1[1u];
    f = fract((_e72 * _e74));
    let _e77 = f;
    f = (0.5f - _e77);
    let _e79 = SourceSize1_;
    let _e81 = vTexCoord_1;
    let _e84 = SourceSize1_;
    let _e87 = SourceSize1_;
    tex = ((floor((_e79.xy * _e81)) * _e84.zw) + (_e87.zw * 0.5f));
    color = vec3<f32>(0f, 0f, 0f);
    let _e92 = SourceSize1_[3u];
    dy = vec2<f32>(0f, _e92);
    wsum = 0f;
    let _e95 = params.SIZEV;
    n = -(_e95);
    loop {
        let _e97 = tex;
        let _e98 = n;
        let _e99 = dy;
        let _e102 = ue2_border_linear(Source, _Source_sampler, (_e97 + (_e99 * _e98)));
        pixel = _e102.xyz;
        let _e104 = n;
        let _e105 = f;
        param = (_e104 + _e105);
        let _e107 = gaussian_u0028_f1_u003b((&param));
        w = _e107;
        let _e108 = color;
        let _e109 = w;
        let _e110 = pixel;
        color = (_e108 + (_e110 * _e109));
        let _e113 = wsum;
        let _e114 = w;
        wsum = (_e113 + _e114);
        let _e116 = n;
        n = (_e116 + 1f);
        continue;
        continuing {
            let _e118 = n;
            let _e120 = params.SIZEV;
            break if !((_e118 <= _e120));
        }
    }
    let _e123 = color;
    let _e124 = wsum;
    color = (_e123 / vec3(_e124));
    let _e127 = color;
    FragColor = vec4<f32>(_e127.x, _e127.y, _e127.z, 1f);
    return;
}

@fragment 
fn main(@location(0) vTexCoord: vec2<f32>) -> @location(0) vec4<f32> {
    vTexCoord_1 = vTexCoord;
    main_1();
    let _e3 = FragColor;
    return _e3;
}

@group(0) @binding(4) var ue2_border_coverage:texture_2d<f32>;
@group(1) @binding(4) var ue2_border_coverage_sampler:sampler;

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
