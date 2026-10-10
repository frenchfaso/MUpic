diagnostic(off, derivative_uniformity);
struct Push {
    IOS: f32,
    OS: f32,
    BLOOM: f32,
    brightboost: f32,
    brightboost1_: f32,
    gsl: f32,
    scanline1_: f32,
    scanline2_: f32,
    beam_min: f32,
    beam_max: f32,
    beam_size: f32,
    h_sharp: f32,
    s_sharp: f32,
    warpX: f32,
    warpY: f32,
    glow: f32,
    shadowMask: f32,
    masksize: f32,
    ring: f32,
    no_scanlines: f32,
    tds: f32,
    clips: f32,
    hiscan: f32,
    rolling_scan: f32,
}

struct UBO {
    MVP: mat4x4<f32>,
    SourceSize: vec4<f32>,
    OriginalSize: vec4<f32>,
    OutputSize: vec4<f32>,
    FrameCount: u32,
    bloom: f32,
    halation: f32,
    scans: f32,
    gamma_c: f32,
    gamma_c2_: f32,
    gamma_out: f32,
    overscanX: f32,
    overscanY: f32,
    VShift: f32,
    intres: f32,
    prescalex: f32,
    c_shape: f32,
    scangamma: f32,
    sborder: f32,
    scan_falloff: f32,
    bloom_dist: f32,
    bmask1_: f32,
    hmask1_: f32,
    interm: f32,
}

@group(0) @binding(5) 
var<uniform> params: Push;
@group(0) @binding(0) 
var<uniform> global: UBO;
@group(0) @binding(2) 
var LinearizePass: texture_2d<f32>;
@group(0) @binding(3) 
var AvgLumPass: texture_2d<f32>;
var<private> vTexCoord_1: vec2<f32>;
@group(0) @binding(4) 
var Pass1_: texture_2d<f32>;
var<private> FragColor: vec4<f32>;
@group(1) @binding(2) 
var _LinearizePass_sampler: sampler;
@group(1) @binding(3) 
var _AvgLumPass_sampler: sampler;
@group(1) @binding(4) 
var _Pass1_sampler: sampler;

fn gc_u0028_vf3_u003b(c: ptr<function, vec3<f32>>) -> vec3<f32> {
    var mc: f32;
    var mg: f32;

    let _e74 = (*c)[0u];
    let _e76 = (*c)[1u];
    let _e79 = (*c)[2u];
    mc = max(max(_e74, _e76), _e79);
    let _e81 = mc;
    let _e83 = global.gamma_c;
    mg = pow(_e81, (1f / _e83));
    let _e86 = (*c);
    let _e87 = mg;
    let _e89 = mc;
    return ((_e86 * _e87) / vec3((_e89 + 0.00000001f)));
}

fn plant_u0028_vf3_u003b_f1_u003b(tar: ptr<function, vec3<f32>>, r: ptr<function, f32>) -> vec3<f32> {
    var t: f32;

    let _e74 = (*tar)[0u];
    let _e76 = (*tar)[1u];
    let _e79 = (*tar)[2u];
    t = (max(max(_e74, _e76), _e79) + 0.00001f);
    let _e82 = (*tar);
    let _e83 = (*r);
    let _e85 = t;
    return ((_e82 * _e83) / vec3(_e85));
}

fn sw2_u0028_f1_u003b_f1_u003b_f1_u003b_vf3_u003b(x: ptr<function, f32>, color: ptr<function, f32>, scanline: ptr<function, f32>, c_1: ptr<function, vec3<f32>>) -> vec3<f32> {
    var tmp: f32;
    var sat: vec3<f32>;
    var ex: f32;

    let _e77 = (*color);
    let _e81 = params.beam_min;
    let _e84 = params.beam_max;
    let _e85 = (*color);
    tmp = mix(((2.5f - (0.5f * _e77)) * _e81), _e84, _e85);
    let _e88 = global.scans;
    let _e92 = (*c_1);
    sat = mix((vec3<f32>(1f, 1f, 1f) + vec3((1.5f * _e88))), vec3<f32>(1f, 1f, 1f), _e92);
    let _e95 = params.beam_max;
    let _e96 = tmp;
    let _e97 = (*x);
    let _e98 = (*color);
    tmp = mix(_e95, _e96, pow(_e97, (_e98 + 0.3f)));
    let _e102 = (*x);
    let _e103 = tmp;
    ex = (_e102 * _e103);
    let _e105 = (*scanline);
    let _e107 = ex;
    let _e109 = ex;
    let _e111 = sat;
    return exp2((_e111 * ((-(_e105) * _e107) * _e109)));
}

