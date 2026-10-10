diagnostic(off, derivative_uniformity);
struct Push {
    SourceSize: vec4<f32>,
    OriginalSize: vec4<f32>,
    OutputSize: vec4<f32>,
    FrameCount: u32,
    TNTC: f32,
    LS: f32,
    LUTLOW: f32,
    LUTBR: f32,
    CP: f32,
    CS: f32,
    WP: f32,
    wp_saturation: f32,
    AS: f32,
    agsat: f32,
    BP: f32,
    vigstr: f32,
    vigdef: f32,
    sega_fix: f32,
    pre_bb: f32,
    contr: f32,
    pre_gc: f32,
}

struct UBO {
    MVP: mat4x4<f32>,
}

@group(0) @binding(8) 
var<uniform> params: Push;
@group(0) @binding(2) 
var StockPass: texture_2d<f32>;
var<private> vTexCoord_1: vec2<f32>;
@group(0) @binding(3) 
var AfterglowPass: texture_2d<f32>;
@group(0) @binding(4) 
var SamplerLUT1_: texture_2d<f32>;
@group(0) @binding(5) 
var SamplerLUT2_: texture_2d<f32>;
@group(0) @binding(6) 
var SamplerLUT3_: texture_2d<f32>;
@group(0) @binding(7) 
var SamplerLUT4_: texture_2d<f32>;
var<private> FragColor: vec4<f32>;
@group(0) @binding(0) 
var<uniform> global: UBO;
@group(1) @binding(2) 
var _StockPass_sampler: sampler;
@group(1) @binding(3) 
var _AfterglowPass_sampler: sampler;
@group(1) @binding(4) 
var _SamplerLUT1_sampler: sampler;
@group(1) @binding(5) 
var _SamplerLUT2_sampler: sampler;
@group(1) @binding(6) 
var _SamplerLUT3_sampler: sampler;
@group(1) @binding(7) 
var _SamplerLUT4_sampler: sampler;

fn vignette_u0028_vf2_u003b(pos: ptr<function, vec2<f32>>) -> f32 {
    var b: vec2<f32>;
    var res: vec2<f32>;

    let _e254 = params.vigdef;
    let _e256 = params.vigdef;
    let _e260 = params.OriginalSize[0u];
    let _e263 = params.OriginalSize[1u];
    b = ((vec2<f32>(_e254, _e256) * vec2<f32>(1f, (_e260 / _e263))) * 0.125f);
    let _e268 = (*pos);
    (*pos) = clamp(_e268, vec2(0f), vec2(1f));
    let _e272 = (*pos);
    (*pos) = abs(((_e272 - vec2(0.5f)) * 2f));
    let _e277 = b;
    let _e279 = (*pos);
    res = mix(vec2<f32>(0f, 0f), vec2<f32>(1f, 1f), smoothstep(vec2<f32>(1f, 1f), (vec2<f32>(1f, 1f) - _e277), sqrt(_e279)));
    let _e283 = res;
    res = pow(_e283, vec2<f32>(0.7f, 0.7f));
    let _e286 = res[0u];
    let _e288 = res[1u];
    let _e292 = params.vigstr;
    return max(mix(1f, sqrt((_e286 * _e288)), _e292), 0f);
}

fn pgc_u0028_vf3_u003b(c: ptr<function, vec3<f32>>) -> vec3<f32> {
    var mc: f32;
    var mg: f32;

    let _e254 = (*c)[0u];
    let _e256 = (*c)[1u];
    let _e259 = (*c)[2u];
    mc = max(max(_e254, _e256), _e259);
    let _e261 = mc;
    let _e263 = params.pre_gc;
    mg = pow(_e261, (1f / _e263));
    let _e266 = (*c);
    let _e267 = mg;
    let _e269 = mc;
    return ((_e266 * _e267) / vec3((_e269 + 0.00000001f)));
}

