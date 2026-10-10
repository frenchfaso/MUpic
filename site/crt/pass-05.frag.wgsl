diagnostic(off, derivative_uniformity);
struct Push {
    SourceSize: vec4<f32>,
    OriginalSize: vec4<f32>,
    OutputSize: vec4<f32>,
    FrameCount: u32,
}

struct UBO {
    MVP: mat4x4<f32>,
}

@group(0) @binding(2) 
var Source: texture_2d<f32>;
var<private> vTexCoord_1: vec2<f32>;
@group(0) @binding(3) 
var PalPass1Feedback: texture_2d<f32>;
var<private> FragColor: vec4<f32>;
@group(0) @binding(4) 
var<uniform> params: Push;
@group(0) @binding(0) 
var<uniform> global: UBO;
@group(1) @binding(2) 
var _Source_sampler: sampler;
@group(1) @binding(3) 
var _PalPass1Feedback_sampler: sampler;

fn rgb2yuv_u0028_vf3_u003b(rgb: ptr<function, vec3<f32>>) -> vec3<f32> {
    let _e22 = (*rgb);
    return (_e22 * mat3x3<f32>(vec3<f32>(0.299f, 0.587f, 0.114f), vec3<f32>(-0.14713f, -0.28886f, 0.436f), vec3<f32>(0.615f, -0.51499f, -0.10001f)));
}

fn main_1() {
    var ycolor: vec3<f32>;
    var param: vec3<f32>;
    var pcolor: vec4<f32>;

    let _e24 = vTexCoord_1;
    let _e25 = ue2_border_nearest(Source, _Source_sampler, _e24);
    param = _e25.xyz;
    let _e27 = rgb2yuv_u0028_vf3_u003b((&param));
    ycolor = _e27;
    let _e28 = vTexCoord_1;
    let _e29 = ue2_border_nearest(PalPass1Feedback, _PalPass1Feedback_sampler, _e28);
    pcolor = _e29;
    let _e30 = ycolor;
    let _e31 = _e30.yz;
    let _e32 = pcolor;
    let _e33 = _e32.xy;
    FragColor = vec4<f32>(_e31.x, _e31.y, _e33.x, _e33.y);
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
