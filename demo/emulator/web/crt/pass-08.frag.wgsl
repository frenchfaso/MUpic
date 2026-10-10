diagnostic(off, derivative_uniformity);
struct Push {
    SourceSize: vec4<f32>,
    OriginalSize: vec4<f32>,
    OutputSize: vec4<f32>,
    FrameCount: u32,
    CSHARPEN: f32,
    CCONTR: f32,
    CDETAILS: f32,
    NDEBLUR: f32,
    DEDGE: f32,
    DESHARP: f32,
}

struct UBO {
    MVP: mat4x4<f32>,
}

@group(0) @binding(2) 
var Source: texture_2d<f32>;
var<private> vTexCoord_1: vec2<f32>;
@group(0) @binding(4) 
var<uniform> params: Push;
@group(0) @binding(3) 
var PrePass0_: texture_2d<f32>;
var<private> FragColor: vec4<f32>;
@group(0) @binding(0) 
var<uniform> global: UBO;
@group(1) @binding(2) 
var _Source_sampler: sampler;
@group(1) @binding(3) 
var _PrePass0_sampler: sampler;

fn yuv2rgb_u0028_vf3_u003b(yuv: ptr<function, vec3<f32>>) -> vec3<f32> {
    let _e53 = (*yuv);
    return (_e53 * mat3x3<f32>(vec3<f32>(1f, 0f, 1.13983f), vec3<f32>(1f, -0.39465f, -0.5806f), vec3<f32>(1f, 2.03211f, 0f)));
}

fn rgb2yuv_u0028_vf3_u003b(rgb: ptr<function, vec3<f32>>) -> vec3<f32> {
    let _e53 = (*rgb);
    return (_e53 * mat3x3<f32>(vec3<f32>(0.299f, 0.587f, 0.114f), vec3<f32>(-0.14713f, -0.28886f, 0.436f), vec3<f32>(0.615f, -0.51499f, -0.10001f)));
}

fn get_luma_u0028_vf3_u003b(c: ptr<function, vec3<f32>>) -> f32 {
    let _e53 = (*c);
    return dot(_e53, vec3<f32>(0.299f, 0.587f, 0.114f));
}

