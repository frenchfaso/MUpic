diagnostic(off, derivative_uniformity);
struct Push {
    SourceSize: vec4<f32>,
    OriginalSize: vec4<f32>,
    OutputSize: vec4<f32>,
    FrameCount: u32,
    PR: f32,
    PG: f32,
    PB: f32,
    esrc: f32,
    bth: f32,
}

struct UBO {
    MVP: mat4x4<f32>,
}

@group(0) @binding(5) 
var<uniform> params: Push;
@group(0) @binding(3) 
var OriginalHistory0_: texture_2d<f32>;
var<private> vTexCoord_1: vec2<f32>;
@group(0) @binding(2) 
var Source: texture_2d<f32>;
@group(0) @binding(4) 
var AfterglowPassFeedback: texture_2d<f32>;
var<private> FragColor: vec4<f32>;
@group(0) @binding(0) 
var<uniform> global: UBO;
@group(1) @binding(3) 
var _OriginalHistory0_sampler: sampler;
@group(1) @binding(2) 
var _Source_sampler: sampler;
@group(1) @binding(4) 
var _AfterglowPassFeedback_sampler: sampler;

fn main_1() {
    var dx: vec2<f32>;
    var dy: vec2<f32>;
    var color0_: vec3<f32>;
    var color1_: vec3<f32>;
    var color2_: vec3<f32>;
    var color3_: vec3<f32>;
    var color4_: vec3<f32>;
    var color: vec3<f32>;
    var accumulate: vec3<f32>;
    var w: f32;
    var b: f32;
    var c: f32;
    var result: vec3<f32>;

    let _e44 = params.OriginalSize[2u];
    dx = vec2<f32>(_e44, 0f);
    let _e48 = params.OriginalSize[3u];
    dy = vec2<f32>(0f, _e48);
    let _e50 = vTexCoord_1;
    let _e51 = ue2_border_nearest(OriginalHistory0_, _OriginalHistory0_sampler, _e50);
    color0_ = _e51.xyz;
    let _e53 = vTexCoord_1;
    let _e54 = dx;
    let _e56 = ue2_border_nearest(OriginalHistory0_, _OriginalHistory0_sampler, (_e53 - _e54));
    color1_ = _e56.xyz;
    let _e58 = vTexCoord_1;
    let _e59 = dx;
    let _e61 = ue2_border_nearest(OriginalHistory0_, _OriginalHistory0_sampler, (_e58 + _e59));
    color2_ = _e61.xyz;
    let _e63 = vTexCoord_1;
    let _e64 = dy;
    let _e66 = ue2_border_nearest(OriginalHistory0_, _OriginalHistory0_sampler, (_e63 - _e64));
    color3_ = _e66.xyz;
    let _e68 = vTexCoord_1;
    let _e69 = dy;
    let _e71 = ue2_border_nearest(OriginalHistory0_, _OriginalHistory0_sampler, (_e68 + _e69));
    color4_ = _e71.xyz;
    let _e74 = params.esrc;
    if (_e74 > 1.5f) {
        let _e76 = vTexCoord_1;
        let _e77 = ue2_border_linear(Source, _Source_sampler, _e76);
        color0_ = _e77.xyz;
        let _e79 = vTexCoord_1;
        let _e80 = dx;
        let _e82 = ue2_border_linear(Source, _Source_sampler, (_e79 - _e80));
        color1_ = _e82.xyz;
        let _e84 = vTexCoord_1;
        let _e85 = dx;
        let _e87 = ue2_border_linear(Source, _Source_sampler, (_e84 + _e85));
        color2_ = _e87.xyz;
        let _e89 = vTexCoord_1;
        let _e90 = dy;
        let _e92 = ue2_border_linear(Source, _Source_sampler, (_e89 - _e90));
        color3_ = _e92.xyz;
        let _e94 = vTexCoord_1;
        let _e95 = dy;
        let _e97 = ue2_border_linear(Source, _Source_sampler, (_e94 + _e95));
        color4_ = _e97.xyz;
    }
    let _e99 = color0_;
    let _e101 = color1_;
    let _e103 = color2_;
    let _e105 = color3_;
    let _e107 = color4_;
    color = ((((((_e99 * 2.5f) + _e101) + _e103) + _e105) + _e107) / vec3(6.5f));
    let _e111 = vTexCoord_1;
    let _e112 = ue2_border_linear(AfterglowPassFeedback, _AfterglowPassFeedback_sampler, _e111);
    accumulate = _e112.xyz;
    w = 1f;
    let _e115 = params.bth;
    b = (_e115 / 255f);
    let _e118 = color0_[0u];
    let _e120 = color0_[1u];
    let _e123 = color0_[2u];
    c = max(max(_e118, _e120), _e123);
    let _e125 = b;
    let _e126 = b;
    let _e128 = c;
    w = smoothstep(_e125, (2f * _e126), _e128);
    let _e130 = color;
    let _e131 = accumulate;
    let _e133 = params.PR;
    let _e135 = params.PG;
    let _e137 = params.PB;
    let _e146 = color;
    let _e147 = w;
    result = mix(max((mix(_e130, _e131, (vec3(0.49f) + vec3<f32>(_e133, _e135, _e137))) - vec3(0.004901961f)), vec3(0f)), _e146, vec3(_e147));
    let _e150 = result;
    let _e151 = w;
    FragColor = vec4<f32>(_e150.x, _e150.y, _e150.z, _e151);
    return;
}

@fragment 
fn main(@location(0) vTexCoord: vec2<f32>) -> @location(0) vec4<f32> {
    vTexCoord_1 = vTexCoord;
    main_1();
    let _e3 = FragColor;
    return _e3;
}

@group(0) @binding(6) var ue2_border_coverage:texture_2d<f32>;
@group(1) @binding(6) var ue2_border_coverage_sampler:sampler;

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
