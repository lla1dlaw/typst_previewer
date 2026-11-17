// Uniforms: NDC rect for the current page quad
// rect.x, rect.y = top-left corner in NDC (y points up)
// rect.z, rect.w = width, height in NDC units
struct Uniforms {
    rect: vec4<f32>,
};

@group(0) @binding(0) var<uniform> uniforms: Uniforms;
@group(0) @binding(1) var page_tex: texture_2d<f32>;
@group(0) @binding(2) var page_sampler: sampler;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> VsOut {
    // 6 vertices for 2 triangles forming a quad
    // local_pos in [0,1]x[0,1], (0,0) = top-left
    var positions = array<vec2<f32>, 6>(
        vec2(0.0, 0.0), vec2(1.0, 0.0), vec2(0.0, 1.0),
        vec2(0.0, 1.0), vec2(1.0, 0.0), vec2(1.0, 1.0),
    );

    let local_pos = positions[vi];

    var out: VsOut;
    // UV maps directly: (0,0) = top-left of texture
    out.uv = local_pos;

    // Transform to NDC
    // X: left edge at rect.x, right edge at rect.x + rect.z
    // Y: top edge at rect.y, bottom edge at rect.y - rect.w (NDC Y goes up)
    let ndc_x = uniforms.rect.x + local_pos.x * uniforms.rect.z;
    let ndc_y = uniforms.rect.y - local_pos.y * uniforms.rect.w;
    out.pos = vec4(ndc_x, ndc_y, 0.0, 1.0);

    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    return textureSample(page_tex, page_sampler, in.uv);
}
