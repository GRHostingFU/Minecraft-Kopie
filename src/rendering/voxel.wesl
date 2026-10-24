// Voxel fragment shader: tiles the procedural atlas across greedy-meshed quads
// with mip-aware filtering, applies per-vertex ambient occlusion, directional face
// shading, sky light, a warm twilight tint, day/night brightness and distance fog.
//
// Vertex colour layout (see rendering/mesher.rs):
//   r = ambient occlusion, g = sky light, b = atlas tile index (+64 = emissive: lava, lit lamps,
//   powered redstone, drawn independent of daylight), a = face shade.
//
// Uniforms:
//   fog    = (rgb fog / horizon colour, fog start distance)
//   params = (daylight 0..1, fog end distance, alpha, twilight 0..1)

import bevy_pbr::render::forward_io::VertexOutput;
import bevy_pbr::render::mesh_view_bindings::view;


@group(3) @binding(0) var<uniform> fog: vec4<f32>;
@group(3) @binding(1) var<uniform> params: vec4<f32>;
@group(3) @binding(2) var atlas: texture_2d<f32>;
@group(3) @binding(3) var atlas_sampler: sampler;

// Must match rendering/atlas.rs.
const ATLAS_COLS: f32 = 8.0;
const ATLAS_ROWS: f32 = 4.0;
const TILE_PX: f32 = 32.0;
const MAX_LOD: f32 = 3.0;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let ao = in.color.r;
    let sky = in.color.g;
    let raw_tile = floor(in.color.b + 0.5);
    let emissive = raw_tile >= 64.0;
    let tile = select(raw_tile, raw_tile - 64.0, emissive);
    let face_shade = in.color.a;

    let col = tile - floor(tile / ATLAS_COLS) * ATLAS_COLS;
    let row = floor(tile / ATLAS_COLS);

    // The tile repeats across merged quads via fract(), which breaks the hardware's automatic
    // mip selection at every tile border. Derive the gradients from the continuous uv instead.
    let ddx_t = dpdx(in.uv);
    let ddy_t = dpdy(in.uv);
    let rho = max(length(ddx_t), length(ddy_t)) * TILE_PX;
    let lod = clamp(log2(max(rho, 1.0)), 0.0, MAX_LOD);
    // Keep bilinear taps inside the tile: half a texel of the coarser mip level.
    let inset = 0.5 * exp2(min(ceil(lod), MAX_LOD)) / TILE_PX;

    // Image rows grow downwards, uv.y grows upwards.
    let f = fract(in.uv);
    let local = vec2<f32>(
        clamp(f.x, inset, 1.0 - inset),
        clamp(1.0 - f.y, inset, 1.0 - inset),
    );
    let cell = vec2<f32>(1.0 / ATLAS_COLS, 1.0 / ATLAS_ROWS);
    let uv = (vec2<f32>(col, row) + local) * cell;
    let gx = vec2<f32>(ddx_t.x, -ddx_t.y) * cell;
    let gy = vec2<f32>(ddy_t.x, -ddy_t.y) * cell;
    let texel = textureSampleGrad(atlas, atlas_sampler, uv, gx, gy);

    let daylight = params.x;
    let twilight = params.w;
    // Sky light only matters in daylight; at night everything is dim, lit areas slightly less.
    let sky_term = mix(0.18, 1.0, sky);
    let light = mix(0.16, 1.0, daylight * sky_term) * ao * face_shade;
    let night_tint = mix(vec3<f32>(0.55, 0.62, 1.0), vec3<f32>(1.0), daylight);
    // Low sun: warm, slightly orange light on sun-lit surfaces.
    let warm = mix(vec3<f32>(1.0), vec3<f32>(1.0, 0.80, 0.62), twilight * sky * daylight);
    var rgb = texel.rgb * light * night_tint * warm;
    if (emissive) {
        rgb = texel.rgb * ao * face_shade;
    }

    // Exponential-squared fog that starts at `fog.a` and is ~99.5 % at the render distance,
    // finished with a short smoothstep so the far edge of the loaded world is fully hidden.
    let dist = distance(in.world_position.xyz, view.world_position);
    let fog_len = max(params.y - fog.a, 1.0);
    let d = max(dist - fog.a, 0.0) / fog_len;
    let fog_exp = 1.0 - exp(-(d * d) * 5.3);
    let fog_edge = smoothstep(params.y - 12.0, params.y, dist);
    let fog_amount = max(fog_exp, fog_edge);
    // Fog tinted by the sun when it is low, so distant terrain glows at sunrise/sunset.
    let fog_rgb = fog.rgb;
    rgb = mix(rgb, fog_rgb, fog_amount);

    return vec4<f32>(rgb, texel.a * params.z);
}
