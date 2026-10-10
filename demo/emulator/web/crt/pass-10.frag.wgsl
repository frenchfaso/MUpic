diagnostic(off, derivative_uniformity);
struct Push {
    FrameCount: u32,
    SourceSize: vec4<f32>,
    lsmooth: f32,
    lsdev: f32,
}

struct UBO {
    MVP: mat4x4<f32>,
}

@group(0) @binding(4) 
var<uniform> params: Push;
@group(0) @binding(2) 
var Source: texture_2d<f32>;
@group(0) @binding(3) 
var AvgLumPassFeedback: texture_2d<f32>;
var<private> vTexCoord_1: vec2<f32>;
var<private> FragColor: vec4<f32>;
@group(0) @binding(0) 
var<uniform> global: UBO;
@group(1) @binding(2) 
var _Source_sampler: sampler;
@group(1) @binding(3) 
var _AvgLumPassFeedback_sampler: sampler;

fn dist_u0028_vf3_u003b_vf3_u003b(A: ptr<function, vec3<f32>>, B: ptr<function, vec3<f32>>) -> f32 {
    var r: f32;
    var d: vec3<f32>;
    var c: vec3<f32>;

    let _e36 = (*A)[0u];
    let _e38 = (*B)[0u];
    r = (0.5f * (_e36 + _e38));
    let _e41 = (*A);
    let _e42 = (*B);
    d = (_e41 - _e42);
    let _e44 = r;
    let _e46 = r;
    c = vec3<f32>((2f + _e44), 4f, (3f - _e46));
    let _e49 = c;
    let _e50 = d;
    let _e52 = d;
    return (sqrt(dot((_e49 * _e50), _e52)) / 3f);
}

fn main_1() {
    var m: f32;
    var dx: vec2<f32>;
    var dy: vec2<f32>;
    var y2_: vec2<f32>;
    var x2_: vec2<f32>;
    var ltotal: f32;
    var lhistory: f32;
    var l1_: vec3<f32>;
    var r1_: vec3<f32>;
    var l2_: vec3<f32>;
    var r2_: vec3<f32>;
    var c1_: f32;
    var param: vec3<f32>;
    var param_1: vec3<f32>;
    var c2_: f32;
    var param_2: vec3<f32>;
    var param_3: vec3<f32>;
    var c3_: f32;
    var param_4: vec3<f32>;
    var param_5: vec3<f32>;

    let _e52 = params.SourceSize[0u];
    let _e56 = params.SourceSize[1u];
    m = max(log2(_e52), log2(_e56));
    let _e59 = m;
    m = (floor(max(_e59, 1f)) - 1f);
    let _e65 = params.SourceSize[0u];
    dx = vec2<f32>((1f / _e65), 0f);
    let _e70 = params.SourceSize[1u];
    dy = vec2<f32>(0f, (1f / _e70));
    let _e73 = dy;
    y2_ = (_e73 * 2f);
    let _e75 = dx;
    x2_ = (_e75 * 2f);
    ltotal = 0f;
    let _e77 = m;
    let _e78 = ue2_border_level_linear(Source, _Source_sampler, vec2<f32>(0.3f, 0.3f), _e77);
    let _e81 = ltotal;
    ltotal = (_e81 + length(_e78.xyz));
    let _e83 = m;
    let _e84 = ue2_border_level_linear(Source, _Source_sampler, vec2<f32>(0.3f, 0.7f), _e83);
    let _e87 = ltotal;
    ltotal = (_e87 + length(_e84.xyz));
    let _e89 = m;
    let _e90 = ue2_border_level_linear(Source, _Source_sampler, vec2<f32>(0.7f, 0.3f), _e89);
    let _e93 = ltotal;
    ltotal = (_e93 + length(_e90.xyz));
    let _e95 = m;
    let _e96 = ue2_border_level_linear(Source, _Source_sampler, vec2<f32>(0.7f, 0.7f), _e95);
    let _e99 = ltotal;
    ltotal = (_e99 + length(_e96.xyz));
    let _e101 = ltotal;
    ltotal = (_e101 * 0.25f);
    let _e103 = ltotal;
    ltotal = pow((0.57735026f * _e103), 0.7f);
    let _e106 = ue2_border_linear(AvgLumPassFeedback, _AvgLumPassFeedback_sampler, vec2<f32>(0.5f, 0.5f));
    lhistory = _e106.w;
    let _e108 = ltotal;
    let _e109 = lhistory;
    let _e111 = params.lsmooth;
    let _e113 = params.lsmooth;
    let _e115 = params.lsdev;
    let _e117 = ltotal;
    ltotal = mix(_e108, _e109, min(mix(_e111, (_e113 + _e115), _e117), 0.99f));
    let _e121 = vTexCoord_1;
    let _e122 = ue2_border_linear(Source, _Source_sampler, _e121);
    l1_ = _e122.xyz;
    let _e124 = vTexCoord_1;
    let _e125 = dx;
    let _e127 = ue2_border_linear(Source, _Source_sampler, (_e124 + _e125));
    r1_ = _e127.xyz;
    let _e129 = vTexCoord_1;
    let _e130 = dx;
    let _e132 = ue2_border_linear(Source, _Source_sampler, (_e129 - _e130));
    l2_ = _e132.xyz;
    let _e134 = vTexCoord_1;
    let _e135 = x2_;
    let _e137 = ue2_border_linear(Source, _Source_sampler, (_e134 + _e135));
    r2_ = _e137.xyz;
    let _e139 = l2_;
    param = _e139;
    let _e140 = l1_;
    param_1 = _e140;
    let _e141 = dist_u0028_vf3_u003b_vf3_u003b((&param), (&param_1));
    c1_ = _e141;
    let _e142 = l1_;
    param_2 = _e142;
    let _e143 = r1_;
    param_3 = _e143;
    let _e144 = dist_u0028_vf3_u003b_vf3_u003b((&param_2), (&param_3));
    c2_ = _e144;
    let _e145 = r2_;
    param_4 = _e145;
    let _e146 = r1_;
    param_5 = _e146;
    let _e147 = dist_u0028_vf3_u003b_vf3_u003b((&param_4), (&param_5));
    c3_ = _e147;
    let _e148 = c1_;
    let _e149 = c2_;
    let _e150 = c3_;
    let _e151 = ltotal;
    FragColor = vec4<f32>(_e148, _e149, _e150, _e151);
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
