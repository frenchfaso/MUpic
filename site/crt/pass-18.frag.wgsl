diagnostic(off, derivative_uniformity);
struct Push {
    IOS: f32,
    OS: f32,
    BLOOM: f32,
    brightboost: f32,
    brightboost1_: f32,
    csize: f32,
    bsize1_: f32,
    warpX: f32,
    warpY: f32,
    glow: f32,
    shadowMask: f32,
    masksize: f32,
    slotmask: f32,
    slotmask1_: f32,
    slotwidth: f32,
    double_slot: f32,
    mcut: f32,
    maskDark: f32,
    maskLight: f32,
    maskstr: f32,
    mshift: f32,
    mask_layout: f32,
    mask_bloom: f32,
    hiscan: f32,
    pr_scan: f32,
}

struct UBO {
    MVP: mat4x4<f32>,
    SourceSize: vec4<f32>,
    OriginalSize: vec4<f32>,
    OutputSize: vec4<f32>,
    FrameCount: u32,
    bloom: f32,
    halation: f32,
    slotms: f32,
    mask_gamma: f32,
    gamma_out: f32,
    overscanX: f32,
    overscanY: f32,
    VShift: f32,
    intres: f32,
    c_shape: f32,
    barspeed: f32,
    barintensity: f32,
    bardir: f32,
    sborder: f32,
    bloom_dist: f32,
    deconr: f32,
    decons: f32,
    addnoised: f32,
    noiseresd: f32,
    noisetype: f32,
    deconrr: f32,
    deconrg: f32,
    deconrb: f32,
    deconrry: f32,
    deconrgy: f32,
    deconrby: f32,
    dctypex: f32,
    dctypey: f32,
    post_br: f32,
    maskboost: f32,
    smoothmask: f32,
    gamma_c: f32,
    gamma_c2_: f32,
    m_glow: f32,
    m_glow_low: f32,
    m_glow_high: f32,
    m_glow_dist: f32,
    m_glow_mask: f32,
    smask_mit: f32,
    mask_zoom: f32,
    no_scanlines: f32,
    bmask: f32,
    bmask1_: f32,
    hmask1_: f32,
    mzoom_sh: f32,
    mclip: f32,
    edgemask: f32,
    oimage: f32,
    interm: f32,
}

@group(0) @binding(9) 
var<uniform> params: Push;
@group(0) @binding(0) 
var<uniform> global: UBO;
@group(0) @binding(7) 
var Source: texture_2d<f32>;
@group(0) @binding(5) 
var BloomPass: texture_2d<f32>;
@group(0) @binding(4) 
var GlowPass: texture_2d<f32>;
@group(0) @binding(3) 
var AvgLumPass: texture_2d<f32>;
@group(0) @binding(2) 
var LinearizePass: texture_2d<f32>;
var<private> vTexCoord_1: vec2<f32>;
@group(0) @binding(6) 
var PrePass0_: texture_2d<f32>;
var<private> FragColor: vec4<f32>;
@group(0) @binding(8) 
var StockPass: texture_2d<f32>;
@group(1) @binding(7) 
var _Source_sampler: sampler;
@group(1) @binding(5) 
var _BloomPass_sampler: sampler;
@group(1) @binding(4) 
var _GlowPass_sampler: sampler;
@group(1) @binding(3) 
var _AvgLumPass_sampler: sampler;
@group(1) @binding(2) 
var _LinearizePass_sampler: sampler;
@group(1) @binding(6) 
var _PrePass0_sampler: sampler;
@group(1) @binding(8) 
var _StockPass_sampler: sampler;

fn corner_u0028_vf2_u003b(pos: ptr<function, vec2<f32>>) -> f32 {
    var aspect: vec2<f32>;
    var b: f32;
    var crn: vec2<f32>;
    var cp: vec2<f32>;
    var cd: f32;
    var res: f32;

    let _e171 = (*pos);
    (*pos) = abs(((_e171 - vec2(0.5f)) * 2f));
    let _e178 = global.OutputSize[0u];
    let _e181 = global.OutputSize[1u];
    aspect = vec2<f32>(1f, (_e178 / _e181));
    let _e185 = params.bsize1_;
    b = ((_e185 * 0.05f) + 0.0005f);
    let _e189 = (*pos)[1u];
    let _e190 = b;
    let _e192 = aspect[1u];
    (*pos)[1u] = (_e189 + (_e190 * (_e192 - 1f)));
    let _e198 = params.csize;
    let _e200 = b;
    crn = max(vec2(_e198), vec2(((2f * _e200) + 0.0015f)));
    let _e205 = (*pos);
    let _e206 = crn;
    let _e207 = aspect;
    let _e214 = aspect;
    cp = (max((_e205 - (vec2(1f) - (_e206 * _e207))), vec2(0f)) / _e214);
    let _e216 = cp;
    let _e217 = cp;
    cd = sqrt(dot(_e216, _e217));
    let _e220 = (*pos);
    let _e221 = crn;
    let _e224 = cd;
    (*pos) = max(_e220, ((vec2(1f) - _e221) + vec2(_e224)));
    let _e228 = b;
    let _e231 = (*pos)[0u];
    let _e233 = (*pos)[1u];
    res = mix(1f, 0f, smoothstep((1f - _e228), 1f, sqrt(max(_e231, _e233))));
    let _e238 = res;
    let _e240 = global.sborder;
    return pow(_e238, _e240);
}

fn humbar_u0028_f1_u003b(pos_1: ptr<function, f32>) -> f32 {
    var local: f32;
    var local_1: f32;

    let _e168 = global.barintensity;
    if (_e168 == 0f) {
        return 1f;
    } else {
        let _e171 = global.barintensity;
        if (_e171 >= 0f) {
            let _e173 = (*pos_1);
            local = _e173;
        } else {
            let _e174 = (*pos_1);
            local = (1f - _e174);
        }
        let _e176 = local;
        (*pos_1) = _e176;
        let _e177 = (*pos_1);
        let _e179 = global.FrameCount;
        let _e180 = f32(_e179);
        let _e182 = global.barspeed;
        let _e188 = global.barspeed;
        (*pos_1) = fract((_e177 + ((_e180 - (floor((_e180 / _e182)) * _e182)) / (_e188 - 1f))));
        let _e194 = global.barintensity;
        if (_e194 < 0f) {
            let _e196 = (*pos_1);
            local_1 = _e196;
        } else {
            let _e197 = (*pos_1);
            local_1 = (1f - _e197);
        }
        let _e199 = local_1;
        (*pos_1) = _e199;
        let _e201 = global.barintensity;
        let _e204 = global.barintensity;
        let _e205 = (*pos_1);
        return ((1f - _e201) + (_e204 * _e205));
    }
}

fn noise_u0028_vf3_u003b(v: ptr<function, vec3<f32>>) -> vec3<f32> {
    let _e166 = global.addnoised;
    if (_e166 < 0f) {
        let _e169 = global.addnoised;
        (*v)[2u] = -(_e169);
    } else {
        let _e173 = (*v)[2u];
        (*v)[2u] = ((_e173 - (floor((_e173 / 6001f)) * 6001f)) / 1753f);
    }
    let _e180 = (*v);
    let _e182 = (*v);
    let _e186 = (*v);
    (*v) = ((fract(_e180) + fract((_e182 * 10000f))) + fract((_e186 * 0.0001f)));
    let _e190 = (*v);
    (*v) = (_e190 + vec3<f32>(0.12345f, 0.6789f, 0.314159f));
    let _e192 = (*v);
    let _e193 = (*v);
    let _e194 = (*v);
    (*v) = fract(((_e192 * dot(_e193, _e194)) * 123.456f));
    let _e199 = (*v);
    let _e200 = (*v);
    let _e201 = (*v);
    (*v) = fract(((_e199 * dot(_e200, _e201)) * 123.456f));
    let _e206 = (*v);
    let _e207 = (*v);
    let _e208 = (*v);
    (*v) = fract(((_e206 * dot(_e207, _e208)) * 123.456f));
    let _e213 = (*v);
    let _e214 = (*v);
    let _e215 = (*v);
    (*v) = fract(((_e213 * dot(_e214, _e215)) * 123.456f));
    let _e220 = (*v);
    return _e220;
}

fn gc2_u0028_vf3_u003b_f1_u003b(c: ptr<function, vec3<f32>>, w3_: ptr<function, f32>) -> vec3<f32> {
    var mc: f32;
    var gp: f32;
    var mg: f32;

    let _e170 = (*c)[0u];
    let _e172 = (*c)[1u];
    let _e175 = (*c)[2u];
    mc = max(max(_e170, _e172), _e175);
    let _e178 = global.gamma_c2_;
    let _e180 = (*w3_);
    gp = (1f / (1f + ((_e178 - 1f) * mix(0.375f, 1f, _e180))));
    let _e185 = mc;
    let _e186 = gp;
    mg = pow(_e185, _e186);
    let _e188 = (*c);
    let _e189 = mg;
    let _e191 = mc;
    return ((_e188 * _e189) / vec3((_e191 + 0.0000000001f)));
}

fn plant_u0028_vf3_u003b_f1_u003b(tar: ptr<function, vec3<f32>>, r: ptr<function, f32>) -> vec3<f32> {
    var t: f32;

    let _e168 = (*tar)[0u];
    let _e170 = (*tar)[1u];
    let _e173 = (*tar)[2u];
    t = (max(max(_e168, _e170), _e173) + 0.00001f);
    let _e176 = (*tar);
    let _e177 = (*r);
    let _e179 = t;
    return ((_e176 * _e177) / vec3(_e179));
}

