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

var<private> auto_rez: f32;
@group(0) @binding(3) 
var<uniform> params: Push;
var<private> unnamed: gl_PerVertex = gl_PerVertex(vec4<f32>(0f, 0f, 0f, 1f), 1f, array<f32, 1>(), array<f32, 1>());
@group(0) @binding(0) 
var<uniform> global: UBO;
var<private> Position_1: vec4<f32>;
var<private> vTexCoord: vec2<f32>;
var<private> TexCoord_1: vec2<f32>;

fn main_1() {
    var local: f32;

    let _e21 = params.OriginalSize[1u];
    if (_e21 < 375f) {
        let _e24 = params.auto_res;
        let _e27 = params.OriginalSize[0u];
        local = mix(1f, 1.85f, clamp(((_e24 * round((_e27 / 300f))) - 1f), 0f, 1f));
    } else {
        local = 1f;
    }
    let _e34 = local;
    auto_rez = _e34;
    let _e36 = global.MVP;
    let _e37 = Position_1;
    unnamed.gl_Position = (_e36 * _e37);
    let _e40 = TexCoord_1;
    vTexCoord = (_e40 * 1.00001f);
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
