diagnostic(off, derivative_uniformity);
struct Push {
    OutputSize: vec4<f32>,
    OriginalSize: vec4<f32>,
    SourceSize: vec4<f32>,
    FrameCount: u32,
    PAL_FILT: f32,
    pal_scale: f32,
    pal_ring: f32,
    pal_res: f32,
    GCORR: f32,
    RFNOISE: f32,
    RFNOISE1_: f32,
    RFNOISE2_: f32,
    CHROMAF: f32,
}

struct UBO {
    MVP: mat4x4<f32>,
}

var<private> RFNOISE: f32;
@group(0) @binding(3) 
var<uniform> params: Push;
var<private> CON_TYPE: array<f32, 4>;
var<private> luma_weight_pal_25_: array<f32, 25>;
@group(0) @binding(2) 
var Source: texture_2d<f32>;
var<private> vTexCoord_1: vec2<f32>;
var<private> FragColor: vec4<f32>;
@group(0) @binding(0) 
var<uniform> global: UBO;
@group(1) @binding(2) 
var _Source_sampler: sampler;

fn yuv2rgb_u0028_vf3_u003b(yuv: ptr<function, vec3<f32>>) -> vec3<f32> {
    let _e164 = (*yuv);
    return (_e164 * mat3x3<f32>(vec3<f32>(1f, 0f, 1.13983f), vec3<f32>(1f, -0.39465f, -0.5806f), vec3<f32>(1f, 2.03211f, 0f)));
}

fn smothstep_u0028_f1_u003b_f1_u003b_f1_u003b(e0_: ptr<function, f32>, e1_: ptr<function, f32>, x: ptr<function, f32>) -> f32 {
    let _e166 = (*x);
    let _e167 = (*e0_);
    let _e169 = (*e1_);
    let _e170 = (*e0_);
    return clamp(((_e166 - _e167) / (_e169 - _e170)), 0f, 1f);
}

fn psrnd_u0028_vf2_u003b_f1_u003b(TexCoord: ptr<function, vec2<f32>>, FrameCount: ptr<function, f32>) -> vec2<f32> {
    var p: vec3<f32>;
    var x_1: f32;
    var y: f32;

    let _e168 = (*TexCoord);
    let _e169 = (_e168 * 16758.545f);
    let _e170 = (*FrameCount);
    p = vec3<f32>(_e169.x, _e169.y, _e170);
    let _e174 = p;
    x_1 = fract((sin(dot(_e174, vec3<f32>(12.9898f, 78.233f, 3.183f))) * 43758.547f));
    let _e179 = p;
    y = fract((sin(dot(_e179, vec3<f32>(25.9796f, 14.113f, 11.271f))) * 96321.914f));
    let _e184 = x_1;
    let _e185 = y;
    return vec2<f32>(_e184, _e185);
}

