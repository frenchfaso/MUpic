diagnostic(off, derivative_uniformity);
struct Push {
    SourceSize: vec4<f32>,
    OriginalSize: vec4<f32>,
    OutputSize: vec4<f32>,
    FrameCount: u32,
    SIZEVB: f32,
    SIGMA_VB: f32,
    FINE_BLOOM: f32,
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

var<private> FINE_BLOOM: f32;
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

    let _e14 = params.FINE_BLOOM;
    if (_e14 > 0.5f) {
        let _e17 = params.FINE_BLOOM;
        local = _e17;
    } else {
        let _e19 = params.FINE_BLOOM;
        local = mix(0.75f, 0.5f, -(_e19));
    }
    let _e22 = local;
    FINE_BLOOM = _e22;
    let _e24 = global.MVP;
    let _e25 = Position_1;
    unnamed.gl_Position = (_e24 * _e25);
    let _e28 = TexCoord_1;
    vTexCoord = _e28;
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
