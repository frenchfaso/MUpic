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

struct gl_PerVertex {
    @builtin(position) gl_Position: vec4<f32>,
    gl_PointSize: f32,
    gl_ClipDistance: array<f32, 1>,
    gl_CullDistance: array<f32, 1>,
}

struct UBO {
    MVP: mat4x4<f32>,
}

struct VertexOutput {
    @builtin(position) gl_Position: vec4<f32>,
    @location(0) member: vec2<f32>,
}

var<private> RFNOISE: f32;
@group(0) @binding(3) 
var<uniform> params: Push;
var<private> unnamed: gl_PerVertex = gl_PerVertex(vec4<f32>(0f, 0f, 0f, 1f), 1f, array<f32, 1>(), array<f32, 1>());
@group(0) @binding(0) 
var<uniform> global: UBO;
var<private> Position_1: vec4<f32>;
var<private> vTexCoord: vec2<f32>;
var<private> TexCoord_1: vec2<f32>;

fn main_1() {
    let _e16 = params.RFNOISE;
    let _e18 = params.RFNOISE;
    RFNOISE = ((_e16 * _e18) + 0.00001f);
    let _e22 = global.MVP;
    let _e23 = Position_1;
    unnamed.gl_Position = (_e22 * _e23);
    let _e26 = TexCoord_1;
    let _e29 = params.OriginalSize[2u];
    vTexCoord = (_e26 + vec2<f32>((0.125f * _e29), 0f));
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
