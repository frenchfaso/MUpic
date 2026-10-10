diagnostic(off, derivative_uniformity);
struct Push {
    OriginalSize: vec4<f32>,
    OutputSize: vec4<f32>,
    FrameCount: u32,
    inter: f32,
    interm: f32,
    iscan: f32,
    intres: f32,
    iscans: f32,
    hiscan: f32,
}

struct UBO {
    SourceSize: vec4<f32>,
    MVP: mat4x4<f32>,
}

@group(0) @binding(2) 
var Source: texture_2d<f32>;
var<private> vTexCoord_1: vec2<f32>;
@group(0) @binding(3) 
var<uniform> params: Push;
var<private> FragColor: vec4<f32>;
@group(0) @binding(0) 
var<uniform> global: UBO;
@group(1) @binding(2) 
var _Source_sampler: sampler;

fn plant_u0028_vf3_u003b_f1_u003b(tar: ptr<function, vec3<f32>>, r: ptr<function, f32>) -> vec3<f32> {
    var t: f32;

    let _e38 = (*tar)[0u];
    let _e40 = (*tar)[1u];
    let _e43 = (*tar)[2u];
    t = (max(max(_e38, _e40), _e43) + 0.00001f);
    let _e46 = (*tar);
    let _e47 = (*r);
    let _e49 = t;
    return ((_e46 * _e47) / vec3(_e49));
}

