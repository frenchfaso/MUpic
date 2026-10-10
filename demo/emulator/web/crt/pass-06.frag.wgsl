diagnostic(off, derivative_uniformity);
struct Push {
    SourceSize: vec4<f32>,
    OriginalSize: vec4<f32>,
    OutputSize: vec4<f32>,
    FrameCount: u32,
    BRIGHTNESS: f32,
    SATURATION: f32,
    ARTIFACTING: f32,
    FRINGING: f32,
    RFNOISE: f32,
    RFNOISE1_: f32,
    RFNOISE2_: f32,
    PHMOD: f32,
    DLINE: f32,
    GCORR: f32,
    PAL_FILT: f32,
}

struct UBO {
    MVP: mat4x4<f32>,
}

var<private> mmat: mat3x3<f32>;
@group(0) @binding(4) 
var<uniform> params: Push;
@group(0) @binding(3) 
var InterlacePass: texture_2d<f32>;
var<private> vTexCoord_1: vec2<f32>;
@group(0) @binding(2) 
var Source: texture_2d<f32>;
var<private> FragColor: vec4<f32>;
@group(0) @binding(0) 
var<uniform> global: UBO;
@group(1) @binding(3) 
var _InterlacePass_sampler: sampler;
@group(1) @binding(2) 
var _Source_sampler: sampler;

fn edge_cycles_u0028_i1_u003b_u1_u003b(line: ptr<function, i32>, frameCount: ptr<function, u32>) -> f32 {
    var cycles: f32;
    var phi_99_: bool;
    var phi_100_: bool;

    let _e57 = (*line);
    cycles = (f32(_e57) * -0.25f);
    let _e61 = params.PHMOD;
    let _e62 = (_e61 == 2f);
    phi_100_ = _e62;
    if !(_e62) {
        let _e65 = params.PHMOD;
        let _e66 = (_e65 == 0f);
        phi_99_ = _e66;
        if _e66 {
            let _e69 = params.OriginalSize[1u];
            phi_99_ = (_e69 > 375f);
        }
        let _e72 = phi_99_;
        phi_100_ = _e72;
    }
    let _e74 = phi_100_;
    if _e74 {
        let _e75 = (*frameCount);
        let _e79 = cycles;
        cycles = (_e79 + (f32((_e75 & 7u)) * -0.125f));
    }
    let _e81 = cycles;
    return _e81;
}

fn rgb2yuv_u0028_vf3_u003b(rgb: ptr<function, vec3<f32>>) -> vec3<f32> {
    let _e55 = (*rgb);
    return (_e55 * mat3x3<f32>(vec3<f32>(0.299f, 0.587f, 0.114f), vec3<f32>(-0.14713f, -0.28886f, 0.436f), vec3<f32>(0.615f, -0.51499f, -0.10001f)));
}

