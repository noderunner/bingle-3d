// A portal open on a flat surface (`portal.rs`): the surface inside its outline goes dark, as an
// opening into the dark would, a bright line of light is traced round just inside the outline
// with a halo spilling a little beyond it, and slow arms of light turn inside.
//
// The mesh's first texture coordinates' `y` runs from its middle (0) out to the outline (1) and
// on to the halo's end beyond it; `x` once round the outline (0 to 1). Its second are where it
// is in its own plane, as a share of the outline's inradius, for the arms' angle. Blended
// premultiplied: the color is added to what is behind, which is first darkened by the alpha.

import bevy_pbr::render::forward_io::VertexOutput;
import bevy_pbr::render::mesh_view_bindings::globals;

struct Look {
    // The rim's light, linear, and its brightness.
    rim: vec4<f32>,
    // The arms' light, linear, and their brightness.
    deep: vec4<f32>,
    // Where the rim's line lies (a share of the way out), how wide it is, how bright the halo
    // beyond the outline is, and how dark the inside goes (0 not at all, 1 black).
    shape: vec4<f32>,
    // How many arms turn, how much they twist towards the middle, how fast they turn (turns a
    // second), and how fast the whole breathes.
    motion: vec4<f32>,
    // How far beyond the outline the halo reaches (as `y` does), and how far it spreads.
    halo: vec4<f32>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> look: Look;

const TAU: f32 = 6.28318530718;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let t = globals.time;
    let d = in.uv.y;
    let p = in.uv_b;
    let r = length(p);
    let a = atan2(p.y, p.x);
    let breath = 0.88 + 0.12 * sin(t * look.motion.w * TAU);

    // The line round the outline, and its halo beyond, fading to nothing at the mesh's end.
    let line = exp(-pow((d - look.shape.x) / look.shape.y, 2.0));
    let beyond = max(d - 1.0, 0.0);
    let halo = exp(-beyond / max(look.halo.y, 0.001)) * look.shape.z
        * smoothstep(look.halo.x, 1.0, d) * step(1.0, d);
    let rim = (line * look.rim.w + halo) * breath;

    // Arms of light turning round the middle, twisting in towards it, seen in a band between
    // the dark middle and the rim.
    let phase = a * look.motion.x + r * look.motion.y - t * look.motion.z * TAU;
    let arm = pow(0.5 + 0.5 * sin(phase), 3.0);
    let band = smoothstep(0.1, 0.55, d) * smoothstep(1.0, 0.7, d);
    let arms = arm * band * look.deep.w * breath;

    // Dark inside, the dark fading out under the rim.
    let dark = look.shape.w * smoothstep(1.0, 0.82, d);

    let light = look.rim.rgb * rim + look.deep.rgb * arms;
    return vec4<f32>(light, dark);
}