fn Noise_u0028_vf2_u003b_f1_u003b(TexCoord_1: ptr<function, vec2<f32>>, FrameCount_1: ptr<function, f32>) -> vec3<f32> {
    var param: vec2<f32>;
    var param_1: f32;
    var p_1: vec3<f32>;
    var res1_: f32;
    var res2_: f32;
    var res3_: f32;
    var lns: f32;
    var cny: f32;
    var cnz: f32;
    var param_2: f32;
    var param_3: f32;
    var param_4: f32;
    var param_5: f32;
    var param_6: f32;
    var param_7: f32;
    var param_8: f32;
    var param_9: f32;
    var param_10: f32;

    let _e183 = (*TexCoord_1);
    param = _e183;
    let _e184 = (*FrameCount_1);
    param_1 = _e184;
    let _e185 = psrnd_u0028_vf2_u003b_f1_u003b((&param), (&param_1));
    (*TexCoord_1) = _e185;
    let _e186 = (*TexCoord_1);
    let _e187 = (_e186 * 758.5453f);
    let _e188 = (*FrameCount_1);
    p_1 = vec3<f32>(_e187.x, _e187.y, _e188);
    let _e192 = p_1;
    res1_ = fract((sin(dot(_e192, vec3<f32>(12.989835f, 78.23341f, 0.16453f))) * 43758.547f));
    let _e197 = p_1;
    res2_ = fract((sin(dot(_e197, vec3<f32>(39.34679f, 11.13523f, 83.15573f))) * 39459.324f));
    let _e202 = p_1;
    res3_ = fract((sin(dot(_e202, vec3<f32>(73.15691f, 52.23504f, 9.15197f))) * 60493.848f));
    let _e207 = res1_;
    let _e210 = RFNOISE;
    lns = select(0f, 1f, (abs((_e207 - 0.5f)) < (0.2f * _e210)));
    let _e214 = res2_;
    let _e217 = RFNOISE;
    cny = select(0f, 1f, (abs((_e214 - 0.5f)) < (0.5f * _e217)));
    let _e221 = res3_;
    let _e224 = RFNOISE;
    cnz = select(0f, 1f, (abs((_e221 - 0.5f)) < (0.5f * _e224)));
    let _e228 = lns;
    let _e229 = RFNOISE;
    let _e232 = RFNOISE;
    param_2 = (0.5f - (0.2f * _e229));
    param_3 = (0.5f + (0.2f * _e232));
    let _e235 = res1_;
    param_4 = _e235;
    let _e236 = smothstep_u0028_f1_u003b_f1_u003b_f1_u003b((&param_2), (&param_3), (&param_4));
    let _e239 = lns;
    lns = (((_e228 * _e236) * 0.66f) - (0.33f * _e239));
    let _e242 = cny;
    let _e243 = RFNOISE;
    let _e246 = RFNOISE;
    param_5 = (0.5f - (0.5f * _e243));
    param_6 = (0.5f + (0.5f * _e246));
    let _e249 = res2_;
    param_7 = _e249;
    let _e250 = smothstep_u0028_f1_u003b_f1_u003b_f1_u003b((&param_5), (&param_6), (&param_7));
    let _e252 = cny;
    cny = ((_e242 * _e250) - (0.5f * _e252));
    let _e255 = cnz;
    let _e256 = RFNOISE;
    let _e259 = RFNOISE;
    param_8 = (0.5f - (0.5f * _e256));
    param_9 = (0.5f + (0.5f * _e259));
    let _e262 = res3_;
    param_10 = _e262;
    let _e263 = smothstep_u0028_f1_u003b_f1_u003b_f1_u003b((&param_8), (&param_9), (&param_10));
    let _e265 = cnz;
    cnz = ((_e255 * _e263) - (0.5f * _e265));
    let _e268 = lns;
    let _e269 = cny;
    let _e270 = cnz;
    return vec3<f32>(_e268, _e269, _e270);
}

fn sample_yuv_u0028_vf3_u003b(dx: ptr<function, vec3<f32>>) -> vec3<f32> {
    let _e164 = vTexCoord_1;
    let _e165 = (*dx);
    let _e168 = ue2_border_linear(Source, _Source_sampler, (_e164 + _e165.xz));
    let _e170 = vTexCoord_1;
    let _e171 = (*dx);
    let _e174 = ue2_border_linear(Source, _Source_sampler, (_e170 - _e171.xz));
    let _e177 = vTexCoord_1;
    let _e178 = (*dx);
    let _e181 = ue2_border_linear(Source, _Source_sampler, (_e177 + _e178.yz));
    let _e183 = vTexCoord_1;
    let _e184 = (*dx);
    let _e187 = ue2_border_linear(Source, _Source_sampler, (_e183 - _e184.yz));
    let _e189 = (_e181.yz + _e187.yz);
    return vec3<f32>((_e168.x + _e174.x), _e189.x, _e189.y);
}