fn main_1() {
    var rgb_1: vec3<f32>;
    var yuv: vec3<f32>;
    var param: vec3<f32>;
    var res4_: vec3<f32>;
    var lum: f32;
    var yuvd: vec3<f32>;
    var param_1: vec3<f32>;
    var line_1: i32;
    var xN: f32;
    var cycles_1: f32;
    var param_2: i32;
    var param_3: u32;
    var phase: f32;
    var linesign: f32;
    var uvmod: vec2<f32>;
    var phi_296_: bool;
    var phi_304_: bool;

    let _e70 = params.BRIGHTNESS;
    let _e72 = params.FRINGING;
    let _e74 = params.FRINGING;
    let _e76 = params.ARTIFACTING;
    let _e78 = params.SATURATION;
    let _e81 = params.ARTIFACTING;
    let _e83 = params.SATURATION;
    mmat = mat3x3<f32>(vec3<f32>(_e70, _e72, _e74), vec3<f32>(_e76, (2f * _e78), 0f), vec3<f32>(_e81, 0f, (2f * _e83)));
    let _e89 = vTexCoord_1;
    let _e90 = ue2_border_linear(InterlacePass, _InterlacePass_sampler, _e89);
    rgb_1 = _e90.xyz;
    let _e92 = rgb_1;
    param = _e92;
    let _e93 = rgb2yuv_u0028_vf3_u003b((&param));
    yuv = _e93;
    let _e95 = yuv[0u];
    let _e98 = params.GCORR;
    yuv[0u] = pow(max(_e95, 0f), _e98);
    let _e101 = yuv;
    res4_ = _e101;
    let _e103 = yuv[0u];
    lum = _e103;
    let _e105 = yuv[0u];
    let _e106 = vTexCoord_1;
    let _e107 = ue2_border_nearest(Source, _Source_sampler, _e106);
    let _e108 = _e107.zw;
    yuvd = vec3<f32>(_e105, _e108.x, _e108.y);
    let _e113 = params.DLINE;
    if (_e113 < 2.5f) {
        let _e115 = vTexCoord_1;
        let _e118 = params.OriginalSize[3u];
        let _e121 = ue2_border_linear(InterlacePass, _InterlacePass_sampler, (_e115 - vec2<f32>(0f, _e118)));
        param_1 = _e121.xyz;
        let _e123 = rgb2yuv_u0028_vf3_u003b((&param_1));
        yuvd = _e123;
        let _e124 = lum;
        yuvd[0u] = _e124;
    }
    let _e127 = vTexCoord_1[1u];
    let _e130 = params.OriginalSize[1u];
    line_1 = i32(floor((_e127 * _e130)));
    let _e135 = vTexCoord_1[0u];
    xN = (_e135 * 283.7516f);
    let _e137 = xN;
    let _e138 = line_1;
    param_2 = _e138;
    let _e140 = params.FrameCount;
    param_3 = _e140;
    let _e141 = edge_cycles_u0028_i1_u003b_u1_u003b((&param_2), (&param_3));
    cycles_1 = (_e137 + _e141);
    let _e143 = cycles_1;
    phase = (6.2831855f * _e143);
    let _e145 = line_1;
    linesign = select(-1f, 1f, ((_e145 - (i32(floor((f32(_e145) / f32(2i)))) * 2i)) == 0i));
    let _e155 = phase;
    let _e157 = phase;
    let _e158 = linesign;
    uvmod = vec2<f32>(cos(_e155), sin((_e157 * _e158)));
    let _e162 = yuv;
    let _e164 = uvmod;
    let _e165 = (_e162.yz * _e164);
    yuv[1u] = _e165.x;
    yuv[2u] = _e165.y;
    let _e170 = yuv;
    let _e171 = mmat;
    yuv = (_e170 * _e171);
    let _e173 = yuv;
    let _e175 = uvmod;
    let _e176 = (_e173.yz * _e175);
    yuv[1u] = _e176.x;
    yuv[2u] = _e176.y;
    let _e182 = params.DLINE;
    if (_e182 > 0.5f) {
        let _e185 = params.DLINE;
        if (_e185 > 4.5f) {
            let _e188 = uvmod[0u];
            let _e191 = uvmod[1u];
            uvmod = vec2<f32>(-(_e188), -(_e191));
        } else {
            let _e195 = uvmod[0u];
            let _e197 = uvmod[1u];
            uvmod = vec2<f32>(_e195, -(_e197));
        }
        let _e200 = yuvd;
        let _e202 = uvmod;
        let _e203 = (_e200.yz * _e202);
        yuvd[1u] = _e203.x;
        yuvd[2u] = _e203.y;
        let _e208 = yuvd;
        let _e209 = mmat;
        yuvd = (_e208 * _e209);
        let _e211 = yuvd;
        let _e213 = uvmod;
        let _e214 = (_e211.yz * _e213);
        yuvd[1u] = _e214.x;
        yuvd[2u] = _e214.y;
        let _e220 = params.DLINE;
        let _e221 = (_e220 == 2f);
        phi_296_ = _e221;
        if !(_e221) {
            let _e224 = params.DLINE;
            phi_296_ = (_e224 == 4f);
        }
        let _e227 = phi_296_;
        phi_304_ = _e227;
        if !(_e227) {
            let _e230 = params.DLINE;
            phi_304_ = (_e230 == 6f);
        }
        let _e233 = phi_304_;
        if _e233 {
            let _e234 = yuv;
            let _e235 = yuvd;
            yuv = mix(_e234, _e235, vec3(0.5f));
        } else {
            let _e239 = yuv[0u];
            let _e241 = yuvd[0u];
            yuv[0u] = mix(_e239, _e241, 0.5f);
        }
    }
    let _e245 = params.PAL_FILT;
    if (_e245 == 4f) {
        let _e247 = res4_;
        yuv = _e247;
    }
    let _e248 = yuv;
    let _e249 = lum;
    FragColor = vec4<f32>(_e248.x, _e248.y, _e248.z, _e249);
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