fn sw1_u0028_f1_u003b_f1_u003b_f1_u003b_vf3_u003b(x_1: ptr<function, f32>, color_1: ptr<function, f32>, scanline_1: ptr<function, f32>, c_2: ptr<function, vec3<f32>>) -> vec3<f32> {
    var sat_1: vec3<f32>;
    var tmp_1: f32;
    var ex_1: f32;

    let _e77 = (*x_1);
    let _e79 = params.beam_min;
    let _e80 = (*x_1);
    let _e82 = (*x_1);
    let _e83 = (*color_1);
    (*x_1) = mix(_e77, (_e79 * _e80), max((_e82 - (0.4f * _e83)), 0f));
    let _e89 = global.scans;
    let _e93 = (*c_2);
    sat_1 = mix((vec3<f32>(1f, 1f, 1f) + vec3((1.5f * _e89))), vec3<f32>(1f, 1f, 1f), _e93);
    let _e96 = params.beam_min;
    let _e99 = params.beam_max;
    let _e100 = (*color_1);
    tmp_1 = mix((1.2f * _e96), _e99, _e100);
    let _e102 = (*x_1);
    let _e103 = tmp_1;
    ex_1 = (_e102 * _e103);
    let _e105 = (*scanline_1);
    let _e107 = ex_1;
    let _e109 = ex_1;
    let _e111 = sat_1;
    return exp2((_e111 * ((-(_e105) * _e107) * _e109)));
}

fn sw0_u0028_f1_u003b_f1_u003b_f1_u003b_vf3_u003b(x_2: ptr<function, f32>, color_2: ptr<function, f32>, scanline_2: ptr<function, f32>, c_3: ptr<function, vec3<f32>>) -> vec3<f32> {
    var tmp_2: f32;
    var sat_2: vec3<f32>;
    var ex_2: f32;
    var local: f32;

    let _e79 = params.beam_min;
    let _e81 = params.beam_max;
    let _e82 = (*color_2);
    tmp_2 = mix(_e79, _e81, _e82);
    let _e85 = global.scans;
    let _e89 = (*c_3);
    sat_2 = mix((vec3<f32>(1f, 1f, 1f) + vec3((1.5f * _e85))), vec3<f32>(1f, 1f, 1f), _e89);
    let _e91 = (*x_2);
    let _e92 = tmp_2;
    ex_2 = (_e91 * _e92);
    let _e95 = params.gsl;
    if (_e95 > -0.5f) {
        let _e97 = ex_2;
        let _e98 = ex_2;
        local = (_e97 * _e98);
    } else {
        let _e100 = ex_2;
        let _e101 = ex_2;
        let _e103 = ex_2;
        let _e104 = ex_2;
        let _e106 = ex_2;
        local = mix((_e100 * _e101), ((_e103 * _e104) * _e106), 0.4f);
    }
    let _e109 = local;
    ex_2 = _e109;
    let _e110 = (*scanline_2);
    let _e112 = ex_2;
    let _e114 = sat_2;
    return exp2((_e114 * (-(_e110) * _e112)));
}

fn st_u0028_f1_u003b(x_3: ptr<function, f32>) -> f32 {
    let _e71 = (*x_3);
    let _e73 = (*x_3);
    return exp2(((-10f * _e71) * _e73));
}

fn Warp_u0028_vf2_u003b(pos: ptr<function, vec2<f32>>) -> vec2<f32> {
    let _e71 = (*pos);
    (*pos) = ((_e71 * 2f) - vec2(1f));
    let _e75 = (*pos);
    let _e77 = (*pos)[0u];
    let _e79 = global.c_shape;
    let _e81 = (*pos)[1u];
    let _e84 = (*pos)[1u];
    let _e90 = (*pos)[1u];
    let _e92 = global.c_shape;
    let _e94 = (*pos)[0u];
    let _e97 = (*pos)[0u];
    let _e104 = params.warpX;
    let _e106 = params.warpY;
    let _e109 = global.c_shape;
    (*pos) = mix(_e75, vec2<f32>((_e77 * inverseSqrt((1f - ((_e79 * _e81) * _e84)))), (_e90 * inverseSqrt((1f - ((_e92 * _e94) * _e97))))), (vec2<f32>(_e104, _e106) / vec2(_e109)));
    let _e113 = (*pos);
    return ((_e113 * 0.5f) + vec2(0.5f));
}