fn SlotMask_u0028_vf2_u003b_f1_u003b_f1_u003b(pos_2: ptr<function, vec2<f32>>, m: ptr<function, f32>, swidth: ptr<function, f32>) -> f32 {
    var mlen: f32;
    var px: f32;
    var py: f32;
    var slot_dark: f32;
    var slot: f32;

    let _e173 = params.slotmask;
    let _e175 = params.slotmask1_;
    if ((_e173 + _e175) == 0f) {
        return 1f;
    } else {
        let _e179 = (*pos_2)[1u];
        let _e181 = global.slotms;
        (*pos_2)[1u] = floor((_e179 / _e181));
        let _e185 = (*swidth);
        mlen = (_e185 * 2f);
        let _e188 = (*pos_2)[0u];
        let _e189 = mlen;
        let _e190 = (0.99999f * _e189);
        px = floor((_e188 - (floor((_e188 / _e190)) * _e190)));
        let _e197 = (*pos_2)[1u];
        let _e199 = params.double_slot;
        let _e205 = params.double_slot;
        py = floor(((fract((_e197 / (2f * _e199))) * 2f) * _e205));
        let _e209 = params.slotmask1_;
        let _e212 = params.slotmask;
        let _e214 = (*m);
        slot_dark = mix((1f - _e209), (1f - _e212), _e214);
        slot = 1f;
        let _e216 = py;
        let _e218 = px;
        let _e219 = (*swidth);
        if ((_e216 == 0f) && (_e218 < _e219)) {
            let _e222 = slot_dark;
            slot = _e222;
        } else {
            let _e223 = py;
            let _e225 = params.double_slot;
            let _e227 = px;
            let _e228 = (*swidth);
            if ((_e223 == _e225) && (_e227 >= _e228)) {
                let _e231 = slot_dark;
                slot = _e231;
            }
        }
        let _e232 = slot;
        return _e232;
    }
}

