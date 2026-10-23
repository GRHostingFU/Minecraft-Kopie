// Voxel fragment shader: tiles the procedural atlas across greedy-meshed quads,
// applies per-vertex ambient occlusion, directional face shading, sky light,
// day/night brightness and distance fog.
//
// Vertex colour layout (see rendering/mesher.rs):
//   r = ambient occlusion, g = sky light, b = atlas tile index, a = face shade.

import bevy_pbr::render::forward_io::VertexOutput;
import bevy_pbr::render::mesh_view_bindings::view;


@group(3) @binding(0) var<uniform> fog: vec4<f32>;
@group(3) @binding(1) var<uniform> params: vec4<f32>;
@group(3) @binding(2) var atlas: texture_2d<f32>;
@group(3) @binding(3) var atlas_sampler: sampler;

const ATLAS_COLS: f32 = 4.0;
const ATLAS_ROWS: f32 = 4.0;
const TEXEL_INSET: f32 = 0.002;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let ao = in.color.r;
    let sky = in.color.g;
    let tile = floor(in.color.b + 0.5);
    let face_shade = in.color.a;

    let col = tile - floor(tile / ATLAS_COLS) * ATLAS_COLS;
    let row = floor(tile / ATLAS_COLS);
    // Repeat the tile across merged quads; image rows grow downwards.
    let f = fract(in.uv);
    let local = vec2<f32>(
        clamp(f.x, TEXEL_INSET, 1.0 - TEXEL_INSET),
        clamp(1.0 - f.y, TEXEL_INSET, 1.0 - TEXEL_INSET),
    );
    let uv = (vec2<f32>(col, row) + local) / vec2<f32>(ATLAS_COLS, ATLAS_ROWS);
    let texel = textureSampleLevel(atlas, atlas_sampler, uv, 0.0);

    let daylight = params.x;
    // Sky light only matters in daylight; at night everything is dim, lit areas slightly less.
    let sky_term = mix(0.18, 1.0, sky);
    let light = mix(0.16, 1.0, daylight * sky_term) * ao * face_shade;
    let night_tint = mix(vec3<f32>(0.55, 0.62, 1.0), vec3<f32>(1.0), daylight);
    var rgb = texel.rgb * light * night_tint;

    let dist = distance(in.world_position.xyz, view.world_position);
    let fog_amount = clamp((dist - fog.a) / max(params.y - fog.a, 1.0), 0.0, 1.0);
    rgb = mix(rgb, fog.rgb, fog_amount * fog_amount);

    return vec4<f32>(rgb, texel.a * params.z);
}
