diagnostic(off, derivative_uniformity);
struct Push {
    LinearizePassSize: vec4<f32>,
    OriginalSize: vec4<f32>,
    OutputSize: vec4<f32>,
    FrameCount: u32,
    SIZEHB: f32,
    SIGMA_HB: f32,
    FINE_BLOOM: f32,
    auto_res: f32,
    GLOW_MAX: f32,
}

struct UBO {
    MVP: mat4x4<f32>,
}

var<private> auto_rez: f32;
@group(0) @binding(3) 
var<uniform> params: Push;
var<private> FINE_BLOOM: f32;
var<private> invsqrsigma: f32;
var<private> vTexCoord_1: vec2<f32>;
@group(0) @binding(2) 
var LinearizePass: texture_2d<f32>;
var<private> FragColor: vec4<f32>;
@group(0) @binding(0) 
var<uniform> global: UBO;
@group(1) @binding(2) 
var _LinearizePass_sampler: sampler;

fn gaussian_u0028_f1_u003b(x: ptr<function, f32>) -> f32 {
    let _e28 = (*x);
    let _e30 = (*x);
    let _e32 = invsqrsigma;
    return exp(((-(_e28) * _e30) * _e32));
}

fn main_1() {
    var local: f32;
    var local_1: f32;
    var SourceSize1_: vec4<f32>;
    var f: f32;
    var tex: vec2<f32>;
    var color: vec4<f32>;
    var dx: vec2<f32>;
    var wsum: f32;
    var n: f32;
    var pixel: vec4<f32>;
    var w: f32;
    var param: f32;

    let _e41 = params.OriginalSize[1u];
    if (_e41 < 375f) {
        let _e44 = params.auto_res;
        let _e47 = params.OriginalSize[0u];
        local = mix(1f, 2f, clamp(((_e44 * round((_e47 / 300f))) - 1f), 0f, 1f));
    } else {
        local = 1f;
    }
    let _e54 = local;
    auto_rez = _e54;
    let _e56 = params.FINE_BLOOM;
    if (_e56 > 0.5f) {
        let _e59 = params.FINE_BLOOM;
        local_1 = _e59;
    } else {
        let _e61 = params.FINE_BLOOM;
        local_1 = mix(0.75f, 0.5f, -(_e61));
    }
    let _e64 = local_1;
    FINE_BLOOM = _e64;
    let _e66 = params.SIGMA_HB;
    let _e69 = params.SIGMA_HB;
    let _e71 = auto_rez;
    let _e73 = auto_rez;
    invsqrsigma = (1f / ((((2f * _e66) * _e69) * _e71) * _e73));
    let _e77 = params.OriginalSize;
    let _e78 = FINE_BLOOM;
    let _e79 = FINE_BLOOM;
    let _e81 = FINE_BLOOM;
    SourceSize1_ = (_e77 * vec4<f32>(_e78, _e78, (1f / _e79), (1f / _e81)));
    let _e86 = SourceSize1_[0u];
    let _e88 = vTexCoord_1[0u];
    f = fract((_e86 * _e88));
    let _e91 = f;
    f = (0.5f - _e91);
    let _e93 = SourceSize1_;
    let _e95 = vTexCoord_1;
    let _e98 = SourceSize1_;
    let _e101 = SourceSize1_;
    tex = ((floor((_e93.xy * _e95)) * _e98.zw) + (_e101.zw * 0.5f));
    color = vec4<f32>(0f, 0f, 0f, 0f);
    let _e106 = SourceSize1_[2u];
    dx = vec2<f32>(_e106, 0f);
    wsum = 0f;
    let _e109 = params.SIZEHB;
    let _e111 = auto_rez;
    n = (-(_e109) * _e111);
    loop {
        let _e113 = tex;
        let _e114 = n;
        let _e115 = dx;
        let _e118 = ue2_border_linear(LinearizePass, _LinearizePass_sampler, (_e113 + (_e115 * _e114)));
        pixel = _e118;
        let _e119 = n;
        let _e120 = f;
        param = (_e119 + _e120);
        let _e122 = gaussian_u0028_f1_u003b((&param));
        w = _e122;
        let _e124 = pixel[0u];
        let _e126 = pixel[1u];
        let _e129 = pixel[2u];
        pixel[3u] = max(max(_e124, _e126), _e129);
        let _e133 = pixel[3u];
        let _e135 = pixel[3u];
        let _e138 = pixel[3u];
        pixel[3u] = (_e138 * (_e133 * _e135));
        let _e141 = color;
        let _e142 = w;
        let _e143 = pixel;
        color = (_e141 + (_e143 * _e142));
        let _e146 = wsum;
        let _e147 = w;
        wsum = (_e146 + _e147);
        let _e149 = n;
        n = (_e149 + 1f);
        continue;
        continuing {
            let _e151 = n;
            let _e153 = params.SIZEHB;
            let _e154 = auto_rez;
            break if !((_e151 <= (_e153 * _e154)));
        }
    }
    let _e158 = color;
    let _e159 = wsum;
    color = (_e158 / vec4(_e159));
    let _e162 = color;
    let _e163 = _e162.xyz;
    let _e165 = color[3u];
    FragColor = vec4<f32>(_e163.x, _e163.y, _e163.z, pow(_e165, 0.333333f));
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
