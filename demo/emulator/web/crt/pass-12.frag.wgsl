diagnostic(off, derivative_uniformity);
struct Push {
    SourceSize: vec4<f32>,
    OriginalSize: vec4<f32>,
    OutputSize: vec4<f32>,
    FrameCount: u32,
    SIGMA_HOR: f32,
    HSHARPNESS: f32,
    S_SHARP: f32,
    HARNG: f32,
    HSHARP: f32,
    spike: f32,
    filter_res: f32,
    MAXS: f32,
    auto_res: f32,
}

struct UBO {
    MVP: mat4x4<f32>,
}

var<private> auto_rez: f32;
@group(0) @binding(3) 
var<uniform> params: Push;
@group(0) @binding(2) 
var LinearizePass: texture_2d<f32>;
var<private> vTexCoord_1: vec2<f32>;
var<private> FragColor: vec4<f32>;
@group(0) @binding(0) 
var<uniform> global: UBO;
@group(1) @binding(2) 
var _LinearizePass_sampler: sampler;

fn gaussian_u0028_f1_u003b_f1_u003b(x: ptr<function, f32>, xscale: ptr<function, f32>) -> f32 {
    let _e38 = (*x);
    let _e40 = (*x);
    let _e42 = (*xscale);
    return exp(((-(_e38) * _e40) * _e42));
}