fn Mask_u0028_vf2_u003b_f1_u003b_f1_u003b(pos_3: ptr<function, vec2<f32>>, mx: ptr<function, f32>, mb: ptr<function, f32>) -> vec3<f32> {
    var mask: vec3<f32>;
    var one: vec3<f32>;
    var mc_1: f32;
    var line: f32;
    var odd: f32;
    var maskmin: f32;

    let _e174 = params.maskDark;
    let _e176 = params.maskDark;
    let _e178 = params.maskDark;
    mask = vec3<f32>(_e174, _e176, _e178);
    one = vec3<f32>(1f, 1f, 1f);
    let _e181 = params.shadowMask;
    if (_e181 == 0f) {
        let _e184 = params.maskstr;
        mc_1 = (1f - max(_e184, 0f));
        let _e188 = (*pos_3)[0u];
        (*pos_3)[0u] = fract((_e188 * 0.5f));
        let _e193 = (*pos_3)[0u];
        if (_e193 < 0.49f) {
            mask[0u] = 1f;
            let _e196 = mc_1;
            mask[1u] = _e196;
            mask[2u] = 1f;
        } else {
            let _e199 = mc_1;
            mask[0u] = _e199;
            mask[1u] = 1f;
            let _e202 = mc_1;
            mask[2u] = _e202;
        }
    } else {
        let _e205 = params.shadowMask;
        if (_e205 == 1f) {
            let _e208 = params.maskLight;
            line = _e208;
            odd = 0f;
            let _e210 = (*pos_3)[0u];
            if (fract((_e210 / 6f)) < 0.49f) {
                odd = 1f;
            }
            let _e215 = (*pos_3)[1u];
            let _e216 = odd;
            if (fract(((_e215 + _e216) / 2f)) < 0.49f) {
                let _e222 = params.maskDark;
                line = _e222;
            }
            let _e224 = (*pos_3)[0u];
            (*pos_3)[0u] = floor((_e224 - (floor((_e224 / 3f)) * 3f)));
            let _e232 = (*pos_3)[0u];
            if (_e232 < 0.5f) {
                let _e235 = params.maskLight;
                mask[0u] = _e235;
            } else {
                let _e238 = (*pos_3)[0u];
                if (_e238 < 1.5f) {
                    let _e241 = params.maskLight;
                    mask[1u] = _e241;
                } else {
                    let _e244 = params.maskLight;
                    mask[2u] = _e244;
                }
            }
            let _e246 = line;
            let _e247 = mask;
            mask = (_e247 * _e246);
        } else {
            let _e250 = params.shadowMask;
            if (_e250 == 2f) {
                let _e253 = (*pos_3)[0u];
                (*pos_3)[0u] = floor((_e253 - (floor((_e253 / 3f)) * 3f)));
                let _e261 = (*pos_3)[0u];
                if (_e261 < 0.5f) {
                    let _e264 = params.maskLight;
                    mask[0u] = _e264;
                } else {
                    let _e267 = (*pos_3)[0u];
                    if (_e267 < 1.5f) {
                        let _e270 = params.maskLight;
                        mask[1u] = _e270;
                    } else {
                        let _e273 = params.maskLight;
                        mask[2u] = _e273;
                    }
                }
            } else {
                let _e276 = params.shadowMask;
                if (_e276 == 3f) {
                    let _e279 = (*pos_3)[1u];
                    let _e282 = (*pos_3)[0u];
                    (*pos_3)[0u] = (_e282 + (_e279 * 3f));
                    let _e286 = (*pos_3)[0u];
                    (*pos_3)[0u] = fract((_e286 / 6f));
                    let _e291 = (*pos_3)[0u];
                    if (_e291 < 0.3f) {
                        let _e294 = params.maskLight;
                        mask[0u] = _e294;
                    } else {
                        let _e297 = (*pos_3)[0u];
                        if (_e297 < 0.6f) {
                            let _e300 = params.maskLight;
                            mask[1u] = _e300;
                        } else {
                            let _e303 = params.maskLight;
                            mask[2u] = _e303;
                        }
                    }
                } else {
                    let _e306 = params.shadowMask;
                    if (_e306 == 4f) {
                        let _e308 = (*pos_3);
                        (*pos_3) = floor((_e308 * vec2<f32>(1f, 0.5f)));
                        let _e312 = (*pos_3)[1u];
                        let _e315 = (*pos_3)[0u];
                        (*pos_3)[0u] = (_e315 + (_e312 * 3f));
                        let _e319 = (*pos_3)[0u];
                        (*pos_3)[0u] = fract((_e319 / 6f));
                        let _e324 = (*pos_3)[0u];
                        if (_e324 < 0.3f) {
                            let _e327 = params.maskLight;
                            mask[0u] = _e327;
                        } else {
                            let _e330 = (*pos_3)[0u];
                            if (_e330 < 0.6f) {
                                let _e333 = params.maskLight;
                                mask[1u] = _e333;
                            } else {
                                let _e336 = params.maskLight;
                                mask[2u] = _e336;
                            }
                        }
                    } else {
                        let _e339 = params.shadowMask;
                        if (_e339 == 5f) {
                            mask = vec3<f32>(0f, 0f, 0f);
                            let _e342 = (*pos_3)[0u];
                            (*pos_3)[0u] = fract((_e342 / 2f));
                            let _e347 = (*pos_3)[0u];
                            if (_e347 < 0.49f) {
                                mask[0u] = 1f;
                                mask[2u] = 1f;
                            } else {
                                mask[1u] = 1f;
                            }
                            let _e352 = one;
                            let _e353 = mask;
                            let _e355 = params.mcut;
                            let _e358 = one;
                            let _e359 = mask;
                            let _e361 = params.maskstr;
                            let _e364 = (*mx);
                            mask = clamp(mix(mix(_e352, _e353, vec3(_e355)), mix(_e358, _e359, vec3(_e361)), vec3(_e364)), vec3(0f), vec3(1f));
                        } else {
                            let _e371 = params.shadowMask;
                            if (_e371 == 6f) {
                                mask = vec3<f32>(0f, 0f, 0f);
                                let _e374 = (*pos_3)[0u];
                                (*pos_3)[0u] = floor((_e374 - (floor((_e374 / 3f)) * 3f)));
                                let _e382 = (*pos_3)[0u];
                                if (_e382 < 0.5f) {
                                    mask[0u] = 1f;
                                } else {
                                    let _e386 = (*pos_3)[0u];
                                    if (_e386 < 1.5f) {
                                        mask[1u] = 1f;
                                    } else {
                                        mask[2u] = 1f;
                                    }
                                }
                                let _e390 = one;
                                let _e391 = mask;
                                let _e393 = params.mcut;
                                let _e396 = one;
                                let _e397 = mask;
                                let _e399 = params.maskstr;
                                let _e402 = (*mx);
                                mask = clamp(mix(mix(_e390, _e391, vec3(_e393)), mix(_e396, _e397, vec3(_e399)), vec3(_e402)), vec3(0f), vec3(1f));
                            } else {
                                let _e409 = params.shadowMask;
                                if (_e409 == 7f) {
                                    mask = vec3<f32>(0f, 0f, 0f);
                                    let _e412 = (*pos_3)[0u];
                                    (*pos_3)[0u] = fract((_e412 / 2f));
                                    let _e417 = (*pos_3)[0u];
                                    if (_e417 < 0.49f) {
                                        mask = vec3<f32>(0f, 0f, 0f);
                                    } else {
                                        mask = vec3<f32>(1f, 1f, 1f);
                                    }
                                    let _e419 = one;
                                    let _e420 = mask;
                                    let _e422 = params.mcut;
                                    let _e425 = one;
                                    let _e426 = mask;
                                    let _e428 = params.maskstr;
                                    let _e431 = (*mx);
                                    mask = clamp(mix(mix(_e419, _e420, vec3(_e422)), mix(_e425, _e426, vec3(_e428)), vec3(_e431)), vec3(0f), vec3(1f));
                                } else {
                                    let _e438 = params.shadowMask;
                                    if (_e438 == 8f) {
                                        mask = vec3<f32>(0f, 0f, 0f);
                                        let _e441 = (*pos_3)[0u];
                                        (*pos_3)[0u] = fract((_e441 / 3f));
                                        let _e446 = (*pos_3)[0u];
                                        if (_e446 < 0.3f) {
                                            mask = vec3<f32>(0f, 0f, 0f);
                                        } else {
                                            let _e449 = (*pos_3)[0u];
                                            if (_e449 < 0.6f) {
                                                mask = vec3<f32>(1f, 1f, 1f);
                                            } else {
                                                mask = vec3<f32>(1f, 1f, 1f);
                                            }
                                        }
                                        let _e451 = one;
                                        let _e452 = mask;
                                        let _e454 = params.mcut;
                                        let _e457 = one;
                                        let _e458 = mask;
                                        let _e460 = params.maskstr;
                                        let _e463 = (*mx);
                                        mask = clamp(mix(mix(_e451, _e452, vec3(_e454)), mix(_e457, _e458, vec3(_e460)), vec3(_e463)), vec3(0f), vec3(1f));
                                    } else {
                                        let _e470 = params.shadowMask;
                                        if (_e470 == 9f) {
                                            mask = vec3<f32>(0f, 0f, 0f);
                                            let _e473 = (*pos_3)[0u];
                                            (*pos_3)[0u] = fract((_e473 / 3f));
                                            let _e478 = (*pos_3)[0u];
                                            if (_e478 < 0.3f) {
                                                mask = vec3<f32>(0f, 0f, 0f);
                                            } else {
                                                let _e481 = (*pos_3)[0u];
                                                if (_e481 < 0.6f) {
                                                    mask[0u] = vec2<f32>(1f, 1f).x;
                                                    mask[2u] = vec2<f32>(1f, 1f).y;
                                                } else {
                                                    mask[1u] = 1f;
                                                }
                                            }
                                            let _e488 = one;
                                            let _e489 = mask;
                                            let _e491 = params.mcut;
                                            let _e494 = one;
                                            let _e495 = mask;
                                            let _e497 = params.maskstr;
                                            let _e500 = (*mx);
                                            mask = clamp(mix(mix(_e488, _e489, vec3(_e491)), mix(_e494, _e495, vec3(_e497)), vec3(_e500)), vec3(0f), vec3(1f));
                                        } else {
                                            let _e507 = params.shadowMask;
                                            if (_e507 == 10f) {
                                                mask = vec3<f32>(0f, 0f, 0f);
                                                let _e510 = (*pos_3)[0u];
                                                (*pos_3)[0u] = fract((_e510 * 0.25f));
                                                let _e515 = (*pos_3)[0u];
                                                if (_e515 < 0.2f) {
                                                    mask = vec3<f32>(0f, 0f, 0f);
                                                } else {
                                                    let _e518 = (*pos_3)[0u];
                                                    if (_e518 < 0.4f) {
                                                        mask[0u] = 1f;
                                                    } else {
                                                        let _e522 = (*pos_3)[0u];
                                                        if (_e522 < 0.7f) {
                                                            mask[1u] = 1f;
                                                        } else {
                                                            mask[2u] = 1f;
                                                        }
                                                    }
                                                }
                                                let _e526 = one;
                                                let _e527 = mask;
                                                let _e529 = params.mcut;
                                                let _e532 = one;
                                                let _e533 = mask;
                                                let _e535 = params.maskstr;
                                                let _e538 = (*mx);
                                                mask = clamp(mix(mix(_e526, _e527, vec3(_e529)), mix(_e532, _e533, vec3(_e535)), vec3(_e538)), vec3(0f), vec3(1f));
                                            } else {
                                                let _e545 = params.shadowMask;
                                                if (_e545 == 11f) {
                                                    mask = vec3<f32>(0f, 0f, 0f);
                                                    let _e548 = (*pos_3)[0u];
                                                    (*pos_3)[0u] = fract((_e548 * 0.25f));
                                                    let _e553 = (*pos_3)[0u];
                                                    if (_e553 < 0.2f) {
                                                        mask[0u] = 1f;
                                                    } else {
                                                        let _e557 = (*pos_3)[0u];
                                                        if (_e557 < 0.4f) {
                                                            mask[0u] = vec2<f32>(1f, 1f).x;
                                                            mask[1u] = vec2<f32>(1f, 1f).y;
                                                        } else {
                                                            let _e564 = (*pos_3)[0u];
                                                            if (_e564 < 0.7f) {
                                                                mask[1u] = vec2<f32>(1f, 1f).x;
                                                                mask[2u] = vec2<f32>(1f, 1f).y;
                                                            } else {
                                                                mask[2u] = 1f;
                                                            }
                                                        }
                                                    }
                                                    let _e571 = one;
                                                    let _e572 = mask;
                                                    let _e574 = params.mcut;
                                                    let _e577 = one;
                                                    let _e578 = mask;
                                                    let _e580 = params.maskstr;
                                                    let _e583 = (*mx);
                                                    mask = clamp(mix(mix(_e571, _e572, vec3(_e574)), mix(_e577, _e578, vec3(_e580)), vec3(_e583)), vec3(0f), vec3(1f));
                                                } else {
                                                    let _e590 = params.shadowMask;
                                                    if (_e590 == 12f) {
                                                        mask = vec3<f32>(0f, 0f, 0f);
                                                        let _e593 = (*pos_3)[0u];
                                                        (*pos_3)[0u] = floor((_e593 - (floor((_e593 / 7f)) * 7f)));
                                                        let _e601 = (*pos_3)[0u];
                                                        if (_e601 < 0.5f) {
                                                            mask = vec3<f32>(0f, 0f, 0f);
                                                        } else {
                                                            let _e604 = (*pos_3)[0u];
                                                            if (_e604 < 2.5f) {
                                                                mask[0u] = 1f;
                                                            } else {
                                                                let _e608 = (*pos_3)[0u];
                                                                if (_e608 < 4.5f) {
                                                                    mask[1u] = 1f;
                                                                } else {
                                                                    mask[2u] = 1f;
                                                                }
                                                            }
                                                        }
                                                        let _e612 = one;
                                                        let _e613 = mask;
                                                        let _e615 = params.mcut;
                                                        let _e618 = one;
                                                        let _e619 = mask;
                                                        let _e621 = params.maskstr;
                                                        let _e624 = (*mx);
                                                        mask = clamp(mix(mix(_e612, _e613, vec3(_e615)), mix(_e618, _e619, vec3(_e621)), vec3(_e624)), vec3(0f), vec3(1f));
                                                    } else {
                                                        let _e631 = params.shadowMask;
                                                        if (_e631 == 13f) {
                                                            mask = vec3<f32>(0f, 0f, 0f);
                                                            let _e634 = (*pos_3)[0u];
                                                            (*pos_3)[0u] = floor((_e634 - (floor((_e634 / 6f)) * 6f)));
                                                            let _e642 = (*pos_3)[0u];
                                                            if (_e642 < 0.5f) {
                                                                mask = vec3<f32>(0f, 0f, 0f);
                                                            } else {
                                                                let _e645 = (*pos_3)[0u];
                                                                if (_e645 < 1.5f) {
                                                                    mask[0u] = 1f;
                                                                } else {
                                                                    let _e649 = (*pos_3)[0u];
                                                                    if (_e649 < 2.5f) {
                                                                        mask[0u] = vec2<f32>(1f, 1f).x;
                                                                        mask[1u] = vec2<f32>(1f, 1f).y;
                                                                    } else {
                                                                        let _e656 = (*pos_3)[0u];
                                                                        if (_e656 < 3.5f) {
                                                                            mask = vec3<f32>(1f, 1f, 1f);
                                                                        } else {
                                                                            let _e659 = (*pos_3)[0u];
                                                                            if (_e659 < 4.5f) {
                                                                                mask[1u] = vec2<f32>(1f, 1f).x;
                                                                                mask[2u] = vec2<f32>(1f, 1f).y;
                                                                            } else {
                                                                                mask[2u] = 1f;
                                                                            }
                                                                        }
                                                                    }
                                                                }
                                                            }
                                                            let _e666 = one;
                                                            let _e667 = mask;
                                                            let _e669 = params.mcut;
                                                            let _e672 = one;
                                                            let _e673 = mask;
                                                            let _e675 = params.maskstr;
                                                            let _e678 = (*mx);
                                                            mask = clamp(mix(mix(_e666, _e667, vec3(_e669)), mix(_e672, _e673, vec3(_e675)), vec3(_e678)), vec3(0f), vec3(1f));
                                                        } else {
                                                            mask = vec3<f32>(0f, 0f, 0f);
                                                            let _e685 = (*pos_3)[0u];
                                                            (*pos_3)[0u] = floor((_e685 - (floor((_e685 / 5f)) * 5f)));
                                                            let _e693 = (*pos_3)[0u];
                                                            if (_e693 < 0.5f) {
                                                                mask = vec3<f32>(0f, 0f, 0f);
                                                            } else {
                                                                let _e696 = (*pos_3)[0u];
                                                                if (_e696 < 1.5f) {
                                                                    mask[0u] = 1f;
                                                                } else {
                                                                    let _e700 = (*pos_3)[0u];
                                                                    if (_e700 < 2.5f) {
                                                                        mask[0u] = vec2<f32>(1f, 1f).x;
                                                                        mask[1u] = vec2<f32>(1f, 1f).y;
                                                                    } else {
                                                                        let _e707 = (*pos_3)[0u];
                                                                        if (_e707 < 3.5f) {
                                                                            mask[1u] = vec2<f32>(1f, 1f).x;
                                                                            mask[2u] = vec2<f32>(1f, 1f).y;
                                                                        } else {
                                                                            mask[2u] = 1f;
                                                                        }
                                                                    }
                                                                }
                                                            }
                                                            let _e714 = one;
                                                            let _e715 = mask;
                                                            let _e717 = params.mcut;
                                                            let _e720 = one;
                                                            let _e721 = mask;
                                                            let _e723 = params.maskstr;
                                                            let _e726 = (*mx);
                                                            mask = clamp(mix(mix(_e714, _e715, vec3(_e717)), mix(_e720, _e721, vec3(_e723)), vec3(_e726)), vec3(0f), vec3(1f));
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    let _e733 = params.mask_layout;
    if (_e733 > 0.5f) {
        let _e735 = mask;
        mask = _e735.xzy;
    }
    let _e738 = mask[0u];
    let _e740 = mask[1u];
    let _e743 = mask[2u];
    maskmin = min(min(_e738, _e740), _e743);
    let _e745 = mask;
    let _e746 = maskmin;
    let _e750 = global.maskboost;
    let _e752 = (*mb);
    let _e756 = maskmin;
    return (((_e745 - vec3(_e746)) * (1f + ((_e750 - 1f) * _e752))) + vec3(_e756));
}

fn igc_u0028_f1_u003b(mc_2: ptr<function, f32>) -> f32 {
    let _e165 = (*mc_2);
    let _e167 = global.gamma_c;
    return pow(_e165, _e167);
}

fn fetch_pixel_u0028_vf3_u003b_vf3_u003b_vf3_u003b_vf2_u003b_vf2_u003b(c_1: ptr<function, vec3<f32>>, b_1: ptr<function, vec3<f32>>, g: ptr<function, vec3<f32>>, coord: ptr<function, vec2<f32>>, bcoord: ptr<function, vec2<f32>>) {
    var stepx: f32;
    var stepy: f32;
    var ds: f32;
    var dx: vec2<f32>;
    var dy: vec2<f32>;
    var posx: f32;
    var posy: f32;
    var rc: vec2<f32>;
    var gc: vec2<f32>;
    var bc: vec2<f32>;
    var r1_: f32;
    var g1_: f32;
    var b1_: f32;
    var d: vec3<f32>;

    let _e185 = global.OutputSize[2u];
    stepx = _e185;
    let _e188 = global.OutputSize[3u];
    stepy = _e188;
    let _e190 = global.decons;
    ds = _e190;
    let _e191 = stepx;
    dx = vec2<f32>(_e191, 0f);
    let _e193 = stepy;
    dy = vec2<f32>(0f, _e193);
    let _e196 = (*coord)[0u];
    posx = ((2f * _e196) - 1f);
    let _e200 = (*coord)[1u];
    posy = ((2f * _e200) - 1f);
    let _e204 = global.dctypex;
    if (_e204 > 0.025f) {
        let _e206 = posx;
        let _e208 = posx;
        let _e211 = global.dctypex;
        posx = (sign(_e206) * pow(abs(_e208), (1.05f - _e211)));
        let _e215 = posx;
        let _e216 = dx;
        dx = (_e216 * _e215);
    }
    let _e219 = global.dctypey;
    if (_e219 > 0.025f) {
        let _e221 = posy;
        let _e223 = posy;
        let _e226 = global.dctypey;
        posy = (sign(_e221) * pow(abs(_e223), (1.05f - _e226)));
        let _e230 = posy;
        let _e231 = dy;
        dy = (_e231 * _e230);
    }
    let _e234 = global.deconrr;
    let _e235 = dx;
    let _e238 = global.deconrry;
    let _e239 = dy;
    rc = ((_e235 * _e234) + (_e239 * _e238));
    let _e243 = global.deconrg;
    let _e244 = dx;
    let _e247 = global.deconrgy;
    let _e248 = dy;
    gc = ((_e244 * _e243) + (_e248 * _e247));
    let _e252 = global.deconrb;
    let _e253 = dx;
    let _e256 = global.deconrby;
    let _e257 = dy;
    bc = ((_e253 * _e252) + (_e257 * _e256));
    let _e260 = (*coord);
    let _e261 = rc;
    let _e263 = ue2_border_linear(Source, _Source_sampler, (_e260 + _e261));
    r1_ = _e263.x;
    let _e265 = (*coord);
    let _e266 = gc;
    let _e268 = ue2_border_linear(Source, _Source_sampler, (_e265 + _e266));
    g1_ = _e268.y;
    let _e270 = (*coord);
    let _e271 = bc;
    let _e273 = ue2_border_linear(Source, _Source_sampler, (_e270 + _e271));
    b1_ = _e273.z;
    let _e275 = r1_;
    let _e276 = g1_;
    let _e277 = b1_;
    d = vec3<f32>(_e275, _e276, _e277);
    let _e279 = (*c_1);
    let _e280 = d;
    let _e281 = ds;
    (*c_1) = clamp(mix(_e279, _e280, vec3(_e281)), vec3(0f), vec3(1f));
    let _e287 = (*bcoord);
    let _e288 = rc;
    let _e290 = ue2_border_linear(BloomPass, _BloomPass_sampler, (_e287 + _e288));
    r1_ = _e290.x;
    let _e292 = (*bcoord);
    let _e293 = gc;
    let _e295 = ue2_border_linear(BloomPass, _BloomPass_sampler, (_e292 + _e293));
    g1_ = _e295.y;
    let _e297 = (*bcoord);
    let _e298 = bc;
    let _e300 = ue2_border_linear(BloomPass, _BloomPass_sampler, (_e297 + _e298));
    b1_ = _e300.z;
    let _e302 = r1_;
    let _e303 = g1_;
    let _e304 = b1_;
    d = vec3<f32>(_e302, _e303, _e304);
    let _e306 = (*b_1);
    let _e307 = d;
    let _e308 = ds;
    let _e311 = mix(_e306, _e307, vec3(min(_e308, 1f)));
    (*g) = _e311;
    (*b_1) = _e311;
    let _e312 = (*bcoord);
    let _e313 = rc;
    let _e315 = ue2_border_linear(GlowPass, _GlowPass_sampler, (_e312 + _e313));
    r1_ = _e315.x;
    let _e317 = (*bcoord);
    let _e318 = gc;
    let _e320 = ue2_border_linear(GlowPass, _GlowPass_sampler, (_e317 + _e318));
    g1_ = _e320.y;
    let _e322 = (*bcoord);
    let _e323 = bc;
    let _e325 = ue2_border_linear(GlowPass, _GlowPass_sampler, (_e322 + _e323));
    b1_ = _e325.z;
    let _e327 = r1_;
    let _e328 = g1_;
    let _e329 = b1_;
    d = vec3<f32>(_e327, _e328, _e329);
    let _e331 = (*g);
    let _e332 = d;
    let _e333 = ds;
    (*g) = mix(_e331, _e332, vec3(min(_e333, 1f)));
    return;
}

fn Warp_u0028_vf2_u003b(pos_4: ptr<function, vec2<f32>>) -> vec2<f32> {
    let _e165 = (*pos_4);
    (*pos_4) = ((_e165 * 2f) - vec2(1f));
    let _e169 = (*pos_4);
    let _e171 = (*pos_4)[0u];
    let _e173 = global.c_shape;
    let _e175 = (*pos_4)[1u];
    let _e178 = (*pos_4)[1u];
    let _e184 = (*pos_4)[1u];
    let _e186 = global.c_shape;
    let _e188 = (*pos_4)[0u];
    let _e191 = (*pos_4)[0u];
    let _e198 = params.warpX;
    let _e200 = params.warpY;
    let _e203 = global.c_shape;
    (*pos_4) = mix(_e169, vec2<f32>((_e171 * inverseSqrt((1f - ((_e173 * _e175) * _e178)))), (_e184 * inverseSqrt((1f - ((_e186 * _e188) * _e191))))), (vec2<f32>(_e198, _e200) / vec2(_e203)));
    let _e207 = (*pos_4);
    return ((_e207 * 0.5f) + vec2(0.5f));
}

fn Overscan_u0028_vf2_u003b_f1_u003b_f1_u003b(pos_5: ptr<function, vec2<f32>>, dx_1: ptr<function, f32>, dy_1: ptr<function, f32>) -> vec2<f32> {
    let _e167 = (*pos_5);
    (*pos_5) = ((_e167 * 2f) - vec2(1f));
    let _e171 = (*dx_1);
    let _e172 = (*dy_1);
    let _e174 = (*pos_5);
    (*pos_5) = (_e174 * vec2<f32>(_e171, _e172));
    let _e176 = (*pos_5);
    return ((_e176 * 0.5f) + vec2(0.5f));
}

fn main_1() {
    var SourceSize: vec4<f32>;
    var lum: f32;
    var gamma_in: f32;
    var intera: f32;
    var inter6_: bool;
    var interb: bool;
    var texcoord: vec2<f32>;
    var ofactor: vec2<f32>;
    var intfactor: vec2<f32>;
    var local_2: vec2<f32>;
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
    var pos1_: vec2<f32>;
    var pos_6: vec2<f32>;
    var param_9: vec2<f32>;
    var pos0_: vec2<f32>;
    var param_10: vec2<f32>;
    var posb: vec2<f32>;
    var color: vec3<f32>;
    var Bloom: vec3<f32>;
    var Glow: vec3<f32>;
    var param_11: vec3<f32>;
    var param_12: vec3<f32>;
    var param_13: vec3<f32>;
    var param_14: vec2<f32>;
    var param_15: vec2<f32>;
    var cm: f32;
    var param_16: f32;
    var mx1_: f32;
    var colmx: f32;
    var w3_1: f32;
    var dx_2: vec2<f32>;
    var mx0_: f32;
    var mx2_: f32;
    var mxg: f32;
    var mx_1: f32;
    var cx: f32;
    var mb_1: f32;
    var one_1: vec3<f32>;
    var orig1_: vec3<f32>;
    var cmask: vec3<f32>;
    var cmask1_: vec3<f32>;
    var cmask2_: vec3<f32>;
    var mwidths: array<f32, 15>;
    var mwidth: f32;
    var mask_compensate: f32;
    var mwidth1_: f32;
    var maskcoord: vec2<f32>;
    var scoord: vec2<f32>;
    var swidth_1: f32;
    var zoomed: bool;
    var mscale: f32;
    var maskcoord0_: vec2<f32>;
    var stagg_lvl: f32;
    var next_line: f32;
    var param_17: vec2<f32>;
    var param_18: f32;
    var param_19: f32;
    var mlerp: f32;
    var mcoord: f32;
    var param_20: vec2<f32>;
    var param_21: f32;
    var param_22: f32;
    var param_23: vec2<f32>;
    var param_24: f32;
    var param_25: f32;
    var sm_offset: f32;
    var bsm_offset: bool;
    var smask: f32;
    var param_26: vec2<f32>;
    var param_27: f32;
    var param_28: f32;
    var maxbl: f32;
    var param_29: vec3<f32>;
    var param_30: f32;
    var dark_compensate: f32;
    var bb: f32;
    var Ref: vec3<f32>;
    var maxb: f32;
    var vig: f32;
    var mbl: f32;
    var param_31: vec3<f32>;
    var param_32: f32;
    var Bloom1_: vec3<f32>;
    var bcmask: vec3<f32>;
    var hcmask: vec3<f32>;
    var param_33: vec3<f32>;
    var param_34: f32;
    var mbl_1: f32;
    var cmxh: f32;
    var param_35: vec3<f32>;
    var param_36: f32;
    var param_37: vec3<f32>;
    var param_38: f32;
    var mbl_2: f32;
    var param_39: vec3<f32>;
    var param_40: f32;
    var param_41: vec3<f32>;
    var param_42: f32;
    var w4_: f32;
    var param_43: vec3<f32>;
    var param_44: f32;
    var orig2_: vec3<f32>;
    var param_45: vec3<f32>;
    var param_46: f32;
    var param_47: vec3<f32>;
    var param_48: f32;
    var cmaskg: vec3<f32>;
    var mx3_: f32;
    var mx4_: f32;
    var temp: vec3<f32>;
    var rc_1: f32;
    var noise0_: vec3<f32>;
    var param_49: vec3<f32>;
    var param_50: f32;
    var param_51: vec2<f32>;
    var phi_1489_: bool;
    var phi_1503_: bool;
    var phi_1510_: bool;
    var phi_1568_: bool;
    var phi_1983_: bool;
    var phi_2026_: bool;
    var phi_2033_: bool;
    var phi_2040_: bool;
    var phi_2047_: bool;
    var phi_2054_: bool;

    let _e291 = global.OriginalSize;
    SourceSize = _e291;
    let _e292 = ue2_border_linear(AvgLumPass, _AvgLumPass_sampler, vec2<f32>(0.5f, 0.5f));
    lum = _e292.w;
    let _e294 = ue2_border_linear(LinearizePass, _LinearizePass_sampler, vec2<f32>(0.25f, 0.25f));
    gamma_in = (1f / _e294.w);
    let _e297 = ue2_border_linear(LinearizePass, _LinearizePass_sampler, vec2<f32>(0.75f, 0.25f));
    intera = _e297.w;
    let _e299 = intera;
    let _e300 = (_e299 < 0.35f);
    phi_1489_ = _e300;
    if _e300 {
        let _e302 = global.interm;
        phi_1489_ = (_e302 == 6f);
    }
    let _e305 = phi_1489_;
    inter6_ = _e305;
    let _e306 = intera;
    let _e308 = inter6_;
    let _e310 = ((_e306 < 0.35f) && !(_e308));
    phi_1503_ = _e310;
    if !(_e310) {
        let _e313 = global.no_scanlines;
        phi_1503_ = (_e313 > 0.025f);
    }
    let _e316 = phi_1503_;
    phi_1510_ = _e316;
    if _e316 {
        let _e318 = params.hiscan;
        phi_1510_ = (_e318 < 0.5f);
    }
    let _e321 = phi_1510_;
    interb = _e321;
    let _e322 = vTexCoord_1;
    texcoord = _e322;
    let _e324 = params.IOS;
    let _e326 = interb;
    if ((_e324 > 0f) && !(_e326)) {
        let _e330 = global.OutputSize;
        let _e333 = global.OriginalSize;
        ofactor = (_e330.xy / _e333.xy);
        let _e337 = params.IOS;
        if (_e337 < 2.5f) {
            let _e339 = ofactor;
            local_2 = floor(_e339);
        } else {
            let _e341 = ofactor;
            local_2 = ceil(_e341);
        }
        let _e343 = local_2;
        intfactor = _e343;
        let _e344 = ofactor;
        let _e345 = intfactor;
        diff = (_e344 / _e345);
        let _e348 = diff[1u];
        scan = _e348;
        let _e349 = texcoord;
        param = _e349;
        let _e350 = scan;
        param_1 = _e350;
        let _e351 = scan;
        param_2 = _e351;
        let _e352 = Overscan_u0028_vf2_u003b_f1_u003b_f1_u003b((&param), (&param_1), (&param_2));
        texcoord = _e352;
        let _e354 = params.IOS;
        let _e355 = (_e354 == 1f);
        phi_1568_ = _e355;
        if !(_e355) {
            let _e358 = params.IOS;
            phi_1568_ = (_e358 == 3f);
        }
        let _e361 = phi_1568_;
        if _e361 {
            let _e363 = vTexCoord_1[0u];
            let _e365 = texcoord[1u];
            texcoord = vec2<f32>(_e363, _e365);
        }
    }
    let _e368 = params.OS;
    let _e372 = params.BLOOM;
    let _e376 = lum;
    let _e378 = params.BLOOM;
    factor = ((1f + (((1f - (0.5f * _e368)) * _e372) / 100f)) - ((_e376 * _e378) / 100f));
    let _e382 = texcoord;
    param_3 = _e382;
    let _e383 = factor;
    param_4 = _e383;
    let _e384 = factor;
    param_5 = _e384;
    let _e385 = Overscan_u0028_vf2_u003b_f1_u003b_f1_u003b((&param_3), (&param_4), (&param_5));
    texcoord = _e385;
    let _e387 = texcoord[1u];
    let _e389 = global.VShift;
    let _e392 = global.OriginalSize[3u];
    texcoord[1u] = (_e387 - (_e389 * _e392));
    let _e398 = global.OriginalSize[0u];
    let _e400 = global.overscanX;
    let _e404 = global.OriginalSize[0u];
    let _e408 = global.OriginalSize[1u];
    let _e410 = global.overscanY;
    let _e414 = global.OriginalSize[1u];
    let _e416 = texcoord;
    param_6 = _e416;
    param_7 = ((_e398 - _e400) / _e404);
    param_8 = ((_e408 - _e410) / _e414);
    let _e417 = Overscan_u0028_vf2_u003b_f1_u003b_f1_u003b((&param_6), (&param_7), (&param_8));
    texcoord = _e417;
    let _e418 = vTexCoord_1;
    pos1_ = _e418;
    let _e419 = texcoord;
    param_9 = _e419;
    let _e420 = Warp_u0028_vf2_u003b((&param_9));
    pos_6 = _e420;
    let _e421 = vTexCoord_1;
    param_10 = _e421;
    let _e422 = Warp_u0028_vf2_u003b((&param_10));
    pos0_ = _e422;
    let _e423 = pos0_;
    posb = ((_e423 - vec2(0.5f)) * 2f);
    let _e428 = params.BLOOM;
    if (_e428 < 0.5f) {
        let _e430 = posb;
        let _e432 = pos_6;
        let _e438 = posb;
        posb = (max(abs(_e430), abs(((_e432 - vec2(0.5f)) * 2f))) * sign(_e438));
    }
    let _e441 = posb;
    posb = ((_e441 * 0.5f) + vec2(0.5f));
    let _e445 = pos1_;
    let _e446 = ue2_border_linear(Source, _Source_sampler, _e445);
    color = _e446.xyz;
    let _e448 = pos_6;
    let _e449 = ue2_border_linear(BloomPass, _BloomPass_sampler, _e448);
    Bloom = _e449.xyz;
    let _e451 = pos_6;
    let _e452 = ue2_border_linear(GlowPass, _GlowPass_sampler, _e451);
    Glow = _e452.xyz;
    let _e455 = global.deconrr;
    let _e458 = global.deconrry;
    let _e462 = global.deconrg;
    let _e466 = global.deconrgy;
    let _e470 = global.deconrb;
    let _e474 = global.deconrby;
    if ((((((abs(_e455) + abs(_e458)) + abs(_e462)) + abs(_e466)) + abs(_e470)) + abs(_e474)) > 0.2f) {
        let _e478 = color;
        param_11 = _e478;
        let _e479 = Bloom;
        param_12 = _e479;
        let _e480 = Glow;
        param_13 = _e480;
        let _e481 = pos1_;
        param_14 = _e481;
        let _e482 = pos_6;
        param_15 = _e482;
        fetch_pixel_u0028_vf3_u003b_vf3_u003b_vf3_u003b_vf2_u003b_vf2_u003b((&param_11), (&param_12), (&param_13), (&param_14), (&param_15));
        let _e483 = param_11;
        color = _e483;
        let _e484 = param_12;
        Bloom = _e484;
        let _e485 = param_13;
        Glow = _e485;
    }
    let _e487 = color[0u];
    let _e489 = color[1u];
    let _e492 = color[2u];
    param_16 = max(max(_e487, _e489), _e492);
    let _e494 = igc_u0028_f1_u003b((&param_16));
    cm = _e494;
    let _e495 = pos1_;
    let _e496 = ue2_border_linear(Source, _Source_sampler, _e495);
    mx1_ = _e496.w;
    let _e498 = mx1_;
    let _e499 = cm;
    colmx = max(_e498, _e499);
    let _e501 = cm;
    let _e506 = colmx;
    w3_1 = min(((max(((_e501 - 0.0005f) * 1.0005f), 0f) + 0.0001f) / (_e506 + 0.0005f)), 1f);
    let _e510 = interb;
    if _e510 {
        w3_1 = 1f;
    }
    dx_2 = vec2<f32>(0.001f, 0f);
    let _e511 = pos1_;
    let _e512 = dx_2;
    let _e514 = ue2_border_linear(Source, _Source_sampler, (_e511 - _e512));
    mx0_ = _e514.w;
    let _e516 = pos1_;
    let _e517 = dx_2;
    let _e519 = ue2_border_linear(Source, _Source_sampler, (_e516 + _e517));
    mx2_ = _e519.w;
    let _e521 = mx0_;
    let _e522 = mx1_;
    let _e524 = mx2_;
    let _e525 = cm;
    mxg = max(max(_e521, _e522), max(_e524, _e525));
    let _e528 = mxg;
    let _e529 = gamma_in;
    mx_1 = pow(_e528, (1.4f / _e529));
    let _e532 = colmx;
    let _e533 = gamma_in;
    cx = pow(_e532, (1.4f / _e533));
    let _e538 = global.OriginalSize[2u];
    dx_2 = (vec2<f32>(_e538, 0f) * 0.25f);
    let _e541 = pos1_;
    let _e542 = dx_2;
    let _e544 = ue2_border_linear(Source, _Source_sampler, (_e541 - _e542));
    mx0_ = _e544.w;
    let _e546 = pos1_;
    let _e547 = dx_2;
    let _e549 = ue2_border_linear(Source, _Source_sampler, (_e546 + _e547));
    mx2_ = _e549.w;
    let _e551 = mx0_;
    let _e552 = mx2_;
    let _e555 = mx1_;
    mb_1 = (1f - min((abs((_e551 - _e552)) / (0.5f + _e555)), 1f));
    one_1 = vec3<f32>(1f, 1f, 1f);
    let _e560 = color;
    orig1_ = _e560;
    let _e561 = one_1;
    cmask = _e561;
    let _e562 = one_1;
    cmask1_ = _e562;
    let _e563 = one_1;
    cmask2_ = _e563;
    mwidths = array<f32, 15>(2f, 3f, 3f, 6f, 6f, 2.4f, 3.4f, 2.4f, 3.25f, 3.4f, 4.4f, 4.25f, 7.4f, 6.25f, 5.25f);
    let _e565 = params.shadowMask;
    let _e568 = mwidths[i32(_e565)];
    mwidth = _e568;
    let _e569 = mwidth;
    mask_compensate = fract(_e569);
    let _e571 = mwidth;
    mwidth1_ = _e571;
    let _e573 = params.shadowMask;
    if (_e573 > -0.5f) {
        let _e575 = vTexCoord_1;
        let _e577 = global.OutputSize;
        maskcoord = (_e575 * _e577.xy);
        let _e580 = maskcoord;
        scoord = _e580;
        let _e581 = mwidth;
        let _e584 = params.masksize;
        mwidth = (floor(_e581) * _e584);
        let _e586 = mwidth;
        swidth_1 = _e586;
        let _e588 = global.mask_zoom;
        zoomed = (abs(_e588) > 0.75f);
        mscale = 1f;
        let _e591 = maskcoord;
        maskcoord0_ = _e591;
        let _e593 = maskcoord[1u];
        let _e595 = params.masksize;
        maskcoord[1u] = floor((_e593 / _e595));
        let _e599 = mwidth;
        let _e601 = global.mask_zoom;
        mwidth1_ = max((_e599 + _e601), 2f);
        let _e605 = params.mshift;
        if (_e605 > 0.25f) {
            stagg_lvl = 1f;
            let _e608 = params.mshift;
            if (fract(_e608) > 0.25f) {
                stagg_lvl = 2f;
            }
            let _e612 = maskcoord[1u];
            let _e613 = stagg_lvl;
            let _e614 = (2f * _e613);
            let _e620 = stagg_lvl;
            next_line = select(0f, 1f, (floor((_e612 - (floor((_e612 / _e614)) * _e614))) < _e620));
            let _e624 = maskcoord0_[0u];
            let _e625 = next_line;
            let _e627 = mwidth1_;
            maskcoord0_[0u] = (_e624 + ((_e625 * 0.5f) * _e627));
        }
        let _e631 = maskcoord0_;
        let _e633 = params.masksize;
        maskcoord = (_e631 / vec2(_e633));
        let _e636 = zoomed;
        if !(_e636) {
            let _e638 = maskcoord;
            param_17 = floor(_e638);
            let _e640 = mx_1;
            param_18 = _e640;
            let _e641 = mb_1;
            param_19 = _e641;
            let _e642 = Mask_u0028_vf2_u003b_f1_u003b_f1_u003b((&param_17), (&param_18), (&param_19));
            cmask = _e642;
        } else {
            let _e643 = mwidth1_;
            let _e644 = mwidth;
            mscale = (_e643 / _e644);
            let _e647 = maskcoord[0u];
            let _e648 = mscale;
            mlerp = fract((_e647 / _e648));
            let _e652 = global.mzoom_sh;
            if (_e652 > 0.025f) {
                let _e655 = global.mzoom_sh;
                let _e657 = mlerp;
                let _e660 = global.mzoom_sh;
                mlerp = clamp((((1f + _e655) * _e657) - (0.5f * _e660)), 0f, 1f);
            }
            let _e665 = maskcoord[0u];
            let _e666 = mscale;
            mcoord = floor((_e665 / _e666));
            let _e670 = params.shadowMask;
            let _e671 = (_e670 == 12f);
            phi_1983_ = _e671;
            if _e671 {
                let _e673 = global.mask_zoom;
                phi_1983_ = (_e673 == -2f);
            }
            let _e676 = phi_1983_;
            if _e676 {
                let _e678 = maskcoord[0u];
                let _e679 = mscale;
                mcoord = ceil((_e678 / _e679));
            }
            let _e682 = mcoord;
            let _e684 = maskcoord[1u];
            param_20 = vec2<f32>(_e682, _e684);
            let _e686 = mx_1;
            param_21 = _e686;
            let _e687 = mb_1;
            param_22 = _e687;
            let _e688 = Mask_u0028_vf2_u003b_f1_u003b_f1_u003b((&param_20), (&param_21), (&param_22));
            let _e689 = mcoord;
            let _e692 = maskcoord[1u];
            param_23 = vec2<f32>((_e689 + 1f), _e692);
            let _e694 = mx_1;
            param_24 = _e694;
            let _e695 = mb_1;
            param_25 = _e695;
            let _e696 = Mask_u0028_vf2_u003b_f1_u003b_f1_u003b((&param_23), (&param_24), (&param_25));
            let _e697 = mlerp;
            cmask = mix(_e688, _e696, vec3(_e697));
        }
        sm_offset = 0f;
        let _e701 = params.shadowMask;
        let _e702 = (_e701 == 0f);
        phi_2026_ = _e702;
        if !(_e702) {
            let _e705 = params.shadowMask;
            phi_2026_ = (_e705 == 2f);
        }
        let _e708 = phi_2026_;
        phi_2033_ = _e708;
        if !(_e708) {
            let _e711 = params.shadowMask;
            phi_2033_ = (_e711 == 5f);
        }
        let _e714 = phi_2033_;
        phi_2040_ = _e714;
        if !(_e714) {
            let _e717 = params.shadowMask;
            phi_2040_ = (_e717 == 6f);
        }
        let _e720 = phi_2040_;
        phi_2047_ = _e720;
        if !(_e720) {
            let _e723 = params.shadowMask;
            phi_2047_ = (_e723 == 8f);
        }
        let _e726 = phi_2047_;
        phi_2054_ = _e726;
        if !(_e726) {
            let _e729 = params.shadowMask;
            phi_2054_ = (_e729 == 11f);
        }
        let _e732 = phi_2054_;
        bsm_offset = _e732;
        let _e733 = zoomed;
        if _e733 {
            let _e735 = params.mask_layout;
            let _e737 = bsm_offset;
            if ((_e735 < 0.5f) && _e737) {
                sm_offset = 1f;
            } else {
                let _e739 = bsm_offset;
                if _e739 {
                    sm_offset = -1f;
                }
            }
        }
        let _e740 = mwidth1_;
        swidth_1 = round(_e740);
        let _e743 = params.slotwidth;
        if (_e743 > 0.5f) {
            let _e746 = params.slotwidth;
            swidth_1 = _e746;
        }
        smask = 1f;
        let _e747 = scoord;
        let _e748 = sm_offset;
        param_26 = (_e747 + vec2<f32>(_e748, 0f));
        let _e751 = mx_1;
        param_27 = _e751;
        let _e752 = swidth_1;
        param_28 = _e752;
        let _e753 = SlotMask_u0028_vf2_u003b_f1_u003b_f1_u003b((&param_26), (&param_27), (&param_28));
        smask = _e753;
        let _e754 = smask;
        let _e756 = global.smask_mit;
        let _e757 = w3_1;
        let _e758 = colmx;
        smask = clamp((_e754 + mix(_e756, 0f, (_e757 * pow(_e758, 0.3f)))), 0f, 1f);
        let _e764 = cmask;
        cmask2_ = _e764;
        let _e765 = smask;
        let _e766 = cmask;
        cmask = (_e766 * _e765);
        let _e768 = cmask;
        cmask1_ = _e768;
        let _e770 = params.mask_bloom;
        if (abs(_e770) > 0.025f) {
            let _e774 = Bloom[0u];
            let _e776 = Bloom[1u];
            let _e779 = Bloom[2u];
            let _e781 = mxg;
            maxbl = max(max(max(_e774, _e776), _e779), _e781);
            let _e783 = maxbl;
            let _e784 = colmx;
            let _e787 = global.bloom_dist;
            maxbl = (_e783 * max(mix(1f, (2f - _e784), _e787), 0f));
            let _e792 = params.mask_bloom;
            if (_e792 > 0.025f) {
                let _e794 = cmask;
                let _e795 = maxbl;
                let _e797 = params.mask_bloom;
                let _e803 = cmask;
                cmask = max(min((_e794 + vec3((_e795 * _e797))), vec3(1f)), _e803);
            } else {
                let _e805 = cmask;
                let _e806 = cmask;
                let _e807 = maxbl;
                let _e811 = Bloom;
                param_29 = pow(_e811, vec3<f32>(0.35f, 0.35f, 0.35f));
                let _e813 = maxbl;
                param_30 = _e813;
                let _e814 = plant_u0028_vf3_u003b_f1_u003b((&param_29), (&param_30));
                let _e817 = params.mask_bloom;
                let _e821 = cmask;
                cmask = max(mix(_e805, ((_e806 * (1f - (0.5f * _e807))) + _e814), vec3(-(_e817))), _e821);
            }
        }
        let _e823 = color;
        let _e825 = global.mask_gamma;
        let _e826 = gamma_in;
        color = pow(_e823, vec3((_e825 / _e826)));
        let _e830 = color;
        let _e831 = cmask;
        color = (_e830 * _e831);
        let _e833 = color;
        color = min(_e833, vec3(1f));
        let _e836 = color;
        let _e837 = gamma_in;
        let _e839 = global.mask_gamma;
        color = pow(_e836, vec3((_e837 / _e839)));
        let _e843 = cmask;
        cmask = min(_e843, vec3(1f));
        let _e846 = cmask1_;
        cmask1_ = min(_e846, vec3(1f));
    }
    let _e850 = params.mcut;
    let _e852 = params.maskstr;
    let _e853 = mx_1;
    let _e857 = mask_compensate;
    let _e861 = mx_1;
    dark_compensate = mix((max(((clamp(mix(_e850, _e852, _e853), 0f, 1f) - 1f) + _e857), 0f) + 1f), 1f, _e861);
    let _e864 = params.shadowMask;
    if (_e864 < -0.5f) {
        dark_compensate = 1f;
    }
    let _e867 = params.brightboost;
    let _e869 = params.brightboost1_;
    let _e870 = mx_1;
    let _e872 = dark_compensate;
    bb = (mix(_e867, _e869, _e870) * _e872);
    let _e874 = bb;
    let _e875 = color;
    color = (_e875 * _e874);
    let _e877 = pos_6;
    let _e878 = ue2_border_linear(LinearizePass, _LinearizePass_sampler, _e877);
    Ref = _e878.xyz;
    let _e880 = pos_6;
    let _e881 = ue2_border_linear(BloomPass, _BloomPass_sampler, _e880);
    maxb = _e881.w;
    let _e883 = pos_6;
    let _e885 = global.OriginalSize;
    let _e891 = global.OriginalSize;
    let _e897 = ue2_border_linear(PrePass0_, _PrePass0_sampler, clamp(_e883, (vec2(0f) + (_e885.zw * 0.5f)), (vec2(1f) - (_e891.zw * 0.5f))));
    vig = _e897.w;
    let _e900 = params.pr_scan;
    if (_e900 > 0.025f) {
        let _e903 = Bloom[0u];
        let _e905 = Bloom[1u];
        let _e908 = Bloom[2u];
        mbl = max(max(_e903, _e905), _e908);
        let _e910 = Bloom;
        let _e911 = Bloom;
        let _e912 = orig1_;
        param_31 = _e912;
        let _e913 = mbl;
        param_32 = _e913;
        let _e914 = plant_u0028_vf3_u003b_f1_u003b((&param_31), (&param_32));
        let _e915 = w3_1;
        let _e922 = params.pr_scan;
        Bloom = mix(_e910, mix(_e911, _e914, vec3(min((2.5f * (1f - _e915)), 1f))), vec3(min((2f * _e922), 1f)));
    }
    let _e927 = Bloom;
    Bloom1_ = _e927;
    let _e928 = one_1;
    let _e929 = cmask1_;
    let _e931 = global.bmask1_;
    bcmask = mix(_e928, _e929, vec3(_e931));
    let _e934 = one_1;
    let _e935 = cmask1_;
    let _e937 = global.hmask1_;
    hcmask = mix(_e934, _e935, vec3(_e937));
    let _e941 = global.bloom;
    if (abs(_e941) > 0.025f) {
        let _e945 = global.bloom;
        if (_e945 < -0.01f) {
            let _e947 = Bloom;
            param_33 = _e947;
            let _e948 = maxb;
            param_34 = _e948;
            let _e949 = plant_u0028_vf3_u003b_f1_u003b((&param_33), (&param_34));
            Bloom1_ = _e949;
        }
        let _e950 = Bloom1_;
        let _e951 = orig1_;
        let _e952 = color;
        let _e955 = colmx;
        let _e956 = orig1_;
        let _e959 = color;
        let _e962 = Bloom1_;
        Bloom1_ = min((_e950 * (_e951 + _e952)), max((((vec3(_e955) + _e956) - _e959) * 0.5f), (_e962 * 0.001f)));
        let _e966 = Bloom1_;
        let _e967 = Bloom1_;
        let _e968 = colmx;
        let _e969 = orig1_;
        let _e971 = Bloom1_;
        let _e974 = color;
        Bloom1_ = ((_e966 + mix(_e967, mix((_e969 * _e968), _e971, vec3(0.5f)), (vec3(1f) - _e974))) * 0.5f);
        let _e980 = bcmask;
        let _e981 = Bloom1_;
        let _e983 = colmx;
        let _e986 = global.bloom_dist;
        Bloom1_ = ((_e980 * _e981) * max(mix(1f, (2f - _e983), _e986), 0f));
        let _e990 = color;
        let _e992 = global.mask_gamma;
        let _e993 = gamma_in;
        let _e998 = global.bloom;
        let _e1000 = Bloom1_;
        let _e1002 = global.mask_gamma;
        let _e1003 = gamma_in;
        let _e1009 = gamma_in;
        let _e1011 = global.mask_gamma;
        color = pow((pow(_e990, vec3((_e992 / _e993))) + (pow(_e1000, vec3((_e1002 / _e1003))) * abs(_e998))), vec3((_e1009 / _e1011)));
    }
    let _e1016 = global.halation;
    if (_e1016 > 0.01f) {
        let _e1018 = Bloom;
        let _e1019 = Bloom;
        let _e1020 = Bloom;
        Bloom = ((_e1018 + (_e1019 * _e1020)) * 0.5f);
        let _e1025 = Bloom[0u];
        let _e1027 = Bloom[1u];
        let _e1030 = Bloom[2u];
        mbl_1 = max(max(_e1025, _e1027), _e1030);
        let _e1032 = colmx;
        let _e1033 = colmx;
        let _e1034 = colmx;
        cmxh = (0.5f * (_e1032 + (_e1033 * _e1034)));
        let _e1038 = cmxh;
        let _e1039 = cmxh;
        let _e1040 = mbl_1;
        let _e1041 = mbl_1;
        let _e1043 = colmx;
        let _e1045 = mbl_1;
        let _e1046 = mb_1;
        mbl_1 = mix(mix(_e1038, mix(_e1039, _e1040, _e1041), _e1043), _e1045, _e1046);
        let _e1048 = mbl_1;
        let _e1049 = cmxh;
        let _e1052 = mbl_1;
        let _e1053 = colmx;
        let _e1057 = cmxh;
        let _e1060 = colmx;
        let _e1063 = Bloom;
        param_35 = _e1063;
        param_36 = mix(sqrt((_e1048 * _e1049)), max((_e1052 - (0.15f * (1f - _e1053))), (0.4f * _e1057)), pow(_e1060, 0.25f));
        let _e1064 = plant_u0028_vf3_u003b_f1_u003b((&param_35), (&param_36));
        let _e1065 = colmx;
        Bloom = (_e1064 * mix(0.425f, 1f, _e1065));
        let _e1068 = colmx;
        let _e1070 = color;
        let _e1073 = orig1_;
        let _e1074 = w3_1;
        let _e1079 = w3_1;
        param_37 = (vec3(0.325f) + (_e1073 / vec3(_e1074)));
        param_38 = (0.5f * (1f + _e1079));
        let _e1082 = plant_u0028_vf3_u003b_f1_u003b((&param_37), (&param_38));
        let _e1084 = hcmask;
        let _e1086 = Bloom;
        Bloom = ((((vec3((3f - _e1068)) - _e1070) * _e1082) * _e1084) * _e1086);
        let _e1088 = color;
        let _e1090 = global.mask_gamma;
        let _e1091 = gamma_in;
        let _e1096 = global.halation;
        let _e1097 = Bloom;
        let _e1099 = global.mask_gamma;
        let _e1100 = gamma_in;
        let _e1106 = gamma_in;
        let _e1108 = global.mask_gamma;
        color = pow((pow(_e1088, vec3((_e1090 / _e1091))) + (pow(_e1097, vec3((_e1099 / _e1100))) * _e1096)), vec3((_e1106 / _e1108)));
    } else {
        let _e1113 = global.halation;
        if (_e1113 < -0.01f) {
            let _e1116 = Bloom[0u];
            let _e1118 = Bloom[1u];
            let _e1121 = Bloom[2u];
            mbl_2 = max(max(_e1116, _e1118), _e1121);
            let _e1123 = Bloom;
            let _e1124 = Ref;
            let _e1126 = orig1_;
            let _e1128 = Bloom;
            let _e1129 = Bloom;
            let _e1131 = Bloom;
            let _e1134 = mbl_2;
            let _e1135 = mbl_2;
            param_39 = (((_e1123 + _e1124) + _e1126) + ((_e1128 * _e1129) * _e1131));
            param_40 = min((_e1134 * _e1135), 0.75f);
            let _e1138 = plant_u0028_vf3_u003b_f1_u003b((&param_39), (&param_40));
            Bloom = _e1138;
            let _e1139 = w3_1;
            let _e1140 = colmx;
            let _e1144 = hcmask;
            let _e1146 = Bloom;
            Bloom = ((_e1144 * (2f * mix(1f, _e1139, (0.5f * _e1140)))) * _e1146);
            let _e1148 = color;
            let _e1150 = global.halation;
            let _e1151 = Bloom;
            color = (_e1148 - (_e1151 * _e1150));
        }
    }
    let _e1154 = color;
    color = min(_e1154, vec3(1f));
    let _e1157 = color;
    param_41 = _e1157;
    let _e1158 = w3_1;
    param_42 = _e1158;
    let _e1159 = gc2_u0028_vf3_u003b_f1_u003b((&param_41), (&param_42));
    color = _e1159;
    let _e1161 = global.smoothmask;
    if (_e1161 > 0.125f) {
        let _e1163 = w3_1;
        let _e1165 = global.smoothmask;
        w4_ = pow(_e1163, (0.425f + (0.3f * _e1165)));
        let _e1169 = w4_;
        let _e1170 = colmx;
        let _e1173 = global.smoothmask;
        w4_ = max((_e1169 - ((0.175f * _e1170) * _e1173)), 0.2f);
        let _e1177 = color;
        let _e1178 = w4_;
        let _e1181 = colmx;
        let _e1184 = global.smoothmask;
        let _e1187 = orig1_;
        param_43 = _e1187;
        param_44 = (1f + ((0.175f * _e1181) * _e1184));
        let _e1188 = plant_u0028_vf3_u003b_f1_u003b((&param_43), (&param_44));
        let _e1190 = w4_;
        let _e1192 = color;
        let _e1193 = w4_;
        color = mix((min((_e1177 / vec3(_e1178)), _e1188) * _e1190), _e1192, vec3(_e1193));
    }
    let _e1197 = global.m_glow;
    if (_e1197 < 0.5f) {
        let _e1199 = Glow;
        let _e1200 = color;
        let _e1202 = colmx;
        Glow = mix(_e1199, (_e1200 * 0.25f), vec3(_e1202));
    } else {
        let _e1206 = Glow[0u];
        let _e1208 = Glow[1u];
        let _e1211 = Glow[2u];
        maxb = max(max(_e1206, _e1208), _e1211);
        let _e1213 = orig1_;
        let _e1214 = Ref;
        param_45 = (_e1213 + (_e1214 * 0.001f));
        param_46 = 1f;
        let _e1217 = plant_u0028_vf3_u003b_f1_u003b((&param_45), (&param_46));
        orig2_ = _e1217;
        let _e1218 = Glow;
        param_47 = _e1218;
        param_48 = 1f;
        let _e1219 = plant_u0028_vf3_u003b_f1_u003b((&param_47), (&param_48));
        Bloom = _e1219;
        let _e1220 = orig2_;
        let _e1221 = Bloom;
        Ref = abs((_e1220 - _e1221));
        let _e1225 = orig2_[0u];
        let _e1227 = orig2_[1u];
        let _e1230 = orig2_[2u];
        let _e1233 = orig2_[0u];
        let _e1235 = orig2_[1u];
        let _e1238 = orig2_[2u];
        mx0_ = (max(max(_e1225, _e1227), _e1230) - min(min(_e1233, _e1235), _e1238));
        let _e1242 = Bloom[0u];
        let _e1244 = Bloom[1u];
        let _e1247 = Bloom[2u];
        let _e1250 = Bloom[0u];
        let _e1252 = Bloom[1u];
        let _e1255 = Bloom[2u];
        mx2_ = (max(max(_e1242, _e1244), _e1247) - min(min(_e1250, _e1252), _e1255));
        let _e1258 = maxb;
        let _e1259 = Bloom;
        let _e1260 = orig2_;
        let _e1263 = Glow;
        let _e1265 = Ref[0u];
        let _e1267 = Ref[1u];
        let _e1270 = Ref[2u];
        let _e1272 = Glow;
        let _e1274 = mx_1;
        let _e1275 = mx0_;
        let _e1279 = color;
        let _e1280 = Glow;
        let _e1281 = mx2_;
        let _e1284 = mx0_;
        let _e1285 = mx2_;
        let _e1287 = Ref;
        let _e1290 = mx0_;
        let _e1292 = mx2_;
        Bloom = mix((min(_e1259, _e1260) * _e1258), mix(mix(_e1263, (_e1272 * max(max(_e1265, _e1267), _e1270)), vec3(max(_e1274, _e1275))), mix(_e1279, _e1280, vec3(_e1281)), (_e1287 * max(_e1284, _e1285))), vec3(min(sqrt(((1.1f - _e1290) * (0.1f + _e1292))), 1f)));
        let _e1300 = global.m_glow;
        if (_e1300 > 1.5f) {
            let _e1302 = Glow;
            let _e1304 = Glow;
            let _e1306 = Bloom;
            let _e1307 = Bloom;
            Glow = mix(((_e1302 * 0.5f) * _e1304), _e1306, _e1307);
        }
        let _e1310 = global.m_glow_low;
        let _e1311 = Glow;
        let _e1314 = global.m_glow_high;
        let _e1315 = Bloom;
        let _e1317 = colmx;
        let _e1319 = global.m_glow_dist;
        let _e1320 = gamma_in;
        Glow = mix((_e1311 * _e1310), (_e1315 * _e1314), vec3(pow(_e1317, (_e1319 / _e1320))));
    }
    let _e1326 = global.m_glow;
    if (_e1326 < 0.5f) {
        let _e1329 = params.glow;
        if (_e1329 >= 0f) {
            let _e1331 = color;
            let _e1332 = Glow;
            let _e1335 = params.glow;
            color = (_e1331 + ((_e1332 * 0.5f) * _e1335));
        } else {
            let _e1338 = color;
            let _e1340 = params.glow;
            let _e1342 = cmask2_;
            let _e1343 = cmask2_;
            let _e1348 = Glow;
            color = (_e1338 + ((min((_e1342 * _e1343), vec3(1f)) * abs(_e1340)) * _e1348));
        }
    } else {
        let _e1351 = one_1;
        let _e1352 = cmask1_;
        let _e1354 = global.m_glow_mask;
        cmaskg = clamp(mix(_e1351, _e1352, vec3(_e1354)), vec3(0f), vec3(1f));
        let _e1360 = color;
        let _e1362 = params.glow;
        let _e1364 = cmaskg;
        let _e1366 = Glow;
        color = (_e1360 + ((_e1364 * abs(_e1362)) * _e1366));
    }
    let _e1369 = color;
    color = min(_e1369, vec3(1f));
    let _e1373 = global.edgemask;
    if (_e1373 > 0.05f) {
        let _e1375 = pos1_;
        let _e1376 = dx_2;
        let _e1378 = ue2_border_linear(Source, _Source_sampler, (_e1375 - _e1376));
        mx0_ = _e1378.w;
        let _e1380 = pos1_;
        let _e1381 = dx_2;
        let _e1382 = mx0_;
        let _e1388 = ue2_border_linear(Source, _Source_sampler, (_e1380 - (_e1381 * (1f - (0.75f * sqrt(_e1382))))));
        mx0_ = _e1388.w;
        let _e1390 = pos1_;
        let _e1391 = dx_2;
        let _e1393 = ue2_border_linear(Source, _Source_sampler, (_e1390 + _e1391));
        mx2_ = _e1393.w;
        let _e1395 = pos1_;
        let _e1396 = dx_2;
        let _e1397 = mx2_;
        let _e1403 = ue2_border_linear(Source, _Source_sampler, (_e1395 + (_e1396 * (1f - (0.75f * sqrt(_e1397))))));
        mx2_ = _e1403.w;
        let _e1405 = pos1_;
        let _e1406 = dx_2;
        let _e1409 = ue2_border_linear(Source, _Source_sampler, (_e1405 - (_e1406 * 4f)));
        mx3_ = _e1409.w;
        let _e1411 = pos1_;
        let _e1412 = dx_2;
        let _e1415 = ue2_border_linear(Source, _Source_sampler, (_e1411 + (_e1412 * 4f)));
        mx4_ = _e1415.w;
        let _e1417 = mx3_;
        let _e1418 = mx4_;
        let _e1421 = cx;
        let _e1425 = mx3_;
        let _e1426 = mx4_;
        let _e1428 = cx;
        mx4_ = max(pow(abs((_e1417 - _e1418)), (0.55f - (0.4f * _e1421))), min((max(_e1425, _e1426) / min((0.1f + _e1428), 1f)), 1f));
        let _e1434 = mx0_;
        let _e1435 = mx2_;
        let _e1439 = mx2_;
        let _e1440 = mx0_;
        mb_1 = (1f - abs((pow(_e1434, (1f - (0.65f * _e1435))) - pow(_e1439, (1f - (0.65f * _e1440))))));
        let _e1447 = mx4_;
        let _e1449 = global.edgemask;
        let _e1451 = mb_1;
        let _e1452 = mb_1;
        mb_1 = ((_e1447 * _e1449) * (1.0001f - (_e1451 * _e1452)));
        let _e1456 = color;
        let _e1457 = orig1_;
        let _e1458 = mb_1;
        temp = mix(_e1456, _e1457, vec3(_e1458));
        let _e1461 = temp;
        let _e1462 = mb_1;
        let _e1464 = temp;
        let _e1466 = temp;
        let _e1467 = cx;
        let _e1471 = color;
        let _e1472 = colmx;
        let _e1479 = color;
        color = max((_e1461 + mix((mix((_e1464 * 1.625f), _e1466, vec3(_e1467)) * (3.5f * _e1462)), vec3<f32>(0f, 0f, 0f), pow(_e1471, (vec3<f32>(0.75f, 0.75f, 0.75f) - vec3((0.5f * _e1472)))))), _e1479);
    }
    let _e1481 = color;
    let _e1482 = w3_1;
    let _e1485 = w3_1;
    let _e1486 = mx_1;
    let _e1489 = params.pr_scan;
    color = (_e1481 * mix(1f, mix((0.5f * (1f + _e1482)), _e1485, _e1486), _e1489));
    let _e1492 = color;
    let _e1493 = orig1_;
    let _e1494 = color;
    let _e1496 = one_1;
    let _e1497 = cmask1_;
    let _e1499 = global.mclip;
    color = min(_e1492, (max(_e1493, _e1494) * mix(_e1496, _e1497, vec3(_e1499))));
    let _e1504 = color;
    let _e1506 = global.gamma_out;
    color = pow(_e1504, vec3((1f / _e1506)));
    let _e1511 = color[0u];
    let _e1513 = color[1u];
    let _e1516 = color[2u];
    rc_1 = ((0.6f * sqrt(max(max(_e1511, _e1513), _e1516))) + 0.4f);
    let _e1522 = global.addnoised;
    if (abs(_e1522) > 0.01f) {
        let _e1526 = global.OutputSize;
        let _e1528 = vTexCoord_1;
        let _e1531 = global.noiseresd;
        let _e1534 = floor(((_e1526.xy * _e1528) / vec2(_e1531)));
        let _e1536 = global.FrameCount;
        param_49 = vec3<f32>(_e1534.x, _e1534.y, f32(_e1536));
        let _e1541 = noise_u0028_vf3_u003b((&param_49));
        noise0_ = _e1541;
        let _e1543 = global.noisetype;
        if (_e1543 < 0.5f) {
            let _e1545 = color;
            let _e1546 = noise0_;
            let _e1548 = global.addnoised;
            let _e1551 = rc_1;
            color = mix(_e1545, _e1546, vec3(((0.25f * abs(_e1548)) * _e1551)));
        } else {
            let _e1555 = color;
            let _e1557 = noise0_[0u];
            let _e1560 = global.addnoised;
            color = min((_e1555 * mix(1f, (1.5f * _e1557), (0.5f * abs(_e1560)))), vec3(1f));
        }
    }
    let _e1568 = orig1_[0u];
    let _e1570 = orig1_[1u];
    let _e1573 = orig1_[2u];
    colmx = max(max(_e1568, _e1570), _e1573);
    let _e1575 = color;
    let _e1577 = global.bmask;
    let _e1578 = cmask2_;
    let _e1579 = colmx;
    let _e1582 = color;
    let _e1584 = colmx;
    color = (_e1575 + (mix(_e1578, (_e1582 * (0.125f * (1f - _e1579))), vec3(min((20f * _e1584), 1f))) * _e1577));
    let _e1591 = color;
    let _e1592 = vig;
    let _e1595 = pos_6[1u];
    let _e1597 = pos_6[0u];
    let _e1599 = global.bardir;
    param_50 = mix(_e1595, _e1597, _e1599);
    let _e1601 = humbar_u0028_f1_u003b((&param_50));
    let _e1604 = global.post_br;
    let _e1606 = posb;
    param_51 = _e1606;
    let _e1607 = corner_u0028_vf2_u003b((&param_51));
    let _e1608 = ((((_e1591 * _e1592) * _e1601) * _e1604) * _e1607);
    let _e1613 = pos1_;
    let _e1615 = global.OriginalSize;
    let _e1622 = global.OriginalSize;
    let _e1625 = ue2_border_nearest(StockPass, _StockPass_sampler, ((floor((_e1613 * _e1615.xy)) + vec2(0.5f)) * _e1622.zw));
    let _e1627 = pos1_[0u];
    let _e1629 = global.oimage;
    FragColor = mix(vec4<f32>(_e1608.x, _e1608.y, _e1608.z, 1f), _e1625, vec4(step(_e1627, (_e1629 - 0.00025f))));
    return;
}

@fragment 
fn main(@location(0) vTexCoord: vec2<f32>) -> @location(0) vec4<f32> {
    vTexCoord_1 = vTexCoord;
    main_1();
    let _e3 = FragColor;
    return _e3;
}

@group(0) @binding(10) var ue2_border_coverage:texture_2d<f32>;
@group(1) @binding(10) var ue2_border_coverage_sampler:sampler;

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
