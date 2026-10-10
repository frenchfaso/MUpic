diagnostic(off, derivative_uniformity);
struct gl_PerVertex {
    @builtin(position) gl_Position: vec4<f32>,
    gl_PointSize: f32,
    gl_ClipDistance: array<f32, 1>,
    gl_CullDistance: array<f32, 1>,
}

struct UBO {
    MVP: mat4x4<f32>,
}

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
@group(0) @binding(8) 
var<uniform> params: Push;

fn main_1() {
    let _e9 = global.MVP;
    let _e10 = Position_1;
    unnamed.gl_Position = (_e9 * _e10);
    let _e13 = TexCoord_1;
    vTexCoord = _e13;
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