fn main_1() {
    var g01_: vec2<f32>;
    var g21_: vec2<f32>;
    var c01_: vec3<f32>;
    var c21_: vec3<f32>;
    var c11_: vec3<f32>;
    var b11_: vec3<f32>;
    var contrast: f32;
    var mn: vec3<f32>;
    var mn1_: vec3<f32>;
    var mx: vec3<f32>;
    var mx1_: vec3<f32>;
    var dif: vec3<f32>;
    var sharpen: vec3<f32>;
    var res: vec3<f32>;
    var tex0_: vec2<f32>;
    var l01_: f32;
    var param: vec3<f32>;
    var l21_: f32;
    var param_1: vec3<f32>;
    var l11_: f32;
    var param_2: vec3<f32>;
    var dth: f32;
    var DEB: f32;
    var v11_: f32;
    var lmin: f32;
    var param_3: vec3<f32>;
    var param_4: vec3<f32>;
    var param_5: vec3<f32>;
    var lmax: f32;
    var param_6: vec3<f32>;
    var param_7: vec3<f32>;
    var param_8: vec3<f32>;
    var ln1_: f32;
    var lx1_: f32;
    var r11_: f32;
    var param_9: vec3<f32>;
    var dif1_: f32;
    var dif2_: f32;
    var ratio: f32;
    var lsharpen: f32;
    var param_10: vec3<f32>;
    var param_11: vec3<f32>;

    g01_ = vec2<f32>(-0.0015625f, 0f);
    g21_ = vec2<f32>(0.0015625f, 0f);
    let _e94 = vTexCoord_1;
    let _e95 = g01_;
    let _e97 = ue2_border_linear(Source, _Source_sampler, (_e94 + _e95));
    c01_ = _e97.xyz;
    let _e99 = vTexCoord_1;
    let _e100 = g21_;
    let _e102 = ue2_border_linear(Source, _Source_sampler, (_e99 + _e100));
    c21_ = _e102.xyz;
    let _e104 = vTexCoord_1;
    let _e105 = ue2_border_linear(Source, _Source_sampler, _e104);
    c11_ = _e105.xyz;
    let _e107 = c01_;
    let _e108 = c21_;
    b11_ = ((_e107 + _e108) * 0.5f);
    let _e112 = c11_[0u];
    let _e114 = c11_[1u];
    let _e117 = c11_[2u];
    contrast = max(max(_e112, _e114), _e117);
    let _e120 = params.CCONTR;
    let _e123 = params.CCONTR;
    let _e124 = contrast;
    contrast = mix((2f * _e120), _e123, _e124);
    let _e126 = c01_;
    let _e127 = c21_;
    let _e129 = c11_;
    mn = min(min(_e126, _e127), _e129);
    let _e131 = mn;
    let _e132 = c11_;
    let _e133 = contrast;
    mn1_ = min(_e131, (_e132 * (1f - _e133)));
    let _e137 = c01_;
    let _e138 = c21_;
    let _e140 = c11_;
    mx = max(max(_e137, _e138), _e140);
    let _e142 = mx;
    let _e143 = c11_;
    let _e144 = contrast;
    mx1_ = max(_e142, (_e143 * (1f + _e144)));
    let _e148 = mx1_;
    let _e149 = mn1_;
    dif = pow(((_e148 - _e149) + vec3(0.00001f)), vec3<f32>(0.75f, 0.75f, 0.75f));
    let _e155 = params.CSHARPEN;
    let _e157 = params.CDETAILS;
    let _e161 = params.CSHARPEN;
    let _e163 = dif;
    sharpen = mix(vec3((_e155 * _e157)), vec3(_e161), _e163);
    let _e165 = c11_;
    let _e166 = b11_;
    let _e167 = sharpen;
    let _e170 = mn1_;
    let _e171 = mx1_;
    res = clamp(mix(_e165, _e166, -(_e167)), _e170, _e171);
    let _e174 = params.NDEBLUR;
    if (_e174 > 1.125f) {
        let _e177 = params.OriginalSize;
        let _e179 = vTexCoord_1;
        let _e185 = params.OriginalSize;
        tex0_ = ((floor((_e177.xy * _e179)) + vec2(0.5f)) * _e185.zw);
        let _e188 = vTexCoord_1;
        let _e189 = g01_;
        let _e192 = ue2_border_linear(PrePass0_, _PrePass0_sampler, (_e188 + (_e189 * 2f)));
        param = _e192.xyz;
        let _e194 = get_luma_u0028_vf3_u003b((&param));
        l01_ = _e194;
        let _e195 = vTexCoord_1;
        let _e196 = g21_;
        let _e199 = ue2_border_linear(PrePass0_, _PrePass0_sampler, (_e195 + (_e196 * 2f)));
        param_1 = _e199.xyz;
        let _e201 = get_luma_u0028_vf3_u003b((&param_1));
        l21_ = _e201;
        let _e202 = tex0_;
        let _e203 = ue2_border_linear(PrePass0_, _PrePass0_sampler, _e202);
        param_2 = _e203.xyz;
        let _e205 = get_luma_u0028_vf3_u003b((&param_2));
        l11_ = _e205;
        let _e206 = vTexCoord_1;
        let _e207 = ue2_border_linear(Source, _Source_sampler, _e206);
        dth = _e207.w;
        let _e210 = params.NDEBLUR;
        DEB = _e210;
        let _e211 = l01_;
        let _e212 = l21_;
        let _e214 = l11_;
        v11_ = min(min(_e211, _e212), _e214);
        let _e216 = l01_;
        let _e217 = l21_;
        let _e219 = l11_;
        l11_ = max(max(_e216, _e217), _e219);
        let _e221 = c01_;
        param_3 = _e221;
        let _e222 = get_luma_u0028_vf3_u003b((&param_3));
        let _e223 = c21_;
        param_4 = _e223;
        let _e224 = get_luma_u0028_vf3_u003b((&param_4));
        let _e226 = c11_;
        param_5 = _e226;
        let _e227 = get_luma_u0028_vf3_u003b((&param_5));
        lmin = min(min(_e222, _e224), _e227);
        let _e229 = c01_;
        param_6 = _e229;
        let _e230 = get_luma_u0028_vf3_u003b((&param_6));
        let _e231 = c21_;
        param_7 = _e231;
        let _e232 = get_luma_u0028_vf3_u003b((&param_7));
        let _e234 = c11_;
        param_8 = _e234;
        let _e235 = get_luma_u0028_vf3_u003b((&param_8));
        lmax = max(max(_e230, _e232), _e235);
        let _e237 = v11_;
        let _e238 = lmin;
        let _e239 = lmin;
        let _e241 = lmin;
        ln1_ = min(mix(_e237, _e238, _e239), _e241);
        let _e243 = lmax;
        let _e244 = l11_;
        let _e245 = lmax;
        let _e247 = lmax;
        lx1_ = max(mix(_e243, _e244, _e245), _e247);
        let _e249 = res;
        param_9 = _e249;
        let _e250 = get_luma_u0028_vf3_u003b((&param_9));
        r11_ = _e250;
        let _e251 = r11_;
        let _e252 = ln1_;
        dif1_ = (max((_e251 - _e252), 0f) + 0.00001f);
        let _e256 = dif1_;
        let _e257 = DEB;
        dif1_ = pow(_e256, _e257);
        let _e259 = lx1_;
        let _e260 = r11_;
        dif2_ = (max((_e259 - _e260), 0f) + 0.00001f);
        let _e264 = dif2_;
        let _e265 = DEB;
        dif2_ = pow(_e264, _e265);
        let _e267 = dif1_;
        let _e268 = dif1_;
        let _e269 = dif2_;
        ratio = (_e267 / (_e268 + _e269));
        let _e272 = ln1_;
        let _e273 = lx1_;
        let _e274 = ratio;
        lsharpen = mix(_e272, _e273, _e274);
        let _e276 = lsharpen;
        let _e278 = params.DEDGE;
        let _e279 = lsharpen;
        let _e281 = r11_;
        lsharpen = min(_e276, max((_e278 * _e279), _e281));
        let _e284 = res;
        param_10 = _e284;
        let _e285 = rgb2yuv_u0028_vf3_u003b((&param_10));
        res = _e285;
        let _e287 = res[0u];
        v11_ = _e287;
        let _e288 = lsharpen;
        res[0u] = _e288;
        let _e291 = params.DESHARP;
        let _e294 = res[0u];
        let _e297 = params.DESHARP;
        let _e298 = v11_;
        let _e301 = ln1_;
        let _e302 = contrast;
        let _e305 = lx1_;
        let _e306 = contrast;
        res[0u] = clamp((((1f + _e291) * _e294) - (_e297 * _e298)), (_e301 * (1f - _e302)), (_e305 * (1f + _e306)));
        let _e311 = res;
        param_11 = _e311;
        let _e312 = yuv2rgb_u0028_vf3_u003b((&param_11));
        res = max(_e312, vec3(0f));
    }
    let _e315 = res;
    FragColor = vec4<f32>(_e315.x, _e315.y, _e315.z, 1f);
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