fn main_1() {
    var c1_: vec3<f32>;
    var c2_: vec3<f32>;
    var c: vec3<f32>;
    var intera: f32;
    var m1_: f32;
    var m2_: f32;
    var df: vec3<f32>;
    var d: f32;
    var r_1: f32;
    var yres_div: f32;
    var hscan: bool;
    var line_no: f32;
    var frame_no: f32;
    var ii: f32;
    var param: vec3<f32>;
    var param_1: f32;
    var param_2: vec3<f32>;
    var param_3: f32;
    var param_4: vec3<f32>;
    var param_5: f32;
    var param_6: vec3<f32>;
    var param_7: f32;
    var phi_161_: bool;
    var phi_167_: bool;
    var phi_173_: bool;
    var phi_212_: bool;

    let _e56 = vTexCoord_1;
    let _e57 = ue2_border_linear(Source, _Source_sampler, _e56);
    c1_ = _e57.xyz;
    let _e59 = vTexCoord_1;
    let _e62 = params.OriginalSize[3u];
    let _e65 = ue2_border_linear(Source, _Source_sampler, (_e59 + vec2<f32>(0f, _e62)));
    c2_ = _e65.xyz;
    let _e67 = c1_;
    c = _e67;
    intera = 1f;
    let _e69 = c1_[0u];
    let _e71 = c1_[1u];
    let _e74 = c1_[2u];
    m1_ = max(max(_e69, _e71), _e74);
    let _e77 = c2_[0u];
    let _e79 = c2_[1u];
    let _e82 = c2_[2u];
    m2_ = max(max(_e77, _e79), _e82);
    let _e84 = c1_;
    let _e85 = c2_;
    df = abs((_e84 - _e85));
    let _e89 = df[0u];
    let _e91 = df[1u];
    let _e94 = df[2u];
    d = max(max(_e89, _e91), _e94);
    let _e97 = params.interm;
    if (_e97 == 2f) {
        let _e99 = d;
        let _e101 = d;
        let _e103 = m1_;
        let _e104 = m2_;
        let _e107 = m2_;
        let _e108 = m1_;
        d = mix((0.1f * _e99), (10f * _e101), step((_e103 / (_e104 + 0.0001f)), (_e107 / (_e108 + 0.0001f))));
    }
    let _e113 = m1_;
    r_1 = _e113;
    yres_div = 1f;
    let _e115 = params.intres;
    if (_e115 > 1.25f) {
        let _e118 = params.intres;
        yres_div = _e118;
    }
    let _e120 = params.hiscan;
    hscan = (_e120 > 0.5f);
    let _e123 = params.inter;
    let _e126 = params.OriginalSize[1u];
    let _e127 = yres_div;
    let _e129 = (_e123 <= (_e126 / _e127));
    phi_161_ = _e129;
    if _e129 {
        let _e131 = params.interm;
        phi_161_ = (_e131 > 0.5f);
    }
    let _e134 = phi_161_;
    phi_167_ = _e134;
    if _e134 {
        let _e136 = params.intres;
        phi_167_ = (_e136 != 1f);
    }
    let _e139 = phi_167_;
    phi_173_ = _e139;
    if _e139 {
        let _e141 = params.intres;
        phi_173_ = (_e141 != 0.5f);
    }
    let _e144 = phi_173_;
    let _e145 = hscan;
    if (_e144 || _e145) {
        intera = 0.25f;
        let _e149 = params.OriginalSize[1u];
        let _e151 = vTexCoord_1[1u];
        let _e152 = (_e149 * _e151);
        line_no = floor((_e152 - (floor((_e152 / 2f)) * 2f)));
        let _e159 = params.FrameCount;
        let _e160 = f32(_e159);
        frame_no = floor((_e160 - (floor((_e160 / 2f)) * 2f)));
        let _e166 = line_no;
        let _e167 = frame_no;
        ii = abs((_e166 - _e167));
        let _e171 = params.interm;
        let _e172 = (_e171 < 3.5f);
        phi_212_ = _e172;
        if !(_e172) {
            let _e175 = params.interm;
            phi_212_ = (_e175 > 5.5f);
        }
        let _e178 = phi_212_;
        if _e178 {
            let _e180 = params.interm;
            if (_e180 == 6f) {
                let _e182 = c2_;
                let _e183 = c1_;
                let _e184 = ii;
                c = mix(_e182, _e183, vec3(_e184));
            } else {
                let _e187 = c2_;
                let _e188 = c2_;
                let _e189 = c2_;
                let _e192 = params.iscans;
                let _e196 = c2_[0u];
                let _e198 = c2_[1u];
                let _e201 = c2_[2u];
                param = mix(_e187, (_e188 * _e189), vec3(_e192));
                param_1 = max(max(_e196, _e198), _e201);
                let _e203 = plant_u0028_vf3_u003b_f1_u003b((&param), (&param_1));
                c2_ = _e203;
                let _e204 = m1_;
                let _e205 = ii;
                let _e208 = params.iscan;
                let _e210 = m1_;
                let _e211 = m2_;
                r_1 = max((_e204 * _e205), ((1f - _e208) * min(_e210, _e211)));
                let _e215 = c1_;
                let _e216 = c2_;
                let _e217 = m1_;
                let _e218 = m2_;
                let _e220 = m1_;
                let _e221 = m1_;
                let _e225 = d;
                let _e231 = c1_;
                let _e232 = ii;
                param_2 = mix(mix(_e215, _e216, vec3(min((mix(_e217, (1f - _e218), min(_e220, (1f - _e221))) / (_e225 + 0.00001f)), 1f))), _e231, vec3(_e232));
                let _e235 = r_1;
                param_3 = _e235;
                let _e236 = plant_u0028_vf3_u003b_f1_u003b((&param_2), (&param_3));
                c = _e236;
                let _e238 = params.interm;
                if (_e238 == 3f) {
                    let _e241 = params.iscan;
                    let _e244 = c2_;
                    let _e245 = c1_;
                    let _e246 = ii;
                    c = (mix(_e244, _e245, vec3(_e246)) * (1f - (0.5f * _e241)));
                }
            }
        }
        let _e251 = params.interm;
        if (_e251 == 4f) {
            let _e253 = c;
            let _e254 = c;
            let _e255 = c;
            let _e258 = params.iscans;
            let _e263 = c[0u];
            let _e265 = c[1u];
            let _e268 = c[2u];
            param_4 = mix(_e253, (_e254 * _e255), vec3((0.5f * _e258)));
            param_5 = max(max(_e263, _e265), _e268);
            let _e270 = plant_u0028_vf3_u003b_f1_u003b((&param_4), (&param_5));
            let _e272 = params.iscan;
            c = (_e270 * (1f - (0.5f * _e272)));
        }
        let _e277 = params.interm;
        if (_e277 == 5f) {
            let _e279 = c2_;
            let _e280 = c1_;
            c = mix(_e279, _e280, vec3(0.5f));
            let _e283 = c;
            let _e284 = c;
            let _e285 = c;
            let _e288 = params.iscans;
            let _e293 = c[0u];
            let _e295 = c[1u];
            let _e298 = c[2u];
            param_6 = mix(_e283, (_e284 * _e285), vec3((0.5f * _e288)));
            param_7 = max(max(_e293, _e295), _e298);
            let _e300 = plant_u0028_vf3_u003b_f1_u003b((&param_6), (&param_7));
            let _e302 = params.iscan;
            c = (_e300 * (1f - (0.5f * _e302)));
        }
        let _e306 = hscan;
        if _e306 {
            let _e307 = c1_;
            c = _e307;
        }
    }
    let _e308 = c;
    let _e309 = intera;
    FragColor = vec4<f32>(_e308.x, _e308.y, _e308.z, _e309);
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
