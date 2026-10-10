diagnostic(off, derivative_uniformity);
struct Push {
    SourceSize: vec4<f32>,
    OriginalSize: vec4<f32>,
    OutputSize: vec4<f32>,
    FrameCount: u32,
    BRIGHTNESS: f32,
    SATURATION: f32,
    ARTIFACTING: f32,
    FRINGING: f32,
    RFNOISE: f32,
    RFNOISE1_: f32,
    RFNOISE2_: f32,
    PHMOD: f32,
    DLINE: f32,
    GCORR: f32,
    PAL_FILT: f32,
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

var<private> mmat: mat3x3<f32>;
@group(0) @binding(4) 
var<uniform> params: Push;
var<private> unnamed: gl_PerVertex = gl_PerVertex(vec4<f32>(0f, 0f, 0f, 1f), 1f, array<f32, 1>(), array<f32, 1>());
@group(0) @binding(0) 
var<uniform> global: UBO;
var<private> Position_1: vec4<f32>;
var<private> vTexCoord: vec2<f32>;
var<private> TexCoord_1: vec2<f32>;

fn main_1() {
    let _e33 = params.BRIGHTNESS;
    let _e35 = params.FRINGING;
    let _e37 = params.FRINGING;
    let _e39 = params.ARTIFACTING;
    let _e41 = params.SATURATION;
    let _e44 = params.ARTIFACTING;
    let _e46 = params.SATURATION;
    mmat = mat3x3<f32>(vec3<f32>(_e33, _e35, _e37), vec3<f32>(_e39, (2f * _e41), 0f), vec3<f32>(_e44, 0f, (2f * _e46)));
    let _e53 = global.MVP;
    let _e54 = Position_1;
    unnamed.gl_Position = (_e53 * _e54);
    let _e57 = TexCoord_1;
    vTexCoord = (_e57 * 1.00001f);
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