fn contrast_u0028_f1_u003b(x: ptr<function, f32>) -> f32 {
    let _e251 = (*x);
    let _e252 = (*x);
    let _e255 = params.contr;
    return max(mix(_e251, smoothstep(0f, 1f, _e252), _e255), 0f);
}

fn plant_u0028_vf3_u003b_f1_u003b(tar: ptr<function, vec3<f32>>, r: ptr<function, f32>) -> vec3<f32> {
    var t: f32;

    let _e254 = (*tar)[0u];
    let _e256 = (*tar)[1u];
    let _e259 = (*tar)[2u];
    t = (max(max(_e254, _e256), _e259) + 0.00001f);
    let _e262 = (*tar);
    let _e263 = (*r);
    let _e265 = t;
    return ((_e262 * _e263) / vec3(_e265));
}

fn fix_lut_u0028_vf3_u003b_vf3_u003b(lutcolor: ptr<function, vec3<f32>>, ref_: ptr<function, vec3<f32>>) -> vec3<f32> {
    var r_1: f32;
    var l: f32;
    var m: f32;

    let _e255 = (*ref_);
    r_1 = length(_e255);
    let _e257 = (*lutcolor);
    l = length(_e257);
    let _e260 = (*ref_)[0u];
    let _e262 = (*ref_)[1u];
    let _e265 = (*ref_)[2u];
    m = max(max(_e260, _e262), _e265);
    let _e267 = (*lutcolor);
    let _e271 = r_1;
    let _e272 = l;
    let _e273 = m;
    (*ref_) = (normalize((_e267 + vec3(0.0000001f))) * mix(_e271, _e272, pow(_e273, 1.25f)));
    let _e277 = (*lutcolor);
    let _e278 = (*ref_);
    return mix(_e277, _e278, vec3(1f));
}

