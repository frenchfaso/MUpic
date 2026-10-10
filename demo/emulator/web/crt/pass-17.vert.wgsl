diagnostic(off, derivative_uniformity);
struct gl_PerVertex {
    @builtin(position) gl_Position: vec4<f32>,
    gl_PointSize: f32,
    gl_ClipDistance: array<f32, 1>,
    gl_CullDistance: array<f32, 1>,
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

struct VertexOutput {
    @builtin(position) gl_Position: vec4<f32>,
    @location(0) member: vec2<f32>,
}

var<private> unnamed: gl_PerVertex = gl_PerVertex(vec4<f32>(0f, 0f, 0f, 1f), 1f, array<f32, 1>(), array<f32, 1>());
@group(0) @binding(0) 
var<uniform> global: UBO;
var<private> Position_1: vec4<f32>;
var<private> vTexCoord: vec2<f32>;
var<private> TexCoord_1: vec2<f32>;
@group(0) @binding(5) 
var<uniform> params: Push;

fn main_1() {
    let _e10 = global.MVP;
    let _e11 = Position_1;
    unnamed.gl_Position = (_e10 * _e11);
    let _e14 = TexCoord_1;
    vTexCoord = (_e14 * 1.00001f);
    return;
}

@vertex 
fn main(@location(0) Position: vec4<f32>, @location(1) TexCoord: vec2<f32>) -> VertexOutput {
    Position_1 = Position;
    TexCoord_1 = TexCoord;
    main_1();
    let _e8 = unnamed.gl_Position.y;
    unnamed.gl_Position.y = -(_e8);
    let _e10 = unnamed.gl_Position;
    let _e11 = vTexCoord;
    return VertexOutput(_e10, _e11);
}