fn Overscan_u0028_vf2_u003b_f1_u003b_f1_u003b(pos_1: ptr<function, vec2<f32>>, dx: ptr<function, f32>, dy: ptr<function, f32>) -> vec2<f32> {
    let _e73 = (*pos_1);
    (*pos_1) = ((_e73 * 2f) - vec2(1f));
    let _e77 = (*dx);
    let _e78 = (*dy);
    let _e80 = (*pos_1);
    (*pos_1) = (_e80 * vec2<f32>(_e77, _e78));
    let _e82 = (*pos_1);
    return ((_e82 * 0.5f) + vec2(0.5f));
}

fn main_1() {
    var prescalex: f32;
    var SourceSize: vec4<f32>;
    var lum: f32;
    var gamma_in: f32;
    var intera: f32;
    var hscan: bool;
    var inter6_: bool;
    var interb: bool;
    var SourceY: f32;
    var sy: f32;
    var texcoord: vec2<f32>;
    var ofactor: vec2<f32>;
    var intfactor: vec2<f32>;
    var local_1: vec2<f32>;
    var diff: vec2<f32>;
    var scan: f32;
    var param: vec2<f32>;
    var param_1: f32;
    var param_2: f32;
    var factor: f32;
    var param_3: vec2<f32>;
    var param_4: f32;
    var param_5: f32;
    var param_6: vec2<f32>;
    var param_7: f32;
    var param_8: f32;
    var pos_2: vec2<f32>;
    var param_9: vec2<f32>;
    var pos0_: vec2<f32>;
    var param_10: vec2<f32>;
    var ii: f32;
    var coffset: vec2<f32>;
    var ps: vec2<f32>;
    var OGL2Pos: vec2<f32>;
    var fp: vec2<f32>;
    var dx_1: vec2<f32>;
    var dy_1: vec2<f32>;
    var f: f32;
    var pC4_: vec2<f32>;
    var color1_: vec3<f32>;
    var scolor1_: vec3<f32>;
    var color2_: vec3<f32>;
    var scolor2_: vec3<f32>;
    var ctmp: vec3<f32>;
    var w3_: f32;
    var color_3: vec3<f32>;
    var one: vec3<f32>;
    var shape1_: f32;
    var shape2_: f32;
    var wt1_: f32;
    var param_11: f32;
    var wt2_: f32;
    var param_12: f32;
    var color00_: vec3<f32>;
    var scolor0_: vec3<f32>;
    var sctmp: vec3<f32>;
    var cref1_: vec3<f32>;
    var creff1_: f32;
    var cref2_: vec3<f32>;
    var creff2_: f32;
    var scanpix: f32;
    var f1_: f32;
    var f2_: f32;
    var mc1_: f32;
    var mc2_: f32;
    var w1_: vec3<f32>;
    var param_13: f32;
    var param_14: f32;
    var param_15: f32;
    var param_16: vec3<f32>;
    var w2_: vec3<f32>;
    var param_17: f32;
    var param_18: f32;
    var param_19: f32;
    var param_20: vec3<f32>;
    var param_21: f32;
    var param_22: f32;
    var param_23: f32;
    var param_24: vec3<f32>;
    var param_25: f32;
    var param_26: f32;
    var param_27: f32;
    var param_28: vec3<f32>;
    var param_29: f32;
    var param_30: f32;
    var param_31: f32;
    var param_32: vec3<f32>;
    var param_33: f32;
    var param_34: f32;
    var param_35: f32;
    var param_36: vec3<f32>;
    var w3_1: vec3<f32>;
    var wf1_: f32;
    var sat_3: f32;
    var param_37: vec3<f32>;
    var param_38: f32;
    var param_39: vec3<f32>;
    var param_40: f32;
    var param_41: vec3<f32>;
    var param_42: vec3<f32>;
    var param_43: vec3<f32>;
    var colmx: f32;
    var phi_402_: bool;
    var phi_417_: bool;
    var phi_443_: bool;
    var phi_517_: bool;
    var phi_680_: bool;

    let _e172 = textureDimensions(LinearizePass, 0i);
    let _e178 = global.OriginalSize[0u];
    prescalex = (f32(vec2<i32>(_e172).x) / (2f * _e178));
    let _e182 = global.OriginalSize;
    let _e183 = prescalex;
    let _e184 = prescalex;
    SourceSize = (_e182 * vec4<f32>(_e183, 1f, (1f / _e184), 1f));
    let _e188 = SourceSize;
    SourceSize = (_e188 * vec4<f32>(2f, 1f, 0.5f, 1f));
    let _e190 = ue2_border_linear(AvgLumPass, _AvgLumPass_sampler, vec2<f32>(0.5f, 0.5f));
    lum = _e190.w;
    let _e192 = ue2_border_linear(LinearizePass, _LinearizePass_sampler, vec2<f32>(0.25f, 0.25f));
    gamma_in = (1f / _e192.w);
    let _e195 = ue2_border_linear(LinearizePass, _LinearizePass_sampler, vec2<f32>(0.75f, 0.25f));
    intera = _e195.w;
    let _e198 = params.hiscan;
    hscan = (_e198 > 0.5f);
    let _e200 = intera;
    let _e201 = (_e200 < 0.35f);
    phi_402_ = _e201;
    if _e201 {
        let _e203 = global.interm;
        phi_402_ = (_e203 == 6f);
    }
    let _e206 = phi_402_;
    inter6_ = _e206;
    let _e207 = intera;
    let _e209 = inter6_;
    let _e211 = ((_e207 < 0.35f) && !(_e209));
    phi_417_ = _e211;
    if !(_e211) {
        let _e214 = params.no_scanlines;
        phi_417_ = (_e214 > 0.025f);
    }
    let _e217 = phi_417_;
    let _e218 = hscan;
    interb = (_e217 && !(_e218));
    let _e222 = SourceSize[1u];
    SourceY = _e222;
    sy = 1f;
    let _e224 = global.intres;
    if (_e224 == 1f) {
        let _e226 = SourceY;
        sy = max(round((_e226 / 224f)), 1f);
    }
    let _e231 = global.intres;
    let _e232 = (_e231 > 0.25f);
    phi_443_ = _e232;
    if _e232 {
        let _e234 = global.intres;
        phi_443_ = (_e234 != 1f);
    }
    let _e237 = phi_443_;
    if _e237 {
        let _e239 = global.intres;
        sy = _e239;
    }
    let _e240 = inter6_;
    if _e240 {
        let _e241 = sy;
        sy = (_e241 * 2f);
    }
    let _e243 = sy;
    let _e245 = sy;
    let _e247 = SourceSize;
    SourceSize = (_e247 * vec4<f32>(1f, (1f / _e243), 1f, _e245));
    let _e249 = vTexCoord_1;
    texcoord = _e249;
    let _e251 = params.IOS;
    let _e253 = interb;
    if ((_e251 > 0f) && !(_e253)) {
        let _e257 = global.OutputSize;
        let _e260 = global.OriginalSize;
        ofactor = (_e257.xy / _e260.xy);
        let _e264 = params.IOS;
        if (_e264 < 2.5f) {
            let _e266 = ofactor;
            local_1 = floor(_e266);
        } else {
            let _e268 = ofactor;
            local_1 = ceil(_e268);
        }
        let _e270 = local_1;
        intfactor = _e270;
        let _e271 = ofactor;
        let _e272 = intfactor;
        diff = (_e271 / _e272);
        let _e275 = diff[1u];
        scan = _e275;
        let _e276 = texcoord;
        param = _e276;
        let _e277 = scan;
        param_1 = _e277;
        let _e278 = scan;
        param_2 = _e278;
        let _e279 = Overscan_u0028_vf2_u003b_f1_u003b_f1_u003b((&param), (&param_1), (&param_2));
        texcoord = _e279;
        let _e281 = params.IOS;
        let _e282 = (_e281 == 1f);
        phi_517_ = _e282;
        if !(_e282) {
            let _e285 = params.IOS;
            phi_517_ = (_e285 == 3f);
        }
        let _e288 = phi_517_;
        if _e288 {
            let _e290 = vTexCoord_1[0u];
            let _e292 = texcoord[1u];
            texcoord = vec2<f32>(_e290, _e292);
        }
    }
    let _e295 = params.OS;
    let _e299 = params.BLOOM;
    let _e303 = lum;
    let _e305 = params.BLOOM;
    factor = ((1f + (((1f - (0.5f * _e295)) * _e299) / 100f)) - ((_e303 * _e305) / 100f));
    let _e309 = texcoord;
    param_3 = _e309;
    let _e310 = factor;
    param_4 = _e310;
    let _e311 = factor;
    param_5 = _e311;
    let _e312 = Overscan_u0028_vf2_u003b_f1_u003b_f1_u003b((&param_3), (&param_4), (&param_5));
    texcoord = _e312;
    let _e314 = texcoord[1u];
    let _e316 = global.VShift;
    let _e319 = global.OriginalSize[3u];
    texcoord[1u] = (_e314 - (_e316 * _e319));
    let _e325 = global.OriginalSize[0u];
    let _e327 = global.overscanX;
    let _e331 = global.OriginalSize[0u];
    let _e335 = global.OriginalSize[1u];
    let _e337 = global.overscanY;
    let _e341 = global.OriginalSize[1u];
    let _e343 = texcoord;
    param_6 = _e343;
    param_7 = ((_e325 - _e327) / _e331);
    param_8 = ((_e335 - _e337) / _e341);
    let _e344 = Overscan_u0028_vf2_u003b_f1_u003b_f1_u003b((&param_6), (&param_7), (&param_8));
    texcoord = _e344;
    let _e345 = texcoord;
    param_9 = _e345;
    let _e346 = Warp_u0028_vf2_u003b((&param_9));
    pos_2 = _e346;
    let _e347 = vTexCoord_1;
    param_10 = _e347;
    let _e348 = Warp_u0028_vf2_u003b((&param_10));
    pos0_ = _e348;
    let _e349 = inter6_;
    let _e352 = global.FrameCount;
    let _e353 = f32(_e352);
    ii = (select(0f, 1f, _e349) * floor((_e353 - (floor((_e353 / 2f)) * 2f))));
    let _e360 = ii;
    coffset = vec2<f32>(0.5f, (0.5f + (0.5f * _e360)));
    let _e364 = SourceSize;
    ps = _e364.zw;
    let _e366 = pos_2;
    let _e367 = SourceSize;
    let _e370 = coffset;
    OGL2Pos = ((_e366 * _e367.xy) - _e370);
    let _e372 = OGL2Pos;
    fp = fract(_e372);
    let _e375 = ps[0u];
    dx_1 = vec2<f32>(_e375, 0f);
    let _e378 = ps[1u];
    dy_1 = vec2<f32>(0f, _e378);
    let _e381 = fp[1u];
    f = _e381;
    let _e382 = OGL2Pos;
    let _e384 = ps;
    let _e386 = ps;
    pC4_ = ((floor(_e382) * _e384) + (_e386 * 0.5f));
    let _e390 = pC4_[1u];
    let _e391 = inter6_;
    let _e395 = dy_1[1u];
    pC4_[1u] = (_e390 + ((select(0f, 1f, _e391) * 0.25f) * _e395));
    let _e400 = pos_2[0u];
    pC4_[0u] = _e400;
    let _e403 = global.intres;
    let _e405 = prescalex;
    if ((_e403 == 0.5f) && (_e405 < 1.5f)) {
        let _e409 = pC4_[1u];
        let _e412 = global.OriginalSize[1u];
        let _e417 = global.OriginalSize[3u];
        let _e421 = global.OriginalSize[3u];
        pC4_[1u] = ((floor((_e409 * _e412)) * _e417) + (0.5f * _e421));
    }
    let _e425 = interb;
    phi_680_ = _e425;
    if _e425 {
        let _e427 = params.no_scanlines;
        phi_680_ = (_e427 < 0.025f);
    }
    let _e430 = phi_680_;
    let _e431 = hscan;
    if (_e430 || _e431) {
        let _e434 = pos_2[1u];
        pC4_[1u] = _e434;
    } else {
        let _e436 = interb;
        if _e436 {
            let _e438 = pC4_[1u];
            let _e440 = params.no_scanlines;
            let _e444 = params.no_scanlines;
            let _e447 = f;
            let _e450 = SourceSize[3u];
            pC4_[1u] = (_e438 + (smoothstep((0.4f - (0.5f * _e440)), (0.6f + (0.5f * _e444)), _e447) * _e450));
        }
    }
    let _e454 = pC4_;
    let _e455 = ue2_border_linear(Pass1_, _Pass1_sampler, _e454);
    color1_ = _e455.xyz;
    let _e457 = pC4_;
    let _e458 = ue2_border_linear(Pass1_, _Pass1_sampler, _e457);
    scolor1_ = _e458.www;
    let _e460 = interb;
    if !(_e460) {
        let _e462 = color1_;
        let _e464 = global.scangamma;
        let _e465 = gamma_in;
        color1_ = pow(_e462, vec3((_e464 / _e465)));
    }
    let _e469 = dy_1;
    let _e470 = pC4_;
    pC4_ = (_e470 + _e469);
    let _e473 = global.intres;
    let _e475 = prescalex;
    if ((_e473 == 0.5f) && (_e475 < 1.5f)) {
        let _e479 = pos_2[1u];
        let _e481 = dy_1[1u];
        let _e486 = global.OriginalSize[1u];
        let _e491 = global.OriginalSize[3u];
        let _e495 = global.OriginalSize[3u];
        pC4_[1u] = ((floor(((_e479 + (0.33f * _e481)) * _e486)) * _e491) + (0.5f * _e495));
    }
    let _e499 = pC4_;
    let _e500 = ue2_border_linear(Pass1_, _Pass1_sampler, _e499);
    color2_ = _e500.xyz;
    let _e502 = pC4_;
    let _e503 = ue2_border_linear(Pass1_, _Pass1_sampler, _e502);
    scolor2_ = _e503.www;
    let _e505 = interb;
    if !(_e505) {
        let _e507 = color2_;
        let _e509 = global.scangamma;
        let _e510 = gamma_in;
        color2_ = pow(_e507, vec3((_e509 / _e510)));
    }
    let _e514 = color1_;
    ctmp = _e514;
    w3_ = 1f;
    let _e515 = color1_;
    color_3 = _e515;
    one = vec3<f32>(1f, 1f, 1f);
    let _e516 = hscan;
    if _e516 {
        let _e517 = color1_;
        color2_ = _e517;
        let _e518 = scolor1_;
        scolor2_ = _e518;
    }
    let _e519 = interb;
    let _e521 = hscan;
    if (!(_e519) || _e521) {
        let _e524 = params.scanline1_;
        let _e526 = params.scanline2_;
        let _e527 = f;
        shape1_ = mix(_e524, _e526, _e527);
        let _e530 = params.scanline1_;
        let _e532 = params.scanline2_;
        let _e533 = f;
        shape2_ = mix(_e530, _e532, (1f - _e533));
        let _e536 = f;
        param_11 = _e536;
        let _e537 = st_u0028_f1_u003b((&param_11));
        wt1_ = _e537;
        let _e538 = f;
        param_12 = (1f - _e538);
        let _e540 = st_u0028_f1_u003b((&param_12));
        wt2_ = _e540;
        let _e541 = color1_;
        let _e542 = wt1_;
        let _e544 = color2_;
        let _e545 = wt2_;
        color00_ = ((_e541 * _e542) + (_e544 * _e545));
        let _e548 = scolor1_;
        let _e549 = wt1_;
        let _e551 = scolor2_;
        let _e552 = wt2_;
        scolor0_ = ((_e548 * _e549) + (_e551 * _e552));
        let _e555 = color00_;
        let _e556 = wt1_;
        let _e557 = wt2_;
        ctmp = (_e555 / vec3((_e556 + _e557)));
        let _e561 = scolor0_;
        let _e562 = wt1_;
        let _e563 = wt2_;
        sctmp = (_e561 / vec3((_e562 + _e563)));
        let _e568 = params.rolling_scan;
        if (abs(_e568) > 0.005f) {
            let _e571 = ctmp;
            color1_ = _e571;
            let _e572 = ctmp;
            color2_ = _e572;
            let _e573 = sctmp;
            scolor1_ = _e573;
            let _e574 = sctmp;
            scolor2_ = _e574;
        }
        let _e575 = sctmp;
        let _e576 = scolor1_;
        let _e578 = params.beam_size;
        cref1_ = mix(_e575, _e576, vec3(_e578));
        let _e582 = cref1_[0u];
        let _e584 = cref1_[1u];
        let _e587 = cref1_[2u];
        let _e590 = global.scan_falloff;
        creff1_ = pow(max(max(_e582, _e584), _e587), _e590);
        let _e592 = sctmp;
        let _e593 = scolor2_;
        let _e595 = params.beam_size;
        cref2_ = mix(_e592, _e593, vec3(_e595));
        let _e599 = cref2_[0u];
        let _e601 = cref2_[1u];
        let _e604 = cref2_[2u];
        let _e607 = global.scan_falloff;
        creff2_ = pow(max(max(_e599, _e601), _e604), _e607);
        let _e610 = params.tds;
        if (_e610 > 0.5f) {
            let _e613 = params.scanline2_;
            let _e614 = shape1_;
            let _e615 = creff1_;
            shape1_ = mix(_e613, _e614, _e615);
            let _e618 = params.scanline2_;
            let _e619 = shape2_;
            let _e620 = creff2_;
            shape2_ = mix(_e618, _e619, _e620);
        }
        let _e624 = global.OriginalSize[1u];
        let _e627 = global.OutputSize[1u];
        scanpix = (_e624 / _e627);
        let _e629 = f;
        let _e631 = params.rolling_scan;
        let _e633 = global.FrameCount;
        let _e636 = scanpix;
        f1_ = fract((_e629 - ((_e631 * f32(_e633)) * _e636)));
        let _e640 = f1_;
        f2_ = (1f - _e640);
        let _e643 = color1_[0u];
        let _e645 = color1_[1u];
        let _e648 = color1_[2u];
        mc1_ = (max(max(_e643, _e645), _e648) + 0.00000001f);
        let _e652 = color2_[0u];
        let _e654 = color2_[1u];
        let _e657 = color2_[2u];
        mc2_ = (max(max(_e652, _e654), _e657) + 0.00000001f);
        let _e660 = color1_;
        let _e661 = mc1_;
        cref1_ = (_e660 / vec3(_e661));
        let _e664 = color2_;
        let _e665 = mc2_;
        cref2_ = (_e664 / vec3(_e665));
        let _e669 = params.gsl;
        if (_e669 < 0.5f) {
            let _e671 = f1_;
            param_13 = _e671;
            let _e672 = creff1_;
            param_14 = _e672;
            let _e673 = shape1_;
            param_15 = _e673;
            let _e674 = cref1_;
            param_16 = _e674;
            let _e675 = sw0_u0028_f1_u003b_f1_u003b_f1_u003b_vf3_u003b((&param_13), (&param_14), (&param_15), (&param_16));
            w1_ = _e675;
            let _e676 = f2_;
            param_17 = _e676;
            let _e677 = creff2_;
            param_18 = _e677;
            let _e678 = shape2_;
            param_19 = _e678;
            let _e679 = cref2_;
            param_20 = _e679;
            let _e680 = sw0_u0028_f1_u003b_f1_u003b_f1_u003b_vf3_u003b((&param_17), (&param_18), (&param_19), (&param_20));
            w2_ = _e680;
        } else {
            let _e682 = params.gsl;
            if (_e682 == 1f) {
                let _e684 = f1_;
                param_21 = _e684;
                let _e685 = creff1_;
                param_22 = _e685;
                let _e686 = shape1_;
                param_23 = _e686;
                let _e687 = cref1_;
                param_24 = _e687;
                let _e688 = sw1_u0028_f1_u003b_f1_u003b_f1_u003b_vf3_u003b((&param_21), (&param_22), (&param_23), (&param_24));
                w1_ = _e688;
                let _e689 = f2_;
                param_25 = _e689;
                let _e690 = creff2_;
                param_26 = _e690;
                let _e691 = shape2_;
                param_27 = _e691;
                let _e692 = cref2_;
                param_28 = _e692;
                let _e693 = sw1_u0028_f1_u003b_f1_u003b_f1_u003b_vf3_u003b((&param_25), (&param_26), (&param_27), (&param_28));
                w2_ = _e693;
            } else {
                let _e694 = f1_;
                param_29 = _e694;
                let _e695 = creff1_;
                param_30 = _e695;
                let _e696 = shape1_;
                param_31 = _e696;
                let _e697 = cref1_;
                param_32 = _e697;
                let _e698 = sw2_u0028_f1_u003b_f1_u003b_f1_u003b_vf3_u003b((&param_29), (&param_30), (&param_31), (&param_32));
                w1_ = _e698;
                let _e699 = f2_;
                param_33 = _e699;
                let _e700 = creff2_;
                param_34 = _e700;
                let _e701 = shape2_;
                param_35 = _e701;
                let _e702 = cref2_;
                param_36 = _e702;
                let _e703 = sw2_u0028_f1_u003b_f1_u003b_f1_u003b_vf3_u003b((&param_33), (&param_34), (&param_35), (&param_36));
                w2_ = _e703;
            }
        }
        let _e704 = w1_;
        let _e705 = w2_;
        w3_1 = (_e704 + _e705);
        let _e708 = w3_1[0u];
        let _e710 = w3_1[1u];
        let _e713 = w3_1[2u];
        wf1_ = max(max(_e708, _e710), _e713);
        let _e715 = wf1_;
        if (_e715 > 1f) {
            let _e717 = wf1_;
            wf1_ = (1f / _e717);
            let _e719 = wf1_;
            let _e720 = w1_;
            w1_ = (_e720 * _e719);
            let _e722 = wf1_;
            let _e723 = w2_;
            w2_ = (_e723 * _e722);
        }
        let _e726 = params.clips;
        if (abs(_e726) > 0.005f) {
            let _e729 = mc1_;
            sy = _e729;
            let _e731 = params.clips;
            let _e733 = w1_;
            one = select(vec3<f32>(1f, 1f, 1f), _e733, vec3((_e731 > 0f)));
            let _e737 = cref1_[0u];
            let _e739 = cref1_[1u];
            let _e742 = cref1_[2u];
            sat_3 = (1.0001f - min(min(_e737, _e739), _e742));
            let _e745 = color1_;
            let _e746 = color1_;
            let _e747 = sat_3;
            param_37 = pow(_e746, (vec3<f32>(0.7f, 0.7f, 0.7f) - vec3((0.325f * _e747))));
            let _e752 = sy;
            param_38 = _e752;
            let _e753 = plant_u0028_vf3_u003b_f1_u003b((&param_37), (&param_38));
            let _e754 = sat_3;
            let _e756 = one;
            let _e759 = params.clips;
            color1_ = mix(_e745, _e753, ((_e756 * pow(_e754, 0.3333f)) * abs(_e759)));
            let _e763 = mc2_;
            sy = _e763;
            let _e765 = params.clips;
            let _e767 = w2_;
            one = select(vec3<f32>(1f, 1f, 1f), _e767, vec3((_e765 > 0f)));
            let _e771 = cref2_[0u];
            let _e773 = cref2_[1u];
            let _e776 = cref2_[2u];
            sat_3 = (1.0001f - min(min(_e771, _e773), _e776));
            let _e779 = color2_;
            let _e780 = color2_;
            let _e781 = sat_3;
            param_39 = pow(_e780, (vec3<f32>(0.7f, 0.7f, 0.7f) - vec3((0.325f * _e781))));
            let _e786 = sy;
            param_40 = _e786;
            let _e787 = plant_u0028_vf3_u003b_f1_u003b((&param_39), (&param_40));
            let _e788 = sat_3;
            let _e790 = one;
            let _e793 = params.clips;
            color2_ = mix(_e779, _e787, ((_e790 * pow(_e788, 0.3333f)) * abs(_e793)));
        }
        let _e797 = color1_;
        param_41 = _e797;
        let _e798 = gc_u0028_vf3_u003b((&param_41));
        let _e799 = w1_;
        let _e801 = color2_;
        param_42 = _e801;
        let _e802 = gc_u0028_vf3_u003b((&param_42));
        let _e803 = w2_;
        color_3 = ((_e798 * _e799) + (_e802 * _e803));
        let _e806 = color_3;
        color_3 = min(_e806, vec3(1f));
    }
    let _e809 = interb;
    if _e809 {
        let _e810 = color1_;
        param_43 = _e810;
        let _e811 = gc_u0028_vf3_u003b((&param_43));
        color_3 = _e811;
    }
    let _e813 = ctmp[0u];
    let _e815 = ctmp[1u];
    let _e818 = ctmp[2u];
    colmx = max(max(_e813, _e815), _e818);
    let _e820 = interb;
    if !(_e820) {
        let _e822 = color_3;
        let _e823 = gamma_in;
        let _e825 = global.scangamma;
        color_3 = pow(_e822, vec3((_e823 / _e825)));
    }
    let _e829 = color_3;
    let _e830 = colmx;
    FragColor = vec4<f32>(_e829.x, _e829.y, _e829.z, _e830);
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