fn main_1() {
    var local: f32;
    var prescalex: vec2<f32>;
    var SourceSize: vec4<f32>;
    var f: f32;
    var tex: vec2<f32>;
    var color: vec3<f32>;
    var scolor: f32;
    var dx: vec2<f32>;
    var w: f32;
    var swsum: f32;
    var wsum: f32;
    var cmax: vec3<f32>;
    var cmin: vec3<f32>;
    var xscale_1: f32;
    var hsharpness: f32;
    var sharp: f32;
    var param: f32;
    var param_1: f32;
    var maxsharp: f32;
    var FPR: f32;
    var FPRi: f32;
    var fpx: f32;
    var sp: f32;
    var sw: f32;
    var ts: f32;
    var luma: vec3<f32>;
    var LOOPSIZE: f32;
    var n: f32;
    var pixel: vec3<f32>;
    var param_2: f32;
    var param_3: f32;

    let _e69 = params.OriginalSize[1u];
    if (_e69 < 375f) {
        let _e72 = params.auto_res;
        let _e75 = params.OriginalSize[0u];
        local = mix(1f, 1.85f, clamp(((_e72 * round((_e75 / 300f))) - 1f), 0f, 1f));
    } else {
        local = 1f;
    }
    let _e82 = local;
    auto_rez = _e82;
    let _e83 = textureDimensions(LinearizePass, 0i);
    let _e87 = params.OriginalSize;
    prescalex = (vec2<f32>(vec2<i32>(_e83)) / _e87.xy);
    let _e91 = params.OriginalSize;
    let _e93 = prescalex[0u];
    let _e95 = prescalex[1u];
    let _e97 = prescalex[0u];
    let _e100 = prescalex[1u];
    SourceSize = (_e91 * vec4<f32>(_e93, _e95, (1f / _e97), (1f / _e100)));
    let _e105 = SourceSize[0u];
    let _e107 = vTexCoord_1[0u];
    f = fract((_e105 * _e107));
    let _e110 = f;
    f = (0.5f - _e110);
    let _e112 = SourceSize;
    let _e114 = vTexCoord_1;
    let _e117 = SourceSize;
    let _e120 = SourceSize;
    tex = ((floor((_e112.xy * _e114)) * _e117.zw) + (_e120.zw * 0.5f));
    color = vec3<f32>(0f, 0f, 0f);
    scolor = 0f;
    let _e125 = SourceSize[2u];
    dx = vec2<f32>(_e125, 0f);
    w = 0f;
    swsum = 0f;
    wsum = 0f;
    cmax = vec3<f32>(0f, 0f, 0f);
    cmin = vec3<f32>(1f, 1f, 1f);
    let _e128 = prescalex[0u];
    let _e131 = auto_rez;
    xscale_1 = (max((_e128 * 0.4f), 1f) * _e131);
    let _e134 = params.HSHARPNESS;
    let _e135 = xscale_1;
    let _e138 = params.filter_res;
    hsharpness = ((_e134 * _e135) * _e138);
    let _e141 = params.SIGMA_HOR;
    let _e144 = params.SIGMA_HOR;
    let _e146 = xscale_1;
    let _e148 = xscale_1;
    let _e151 = params.filter_res;
    let _e154 = params.filter_res;
    xscale_1 = (1f / ((((((2f * _e141) * _e144) * _e146) * _e148) * _e151) * _e154));
    let _e157 = hsharpness;
    param = _e157;
    let _e158 = xscale_1;
    param_1 = _e158;
    let _e159 = gaussian_u0028_f1_u003b_f1_u003b((&param), (&param_1));
    let _e161 = params.S_SHARP;
    sharp = (_e159 * _e161);
    let _e164 = params.MAXS;
    maxsharp = _e164;
    let _e165 = hsharpness;
    FPR = _e165;
    let _e166 = hsharpness;
    FPRi = (1f / _e166);
    fpx = 0f;
    sp = 0f;
    sw = 0f;
    ts = 0.025f;
    luma = vec3<f32>(0.2126f, 0.7152f, 0.0722f);
    let _e168 = FPR;
    LOOPSIZE = ceil((2f * _e168));
    let _e171 = LOOPSIZE;
    n = -(_e171);
    loop {
        let _e173 = tex;
        let _e174 = n;
        let _e175 = dx;
        let _e178 = ue2_border_linear(LinearizePass, _LinearizePass_sampler, (_e173 + (_e175 * _e174)));
        pixel = _e178.xyz;
        let _e180 = n;
        let _e181 = f;
        param_2 = (_e180 + _e181);
        let _e183 = xscale_1;
        param_3 = _e183;
        let _e184 = gaussian_u0028_f1_u003b_f1_u003b((&param_2), (&param_3));
        let _e185 = sharp;
        w = (_e184 - _e185);
        let _e187 = n;
        let _e188 = f;
        let _e191 = FPR;
        let _e193 = FPRi;
        fpx = ((abs((_e187 + _e188)) - _e191) * _e193);
        let _e195 = w;
        if (_e195 < 0f) {
            let _e197 = w;
            let _e198 = maxsharp;
            let _e200 = fpx;
            let _e203 = params.HSHARP;
            w = max(_e197, mix(-(_e198), 0f, pow(clamp(_e200, 0f, 1f), _e203)));
        } else {
            let _e207 = cmax;
            let _e208 = pixel;
            cmax = max(_e207, _e208);
            let _e210 = cmin;
            let _e211 = pixel;
            cmin = min(_e210, _e211);
            let _e213 = w;
            let _e214 = pixel;
            let _e215 = luma;
            let _e217 = ts;
            sw = (_e213 * (dot(_e214, _e215) + _e217));
            let _e221 = pixel[0u];
            let _e223 = pixel[1u];
            let _e226 = pixel[2u];
            sp = max(max(_e221, _e223), _e226);
            let _e228 = scolor;
            let _e229 = sw;
            let _e230 = sp;
            scolor = (_e228 + (_e229 * _e230));
            let _e233 = swsum;
            let _e234 = sw;
            swsum = (_e233 + _e234);
        }
        let _e236 = color;
        let _e237 = w;
        let _e238 = pixel;
        color = (_e236 + (_e238 * _e237));
        let _e241 = wsum;
        let _e242 = w;
        wsum = (_e241 + _e242);
        let _e244 = n;
        n = (_e244 + 1f);
        continue;
        continuing {
            let _e246 = n;
            let _e247 = LOOPSIZE;
            break if !((_e246 <= _e247));
        }
    }
    let _e250 = color;
    let _e251 = wsum;
    color = (_e250 / vec3(_e251));
    let _e254 = scolor;
    let _e255 = swsum;
    scolor = (_e254 / _e255);
    let _e257 = color;
    let _e258 = cmin;
    let _e259 = cmax;
    let _e261 = color;
    let _e263 = params.HARNG;
    color = clamp(mix(clamp(_e257, _e258, _e259), _e261, vec3(_e263)), vec3(0f), vec3(1f));
    let _e270 = color[0u];
    let _e272 = color[1u];
    let _e275 = color[2u];
    let _e277 = scolor;
    let _e279 = params.spike;
    scolor = clamp(mix(max(max(_e270, _e272), _e275), _e277, _e279), 0f, 1f);
    let _e282 = color;
    let _e283 = scolor;
    FragColor = vec4<f32>(_e282.x, _e282.y, _e282.z, _e283);
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