fn main_1() {
    var chroma_weight_pal_25_: array<f32, 25>;
    var yres: f32;
    var hres: f32;
    var local: f32;
    var cres: f32;
    var res: f32;
    var one_x: f32;
    var one_y: f32;
    var signal: vec3<f32>;
    var offset: f32;
    var i: i32;
    var wsum: vec3<f32>;
    var sums: vec3<f32>;
    var tmp: vec3<f32>;
    var dx_1: vec3<f32>;
    var mit: f32;
    var iloop: f32;
    var dx1_: vec3<f32>;
    var param_11: vec3<f32>;
    var dx_2: vec2<f32>;
    var a: f32;
    var b: f32;
    var c: f32;
    var d: f32;
    var e: f32;
    var ns: vec3<f32>;
    var param_12: vec2<f32>;
    var param_13: f32;
    var Y: f32;
    var param_14: vec3<f32>;
    var phi_360_: bool;

    let _e194 = params.RFNOISE;
    let _e196 = params.RFNOISE;
    RFNOISE = ((_e194 * _e196) + 0.00001f);
    CON_TYPE = array<f32, 4>(288f, 320f, 410f, 512f);
    luma_weight_pal_25_ = array<f32, 25>(-0.0165f, -0.020218f, -0.025f, -0.029469f, -0.0335f, -0.035062f, -0.0385f, -0.047562f, -0.062f, -0.084469f, -0.11f, -0.141719f, -0.158f, -0.144031f, -0.0825f, 0.063469f, 0.25f, 0.460156f, 0.65f, 0.749375f, 0.82f, 0.915f, 0.99f, 0.995f, 1f);
    chroma_weight_pal_25_ = array<f32, 25>(-0.0011f, -0.00165f, -0.0025f, -0.0037f, -0.0052f, -0.0069f, -0.0083f, -0.009f, -0.0081f, -0.0056f, -0.0022f, 0.0003f, 0.0023f, 0.0052f, 0.009f, 0.0236f, 0.049f, 0.0752f, 0.1521f, 0.3397f, 0.4478f, 0.676f, 0.892f, 0.984f, 1f);
    let _e200 = params.CHROMAF;
    let _e201 = (_e200 > 1.5f);
    phi_360_ = _e201;
    if _e201 {
        let _e203 = params.PAL_FILT;
        phi_360_ = (_e203 < 2.5f);
    }
    let _e206 = phi_360_;
    if _e206 {
        chroma_weight_pal_25_ = array<f32, 25>(0.00398f, 0.00082f, -0.0052f, -0.00895f, -0.00675f, 0.0012f, 0.00785f, 0.0098f, 0.0049f, -0.0038f, -0.01095f, -0.0076f, 0.0059f, 0.0163f, 0.0201f, -0.0227f, -0.0165f, 0.034f, 0.078f, 0.148f, 0.29f, 0.432f, 0.652f, 0.84f, 1f);
    }
    let _e208 = params.PAL_FILT;
    let _e212 = CON_TYPE[(i32(_e208) - 1i)];
    yres = _e212;
    let _e214 = params.pal_res;
    if (_e214 < 1.5f) {
        let _e216 = yres;
        local = _e216;
    } else {
        let _e219 = params.OriginalSize[0u];
        local = _e219;
    }
    let _e220 = local;
    hres = _e220;
    let _e221 = yres;
    cres = (_e221 / 320f);
    let _e224 = params.pal_scale;
    res = _e224;
    let _e225 = hres;
    let _e227 = res;
    one_x = ((0.125f / _e225) / _e227);
    let _e229 = hres;
    let _e231 = res;
    let _e233 = cres;
    one_y = (((0.25f / _e229) / _e231) / _e233);
    signal = vec3<f32>(0f, 0f, 0f);
    offset = 0f;
    i = 0i;
    wsum = vec3<f32>(0f, 0f, 0f);
    let _e235 = wsum;
    sums = _e235;
    let _e236 = wsum;
    tmp = _e236;
    let _e237 = one_x;
    let _e238 = one_y;
    dx_1 = vec3<f32>(_e237, _e238, 0f);
    mit = 1f;
    iloop = 24f;
    let _e240 = dx_1;
    dx1_ = _e240;
    i = 0i;
    loop {
        let _e241 = i;
        if (_e241 < 24i) {
            let _e243 = i;
            offset = f32(_e243);
            let _e245 = offset;
            let _e246 = iloop;
            let _e248 = dx_1;
            let _e250 = (_e248.xy * (_e245 - _e246));
            dx1_[0u] = _e250.x;
            dx1_[1u] = _e250.y;
            let _e255 = dx1_;
            param_11 = _e255;
            let _e256 = sample_yuv_u0028_vf3_u003b((&param_11));
            sums = _e256;
            let _e257 = i;
            let _e259 = luma_weight_pal_25_[_e257];
            let _e260 = i;
            let _e262 = chroma_weight_pal_25_[_e260];
            let _e263 = vec2(_e262);
            tmp = vec3<f32>(_e259, _e263.x, _e263.y);
            let _e267 = wsum;
            let _e268 = tmp;
            wsum = (_e267 + _e268);
            let _e270 = sums;
            let _e271 = tmp;
            let _e273 = signal;
            signal = (_e273 + (_e270 * _e271));
            continue;
        } else {
            break;
        }
        continuing {
            let _e275 = i;
            i = (_e275 + 1i);
        }
    }
    let _e278 = luma_weight_pal_25_[24i];
    let _e280 = chroma_weight_pal_25_[24i];
    let _e282 = chroma_weight_pal_25_[24i];
    tmp = vec3<f32>(_e278, _e280, _e282);
    let _e284 = wsum;
    let _e285 = wsum;
    let _e287 = tmp;
    wsum = ((_e284 + _e285) + _e287);
    let _e289 = vTexCoord_1;
    let _e290 = ue2_border_linear(Source, _Source_sampler, _e289);
    let _e292 = tmp;
    let _e294 = signal;
    signal = (_e294 + (_e290.xyz * _e292));
    let _e296 = signal;
    let _e297 = wsum;
    signal = (_e296 / _e297);
    let _e300 = params.pal_ring;
    if (_e300 > 0.05f) {
        let _e302 = one_x;
        let _e304 = res;
        dx_2 = vec2<f32>(((8f * _e302) / min(_e304, 1f)), 0f);
        let _e308 = vTexCoord_1;
        let _e309 = dx_2;
        let _e312 = ue2_border_linear(Source, _Source_sampler, (_e308 - (_e309 * 2f)));
        a = _e312.w;
        let _e314 = vTexCoord_1;
        let _e315 = dx_2;
        let _e317 = ue2_border_linear(Source, _Source_sampler, (_e314 - _e315));
        b = _e317.w;
        let _e319 = vTexCoord_1;
        let _e320 = dx_2;
        let _e323 = ue2_border_linear(Source, _Source_sampler, (_e319 + (_e320 * 2f)));
        c = _e323.w;
        let _e325 = vTexCoord_1;
        let _e326 = dx_2;
        let _e328 = ue2_border_linear(Source, _Source_sampler, (_e325 + _e326));
        d = _e328.w;
        let _e330 = vTexCoord_1;
        let _e331 = ue2_border_linear(Source, _Source_sampler, _e330);
        e = _e331.w;
        let _e334 = signal[0u];
        let _e336 = signal[0u];
        let _e337 = a;
        let _e338 = b;
        let _e340 = c;
        let _e341 = d;
        let _e344 = e;
        let _e346 = a;
        let _e347 = b;
        let _e349 = c;
        let _e350 = d;
        let _e353 = e;
        let _e357 = params.pal_ring;
        signal[0u] = mix(_e334, clamp(_e336, min(min(min(_e337, _e338), min(_e340, _e341)), _e344), max(max(max(_e346, _e347), max(_e349, _e350)), _e353)), _e357);
    }
    let _e361 = signal[0u];
    let _e364 = params.GCORR;
    signal[0u] = pow(max(_e361, 0f), (1f / _e364));
    let _e369 = params.RFNOISE1_;
    let _e371 = params.RFNOISE2_;
    if ((_e369 + _e371) > 0.005f) {
        let _e375 = params.FrameCount;
        let _e377 = vTexCoord_1;
        param_12 = _e377;
        param_13 = f32(_e375);
        let _e378 = Noise_u0028_vf2_u003b_f1_u003b((&param_12), (&param_13));
        ns = _e378;
        let _e380 = signal[0u];
        Y = mix(0.375f, 1f, pow(_e380, 0.2f));
        let _e384 = signal[0u];
        let _e386 = params.RFNOISE1_;
        let _e388 = ns[0u];
        signal[0u] = clamp((_e384 + (_e386 * _e388)), 0f, 1f);
        let _e394 = signal[1u];
        let _e395 = Y;
        let _e397 = params.RFNOISE2_;
        let _e400 = signal[1u];
        let _e406 = ns[1u];
        signal[1u] = clamp((_e394 + (((_e395 * _e397) * (0.325f + (1.325f * abs(_e400)))) * _e406)), -0.45f, 0.45f);
        let _e412 = signal[2u];
        let _e413 = Y;
        let _e415 = params.RFNOISE2_;
        let _e418 = signal[2u];
        let _e424 = ns[2u];
        signal[2u] = clamp((_e412 + (((_e413 * _e415) * (0.325f + (1.325f * abs(_e418)))) * _e424)), -0.62f, 0.62f);
    }
    let _e429 = signal;
    param_14 = _e429;
    let _e430 = yuv2rgb_u0028_vf3_u003b((&param_14));
    let _e433 = clamp(_e430, vec3(0f), vec3(1f));
    FragColor = vec4<f32>(_e433.x, _e433.y, _e433.z, 1f);
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
