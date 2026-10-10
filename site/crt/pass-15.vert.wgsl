diagnostic(off, derivative_uniformity);
struct Push {
    LinearizePassSize: vec4<f32>,
    OriginalSize: vec4<f32>,
    OutputSize: vec4<f32>,
    FrameCount: u32,
    SIZEHB: f32,
    SIGMA_HB: f32,
    FINE_BLOOM: f32,
    auto_res: f32,
    GLOW_MAX: f32,
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
var<private> FINE_BLOOM: f32;
var<private> unnamed: gl_PerVertex = gl_PerVertex(vec4<f32>(0f, 0f, 0f, 1f), 1f, array<f32, 1>(), array<f32, 1>());
@group(0) @binding(0) 
var<uniform> global: UBO;
var<private> Position_1: vec4<f32>;
var<private> vTexCoord: vec2<f32>;
var<private> TexCoord_1: vec2<f32>;

fn main_1() {
    var local: f32;
    var local_1: f32;

    let _e25 = params.OriginalSize[1u];
    if (_e25 < 375f) {
        let _e28 = params.auto_res;
        let _e31 = params.OriginalSize[0u];
        local = mix(1f, 2f, clamp(((_e28 * round((_e31 / 300f))) - 1f), 0f, 1f));
    } else {
        local = 1f;
    }
    let _e38 = local;
    auto_rez = _e38;
    let _e40 = params.FINE_BLOOM;
    if (_e40 > 0.5f) {
        let _e43 = params.FINE_BLOOM;
        local_1 = _e43;
    } else {
        let _e45 = params.FINE_BLOOM;
        local_1 = mix(0.75f, 0.5f, -(_e45));
    }
    let _e48 = local_1;
    FINE_BLOOM = _e48;
    let _e50 = global.MVP;
    let _e51 = Position_1;
    unnamed.gl_Position = (_e50 * _e51);
    let _e54 = TexCoord_1;
    vTexCoord = _e54;
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