fn main_1() {
    var imgColor: vec4<f32>;
    var aftglow: vec4<f32>;
    var w: f32;
    var l_1: f32;
    var bp: f32;
    var color: vec3<f32>;
    var lutlow: f32;
    var invLS: f32;
    var lut_ref: vec3<f32>;
    var lutb: f32;
    var tile1_: f32;
    var tile0_: f32;
    var f: f32;
    var coord0_: vec2<f32>;
    var coord1_: vec2<f32>;
    var color1_: vec4<f32>;
    var color2_: vec4<f32>;
    var res_1: vec4<f32>;
    var param: vec3<f32>;
    var param_1: vec3<f32>;
    var c_1: vec3<f32>;
    var p: f32;
    var m_out: mat3x3<f32>;
    var m_in: mat3x3<f32>;
    var scolor1_: vec3<f32>;
    var param_2: vec3<f32>;
    var param_3: f32;
    var luma: f32;
    var scolor2_: vec3<f32>;
    var param_4: f32;
    var param_5: vec3<f32>;
    var param_6: f32;
    var warmer: vec3<f32>;
    var cooler: vec3<f32>;
    var m_1: f32;
    var comp: vec3<f32>;
    var param_7: vec3<f32>;
    var param_8: vec2<f32>;

    let _e288 = vTexCoord_1;
    let _e289 = ue2_border_nearest(StockPass, _StockPass_sampler, _e288);
    imgColor = _e289;
    let _e290 = vTexCoord_1;
    let _e291 = ue2_border_linear(AfterglowPass, _AfterglowPass_sampler, _e290);
    aftglow = _e291;
    let _e293 = aftglow[3u];
    w = (1f - _e293);
    let _e295 = aftglow;
    l_1 = length(_e295.xyz);
    let _e299 = params.AS;
    let _e300 = w;
    let _e302 = aftglow;
    let _e307 = params.agsat;
    let _e312 = l_1;
    let _e313 = ((normalize(pow((_e302.xyz + vec3(0.01f)), vec3(_e307))) * (_e299 * _e300)) * _e312);
    aftglow[0u] = _e313.x;
    aftglow[1u] = _e313.y;
    aftglow[2u] = _e313.z;
    let _e320 = w;
    let _e322 = params.BP;
    bp = ((_e320 * _e322) / 255f);
    let _e326 = params.sega_fix;
    if (_e326 > 0.5f) {
        let _e328 = imgColor;
        let _e330 = (_e328.xyz * 1.0669456f);
        imgColor[0u] = _e330.x;
        imgColor[1u] = _e330.y;
        imgColor[2u] = _e330.z;
    }
    let _e337 = imgColor;
    let _e340 = min(_e337.xyz, vec3(1f));
    imgColor[0u] = _e340.x;
    imgColor[1u] = _e340.y;
    imgColor[2u] = _e340.z;
    let _e347 = imgColor;
    color = _e347.xyz;
    let _e350 = params.TNTC;
    if (i32(_e350) == 0i) {
        let _e353 = imgColor;
        color = _e353.xyz;
    } else {
        lutlow = 0.019607844f;
        let _e356 = params.LS;
        invLS = (1f / _e356);
        let _e358 = imgColor;
        let _e360 = lutlow;
        let _e361 = imgColor;
        lut_ref = (_e358.xyz + ((vec3(1f) - pow(_e361.xyz, vec3<f32>(0.333f, 0.333f, 0.333f))) * _e360));
        let _e369 = lut_ref[2u];
        let _e370 = invLS;
        lutb = (_e369 * (1f - (0.5f * _e370)));
        let _e374 = lut_ref;
        let _e376 = invLS;
        let _e379 = invLS;
        let _e382 = ((_e374.xy * (1f - _e376)) + vec2((0.5f * _e379)));
        lut_ref[0u] = _e382.x;
        lut_ref[1u] = _e382.y;
        let _e387 = lutb;
        let _e389 = params.LS;
        tile1_ = ceil((_e387 * (_e389 - 1f)));
        let _e393 = tile1_;
        tile0_ = max((_e393 - 1f), 0f);
        let _e396 = lutb;
        let _e398 = params.LS;
        f = fract((_e396 * (_e398 - 1f)));
        let _e402 = f;
        if (_e402 == 0f) {
            f = 1f;
        }
        let _e404 = tile0_;
        let _e406 = lut_ref[0u];
        let _e409 = lut_ref[1u];
        let _e411 = invLS;
        coord0_ = (vec2<f32>((_e404 + _e406), _e409) * vec2<f32>(_e411, 1f));
        let _e414 = tile1_;
        let _e416 = lut_ref[0u];
        let _e419 = lut_ref[1u];
        let _e421 = invLS;
        coord1_ = (vec2<f32>((_e414 + _e416), _e419) * vec2<f32>(_e421, 1f));
        let _e425 = params.TNTC;
        if (i32(_e425) == 1i) {
            let _e428 = coord0_;
            let _e429 = ue2_border_linear(SamplerLUT1_, _SamplerLUT1_sampler, _e428);
            color1_ = _e429;
            let _e430 = coord1_;
            let _e431 = ue2_border_linear(SamplerLUT1_, _SamplerLUT1_sampler, _e430);
            color2_ = _e431;
            let _e432 = color1_;
            let _e433 = color2_;
            let _e434 = f;
            res_1 = mix(_e432, _e433, vec4(_e434));
        } else {
            let _e438 = params.TNTC;
            if (i32(_e438) == 2i) {
                let _e441 = coord0_;
                let _e442 = ue2_border_linear(SamplerLUT2_, _SamplerLUT2_sampler, _e441);
                color1_ = _e442;
                let _e443 = coord1_;
                let _e444 = ue2_border_linear(SamplerLUT2_, _SamplerLUT2_sampler, _e443);
                color2_ = _e444;
                let _e445 = color1_;
                let _e446 = color2_;
                let _e447 = f;
                res_1 = mix(_e445, _e446, vec4(_e447));
            } else {
                let _e451 = params.TNTC;
                if (i32(_e451) == 3i) {
                    let _e454 = coord0_;
                    let _e455 = ue2_border_linear(SamplerLUT3_, _SamplerLUT3_sampler, _e454);
                    color1_ = _e455;
                    let _e456 = coord1_;
                    let _e457 = ue2_border_linear(SamplerLUT3_, _SamplerLUT3_sampler, _e456);
                    color2_ = _e457;
                    let _e458 = color1_;
                    let _e459 = color2_;
                    let _e460 = f;
                    res_1 = mix(_e458, _e459, vec4(_e460));
                } else {
                    let _e464 = params.TNTC;
                    if (i32(_e464) == 4i) {
                        let _e467 = coord0_;
                        let _e468 = ue2_border_linear(SamplerLUT4_, _SamplerLUT4_sampler, _e467);
                        color1_ = _e468;
                        let _e469 = coord1_;
                        let _e470 = ue2_border_linear(SamplerLUT4_, _SamplerLUT4_sampler, _e469);
                        color2_ = _e470;
                        let _e471 = color1_;
                        let _e472 = color2_;
                        let _e473 = f;
                        res_1 = mix(_e471, _e472, vec4(_e473));
                    }
                }
            }
        }
        let _e476 = res_1;
        param = _e476.xyz;
        let _e478 = imgColor;
        param_1 = _e478.xyz;
        let _e480 = fix_lut_u0028_vf3_u003b_vf3_u003b((&param), (&param_1));
        res_1[0u] = _e480.x;
        res_1[1u] = _e480.y;
        res_1[2u] = _e480.z;
        let _e487 = imgColor;
        let _e489 = res_1;
        let _e492 = params.TNTC;
        color = mix(_e487.xyz, _e489.xyz, vec3(min(_e492, 1f)));
    }
    let _e496 = color;
    c_1 = clamp(_e496, vec3(0f), vec3(1f));
    let _e501 = params.CS;
    if (_e501 == 0f) {
        p = 2.2f;
        m_out = mat3x3<f32>(vec3<f32>(3.24097f, -0.969244f, 0.05563f), vec3<f32>(-1.537383f, 1.875968f, -0.203977f), vec3<f32>(-0.498611f, 0.041555f, 1.056972f));
    } else {
        let _e504 = params.CS;
        if (_e504 == 1f) {
            p = 2.2f;
            m_out = mat3x3<f32>(vec3<f32>(2.791723f, -0.894766f, 0.041678f), vec3<f32>(-1.173165f, 1.815586f, -0.130886f), vec3<f32>(-0.440973f, 0.032f, 1.002034f));
        } else {
            let _e507 = params.CS;
            if (_e507 == 2f) {
                p = 2.6f;
                m_out = mat3x3<f32>(vec3<f32>(2.973422f, -0.867605f, 0.045031f), vec3<f32>(-1.110433f, 1.843757f, -0.095697f), vec3<f32>(-0.480247f, 0.024743f, 1.201215f));
            } else {
                let _e510 = params.CS;
                if (_e510 == 3f) {
                    p = 2.2f;
                    m_out = mat3x3<f32>(vec3<f32>(2.041588f, -0.969244f, 0.013444f), vec3<f32>(-0.565007f, 1.875968f, -0.11836f), vec3<f32>(-0.344731f, 0.041555f, 1.015175f));
                } else {
                    let _e513 = params.CS;
                    if (_e513 == 4f) {
                        p = 2.4f;
                        m_out = mat3x3<f32>(vec3<f32>(1.716651f, -0.666684f, 0.01764f), vec3<f32>(-0.355671f, 1.616481f, -0.042771f), vec3<f32>(-0.253366f, 0.015769f, 0.942103f));
                    } else {
                        let _e516 = params.CS;
                        if (_e516 == 5f) {
                            p = 2.2f;
                            m_out = mat3x3<f32>(vec3<f32>(2.493509f, -0.829473f, 0.0358512f), vec3<f32>(-0.931388f, 1.76263f, -0.0761839f), vec3<f32>(-0.402712f, 0.023624f, 0.9570296f));
                        }
                    }
                }
            }
        }
    }
    let _e518 = c_1;
    let _e519 = p;
    color = pow(_e518, vec3(_e519));
    m_in = mat3x3<f32>(vec3<f32>(0.412391f, 0.212639f, 0.019331f), vec3<f32>(0.357584f, 0.715169f, 0.119195f), vec3<f32>(0.180481f, 0.072192f, 0.950532f));
    let _e523 = params.CP;
    if (_e523 == 0f) {
        m_in = mat3x3<f32>(vec3<f32>(0.412391f, 0.212639f, 0.019331f), vec3<f32>(0.357584f, 0.715169f, 0.119195f), vec3<f32>(0.180481f, 0.072192f, 0.950532f));
    } else {
        let _e526 = params.CP;
        if (_e526 == 1f) {
            m_in = mat3x3<f32>(vec3<f32>(0.430554f, 0.222004f, 0.020182f), vec3<f32>(0.34155f, 0.706655f, 0.129553f), vec3<f32>(0.178352f, 0.071341f, 0.939322f));
        } else {
            let _e529 = params.CP;
            if (_e529 == 2f) {
                m_in = mat3x3<f32>(vec3<f32>(0.396686f, 0.210299f, 0.006131f), vec3<f32>(0.372504f, 0.713766f, 0.115356f), vec3<f32>(0.181266f, 0.075936f, 0.967571f));
            } else {
                let _e532 = params.CP;
                if (_e532 == 3f) {
                    m_in = mat3x3<f32>(vec3<f32>(0.393521f, 0.212376f, 0.018739f), vec3<f32>(0.365258f, 0.70106f, 0.111934f), vec3<f32>(0.191677f, 0.086564f, 0.958385f));
                } else {
                    let _e535 = params.CP;
                    if (_e535 == 4f) {
                        m_in = mat3x3<f32>(vec3<f32>(0.392258f, 0.20941f, 0.016061f), vec3<f32>(0.351135f, 0.72568f, 0.093636f), vec3<f32>(0.166603f, 0.06491f, 0.850324f));
                    } else {
                        let _e538 = params.CP;
                        if (_e538 == 5f) {
                            m_in = mat3x3<f32>(vec3<f32>(0.377923f, 0.195679f, 0.010514f), vec3<f32>(0.317366f, 0.722319f, 0.097826f), vec3<f32>(0.207738f, 0.082002f, 1.07696f));
                        }
                    }
                }
            }
        }
    }
    let _e540 = m_in;
    let _e541 = color;
    color = (_e540 * _e541);
    let _e543 = m_out;
    let _e544 = color;
    color = (_e543 * _e544);
    let _e546 = color;
    color = clamp(_e546, vec3(0f), vec3(1f));
    let _e550 = color;
    let _e551 = p;
    color = pow(_e550, vec3((1f / _e551)));
    let _e556 = params.CP;
    if (_e556 == -1f) {
        let _e558 = c_1;
        color = _e558;
    }
    let _e559 = color;
    let _e561 = params.wp_saturation;
    let _e565 = color[0u];
    let _e567 = color[1u];
    let _e570 = color[2u];
    param_2 = pow(_e559, vec3(_e561));
    param_3 = max(max(_e565, _e567), _e570);
    let _e572 = plant_u0028_vf3_u003b_f1_u003b((&param_2), (&param_3));
    scolor1_ = _e572;
    let _e573 = color;
    luma = dot(_e573, vec3<f32>(0.299f, 0.587f, 0.114f));
    let _e575 = luma;
    let _e577 = color;
    let _e579 = params.wp_saturation;
    scolor2_ = mix(vec3(_e575), _e577, vec3(_e579));
    let _e583 = params.wp_saturation;
    let _e585 = scolor1_;
    let _e586 = scolor2_;
    color = select(_e586, _e585, vec3((_e583 > 1f)));
    let _e590 = color[0u];
    let _e592 = color[1u];
    let _e595 = color[2u];
    param_4 = max(max(_e590, _e592), _e595);
    let _e597 = contrast_u0028_f1_u003b((&param_4));
    let _e598 = color;
    param_5 = _e598;
    param_6 = _e597;
    let _e599 = plant_u0028_vf3_u003b_f1_u003b((&param_5), (&param_6));
    color = _e599;
    p = 2.2f;
    let _e600 = color;
    color = clamp(_e600, vec3(0f), vec3(1f));
    let _e604 = color;
    let _e605 = p;
    color = pow(_e604, vec3(_e605));
    let _e608 = color;
    warmer = (mat3x3<f32>(vec3<f32>(0.48503393f, 0.2500956f, 0.022735965f), vec3<f32>(0.34889573f, 0.69779146f, 0.11629857f), vec3<f32>(0.13028236f, 0.052112944f, 0.68615377f)) * _e608);
    let _e610 = warmer;
    warmer = (mat3x3<f32>(vec3<f32>(3.24097f, -0.969244f, 0.05563f), vec3<f32>(-1.537383f, 1.875968f, -0.203977f), vec3<f32>(-0.498611f, 0.041555f, 1.056972f)) * _e610);
    let _e612 = color;
    cooler = (mat3x3<f32>(vec3<f32>(0.3412754f, 0.17597014f, 0.015997285f), vec3<f32>(0.36461705f, 0.7292341f, 0.12153902f), vec3<f32>(0.23698941f, 0.09479576f, 1.2481443f)) * _e612);
    let _e614 = cooler;
    cooler = (mat3x3<f32>(vec3<f32>(3.24097f, -0.969244f, 0.05563f), vec3<f32>(-1.537383f, 1.875968f, -0.203977f), vec3<f32>(-0.498611f, 0.041555f, 1.056972f)) * _e614);
    let _e617 = params.WP;
    m_1 = (abs(_e617) / 100f);
    let _e621 = params.WP;
    let _e623 = cooler;
    let _e624 = warmer;
    comp = select(_e624, _e623, vec3((_e621 < 0f)));
    let _e627 = color;
    let _e628 = comp;
    let _e629 = m_1;
    color = mix(_e627, _e628, vec3(_e629));
    let _e632 = color;
    let _e635 = p;
    color = pow(max(_e632, vec3(0f)), vec3((1f / _e635)));
    let _e639 = color;
    param_7 = _e639;
    let _e640 = pgc_u0028_vf3_u003b((&param_7));
    color = _e640;
    let _e642 = params.BP;
    if (_e642 > -0.5f) {
        let _e644 = color;
        let _e645 = aftglow;
        let _e648 = bp;
        color = ((_e644 + _e645.xyz) + vec3(_e648));
    } else {
        let _e651 = color;
        let _e653 = params.BP;
        let _e660 = params.BP;
        let _e663 = params.BP;
        let _e667 = color[0u];
        let _e669 = color[1u];
        let _e672 = color[2u];
        let _e679 = aftglow;
        color = ((max((_e651 + vec3((_e653 / 255f))), vec3(0f)) / vec3((1f + ((_e660 / 255f) * step((-(_e663) / 255f), max(max(_e667, _e669), _e672)))))) + _e679.xyz);
    }
    let _e682 = color;
    let _e684 = params.pre_bb;
    color = min((_e682 * _e684), vec3(1f));
    let _e688 = color;
    let _e689 = vTexCoord_1;
    param_8 = _e689;
    let _e690 = vignette_u0028_vf2_u003b((&param_8));
    FragColor = vec4<f32>(_e688.x, _e688.y, _e688.z, _e690);
    return;
}

@fragment 
fn main(@location(0) vTexCoord: vec2<f32>) -> @location(0) vec4<f32> {
    vTexCoord_1 = vTexCoord;
    main_1();
    let _e3 = FragColor;
    return _e3;
}

@group(0) @binding(9) var ue2_border_coverage:texture_2d<f32>;
@group(1) @binding(9) var ue2_border_coverage_sampler:sampler;

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
