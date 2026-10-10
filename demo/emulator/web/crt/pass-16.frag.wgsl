diagnostic(off, derivative_uniformity);
struct Push {
    SourceSize: vec4<f32>,
    OriginalSize: vec4<f32>,
    OutputSize: vec4<f32>,
    FrameCount: u32,
    SIZEVB: f32,
    SIGMA_VB: f32,
    FINE_BLOOM: f32,
}

struct UBO {
    MVP: mat4x4<f32>,
}

var<private> FINE_BLOOM: f32;
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
    let _e23 = (*x);
    let _e25 = (*x);
    let _e27 = invsqrsigma;
    return exp(((-(_e23) * _e25) * _e27));
}

fn main_1() {
    var local: f32;
    var SourceSize1_: vec4<f32>;
    var f: f32;
    var tex: vec2<f32>;
    var color: vec4<f32>;
    var dy: vec2<f32>;
    var wsum: f32;
    var n: f32;
    var pixel: vec4<f32>;
    var w: f32;
    var param: f32;

    let _e34 = params.FINE_BLOOM;
    if (_e34 > 0.5f) {
        let _e37 = params.FINE_BLOOM;
        local = _e37;
    } else {
        let _e39 = params.FINE_BLOOM;
        local = mix(0.75f, 0.5f, -(_e39));
    }
    let _e42 = local;
    FINE_BLOOM = _e42;
    let _e44 = params.SIGMA_VB;
    let _e47 = params.SIGMA_VB;
    invsqrsigma = (1f / ((2f * _e44) * _e47));
    let _e51 = params.SourceSize;
    SourceSize1_ = _e51;
    let _e53 = params.OriginalSize;
    let _e54 = _e53.yw;
    SourceSize1_[1u] = _e54.x;
    SourceSize1_[3u] = _e54.y;
    let _e59 = SourceSize1_;
    let _e60 = FINE_BLOOM;
    let _e61 = FINE_BLOOM;
    let _e63 = FINE_BLOOM;
    SourceSize1_ = (_e59 * vec4<f32>(_e60, _e60, (1f / _e61), (1f / _e63)));
    let _e68 = SourceSize1_[1u];
    let _e70 = vTexCoord_1[1u];
    f = fract((_e68 * _e70));
    let _e73 = f;
    f = (0.5f - _e73);
    let _e75 = SourceSize1_;
    let _e77 = vTexCoord_1;
    let _e80 = SourceSize1_;
    let _e83 = SourceSize1_;
    tex = ((floor((_e75.xy * _e77)) * _e80.zw) + (_e83.zw * 0.5f));
    color = vec4<f32>(0f, 0f, 0f, 0f);
    let _e88 = SourceSize1_[3u];
    dy = vec2<f32>(0f, _e88);
    wsum = 0f;
    let _e91 = params.SIZEVB;
    n = -(_e91);
    loop {
        let _e93 = tex;
        let _e94 = n;
        let _e95 = dy;
        let _e98 = ue2_border_linear(Source, _Source_sampler, (_e93 + (_e95 * _e94)));
        pixel = _e98;
        let _e99 = n;
        let _e100 = f;
        param = (_e99 + _e100);
        let _e102 = gaussian_u0028_f1_u003b((&param));
        w = _e102;
        let _e104 = pixel[3u];
        let _e106 = pixel[3u];
        let _e109 = pixel[3u];
        pixel[3u] = (_e109 * (_e104 * _e106));
        let _e112 = color;
        let _e113 = w;
        let _e114 = pixel;
        color = (_e112 + (_e114 * _e113));
        let _e117 = wsum;
        let _e118 = w;
        wsum = (_e117 + _e118);
        let _e120 = n;
        n = (_e120 + 1f);
        continue;
        continuing {
            let _e122 = n;
            let _e124 = params.SIZEVB;
            break if !((_e122 <= _e124));
        }
    }
    let _e127 = color;
    let _e128 = wsum;
    color = (_e127 / vec4(_e128));
    let _e131 = color;
    let _e132 = _e131.xyz;
    let _e134 = color[3u];
    FragColor = vec4<f32>(_e132.x, _e132.y, _e132.z, pow(_e134, 0.175f));
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
