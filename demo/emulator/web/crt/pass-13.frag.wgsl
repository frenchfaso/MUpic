diagnostic(off, derivative_uniformity);
struct Push {
    LinearizePassSize: vec4<f32>,
    OriginalSize: vec4<f32>,
    OutputSize: vec4<f32>,
    FrameCount: u32,
    SIZEH: f32,
    SIGMA_H: f32,
    FINE_GLOW: f32,
    m_glow: f32,
    m_glow_cutoff: f32,
    m_glow_low: f32,
    m_glow_high: f32,
    m_glow_dist: f32,
    m_glow_mask: f32,
    auto_res: f32,
}

struct UBO {
    MVP: mat4x4<f32>,
}

var<private> auto_rez: f32;
@group(0) @binding(3) 
var<uniform> params: Push;
var<private> FINE_GLOW: f32;
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
    let _e29 = (*x);
    let _e31 = (*x);
    let _e33 = invsqrsigma;
    return exp(((-(_e29) * _e31) * _e33));
}

fn plant_u0028_vf3_u003b_f1_u003b(tar: ptr<function, vec3<f32>>, r: ptr<function, f32>) -> vec3<f32> {
    var t: f32;

    let _e32 = (*tar)[0u];
    let _e34 = (*tar)[1u];
    let _e37 = (*tar)[2u];
    t = (max(max(_e32, _e34), _e37) + 0.00001f);
    let _e40 = (*tar);
    let _e41 = (*r);
    let _e43 = t;
    return ((_e40 * _e41) / vec3(_e43));
}

fn main_1() {
    var local: f32;
    var local_1: f32;
    var SourceSize1_: vec4<f32>;
    var f: f32;
    var tex: vec2<f32>;
    var color: vec3<f32>;
    var dx: vec2<f32>;
    var wsum: f32;
    var n: f32;
    var pixel: vec3<f32>;
    var param: vec3<f32>;
    var param_1: f32;
    var w: f32;
    var param_2: f32;

    let _e44 = params.OriginalSize[1u];
    if (_e44 < 375f) {
        let _e47 = params.auto_res;
        let _e50 = params.OriginalSize[0u];
        local = mix(1f, 2f, clamp(((_e47 * round((_e50 / 300f))) - 1f), 0f, 1f));
    } else {
        local = 1f;
    }
    let _e57 = local;
    auto_rez = _e57;
    let _e59 = params.FINE_GLOW;
    if (_e59 > 0.5f) {
        let _e62 = params.FINE_GLOW;
        local_1 = _e62;
    } else {
        let _e64 = params.FINE_GLOW;
        local_1 = mix(0.75f, 0.5f, -(_e64));
    }
    let _e67 = local_1;
    FINE_GLOW = _e67;
    let _e69 = params.SIGMA_H;
    let _e72 = params.SIGMA_H;
    let _e74 = auto_rez;
    let _e76 = auto_rez;
    invsqrsigma = (1f / ((((2f * _e69) * _e72) * _e74) * _e76));
    let _e80 = params.OriginalSize;
    let _e81 = FINE_GLOW;
    let _e82 = FINE_GLOW;
    let _e84 = FINE_GLOW;
    SourceSize1_ = (_e80 * vec4<f32>(_e81, _e81, (1f / _e82), (1f / _e84)));
    let _e89 = SourceSize1_[0u];
    let _e91 = vTexCoord_1[0u];
    f = fract((_e89 * _e91));
    let _e94 = f;
    f = (0.5f - _e94);
    let _e96 = SourceSize1_;
    let _e98 = vTexCoord_1;
    let _e101 = SourceSize1_;
    let _e104 = SourceSize1_;
    tex = ((floor((_e96.xy * _e98)) * _e101.zw) + (_e104.zw * 0.5f));
    color = vec3<f32>(0f, 0f, 0f);
    let _e109 = SourceSize1_[2u];
    dx = vec2<f32>(_e109, 0f);
    wsum = 0f;
    let _e112 = params.SIZEH;
    let _e114 = auto_rez;
    n = (-(_e112) * _e114);
    loop {
        let _e116 = tex;
        let _e117 = n;
        let _e118 = dx;
        let _e121 = ue2_border_linear(LinearizePass, _LinearizePass_sampler, (_e116 + (_e118 * _e117)));
        pixel = _e121.xyz;
        let _e124 = params.m_glow;
        if (_e124 > 0.5f) {
            let _e126 = pixel;
            let _e128 = params.m_glow_cutoff;
            pixel = max((_e126 - vec3(_e128)), vec3(0f));
            let _e134 = pixel[0u];
            let _e136 = pixel[1u];
            let _e139 = pixel[2u];
            let _e142 = params.m_glow_cutoff;
            let _e145 = pixel;
            param = _e145;
            param_1 = max((max(max(_e134, _e136), _e139) - _e142), 0f);
            let _e146 = plant_u0028_vf3_u003b_f1_u003b((&param), (&param_1));
            pixel = _e146;
        }
        let _e147 = n;
        let _e148 = f;
        param_2 = (_e147 + _e148);
        let _e150 = gaussian_u0028_f1_u003b((&param_2));
        w = _e150;
        let _e151 = color;
        let _e152 = w;
        let _e153 = pixel;
        color = (_e151 + (_e153 * _e152));
        let _e156 = wsum;
        let _e157 = w;
        wsum = (_e156 + _e157);
        let _e159 = n;
        n = (_e159 + 1f);
        continue;
        continuing {
            let _e161 = n;
            let _e163 = params.SIZEH;
            let _e164 = auto_rez;
            break if !((_e161 <= (_e163 * _e164)));
        }
    }
    let _e168 = color;
    let _e169 = wsum;
    color = (_e168 / vec3(_e169));
    let _e172 = color;
    FragColor = vec4<f32>(_e172.x, _e172.y, _e172.z, 1f);
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
