// The glow round a position a click would be taken on (`highlight.rs`): a soft line of light
// traced along its edge (the ring, lying on its top), and thin streaks of light rising from that
// edge (the ribbon, standing round it), both slowly moving.
//
// Each mesh's first texture coordinates run along the outline, one unit for each streak's place
// (`u`), and across the ring from the edge inwards or up the ribbon from its foot (`v`, 0 to 1);
// its second's `x` runs once round the outline (0 to 1). The light is added to what is behind
// it, so the output's alpha is 0 (premultiplied blending, `AlphaMode::Add`).

import bevy_pbr::render::forward_io::VertexOutput;
import bevy_pbr::render::mesh_view_bindings::globals;

struct Glow {
    // The light's color, linear.
    color: vec4<f32>,
    // The ring: its line's brightness, where it lies across the ring (0 the edge), how wide it is,
    // and the halo's brightness round it.
    ring: vec4<f32>,
    // The halo's reach across the ring; how far the light along the edge rises and falls as it
    // travels round; how fast it travels; how fast the whole breathes.
    ring_motion: vec4<f32>,
    // The ribbon: its streaks' brightness, the glow hugging its foot, how fast the streaks rise
    // (ribbons a second), and how wide a streak is (a share of its place).
    ribbon: vec4<f32>,
    // Which part this is: 0 the ring, 1 the ribbon, 2 a jump's arc (`arcs.rs`), whose `u` runs
    // from where the piece left (0) to where it landed (1) and `v` across it; it reads `ring`'s
    // line, width and halo, and `ring_motion`'s reach, and its light running along it.
    part: vec4<f32>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> glow: Glow;

const TAU: f32 = 6.28318530718;

fn hash(n: f32) -> f32 {
    return fract(sin(n * 127.1 + 311.7) * 43758.5453);
}

// How much the whole glow breathes: gently, never out.
fn breath(t: f32) -> f32 {
    return 0.85 + 0.15 * sin(t * glow.ring_motion.w * TAU);
}

fn ring(u: f32, v: f32, round: f32, t: f32) -> f32 {
    let line = exp(-pow((v - glow.ring.y) / glow.ring.z, 2.0)) * glow.ring.x;
    let halo = exp(-v / max(glow.ring_motion.x, 0.001)) * glow.ring.w;
    // Two brighter stretches travelling round the edge.
    let travel = 1.0 - glow.ring_motion.y * (0.5 - 0.5 * cos((round * 2.0 - t * glow.ring_motion.z) * TAU));
    return (line + halo) * travel;
}

fn ribbon(u: f32, v: f32, t: f32) -> f32 {
    let place = floor(u);
    let across = fract(u) - 0.5;
    // Each streak its own: a little to one side, as wide as it likes, rising at its own pace.
    let r = hash(place);
    let r2 = hash(place + 17.0);
    let width = glow.ribbon.w * (0.6 + 0.8 * r2);
    let off = (r - 0.5) * 0.4;
    let column = smoothstep(width, 0.0, abs(across - off));
    let phase = fract(t * glow.ribbon.z * (0.55 + 0.9 * r) + r2);
    // A soft stretch of light running up the streak, longer below than above.
    let d = v - phase;
    let pulse = select(exp(-pow(d * 3.0, 2.0)), exp(-pow(d * 7.0, 2.0)), d > 0.0);
    // Fading out to nothing at the top, and a little at the foot so it grows out of the ring.
    let fade = pow(1.0 - v, 1.6) * smoothstep(0.0, 0.08, v);
    let streak = column * (0.25 + 0.75 * pulse) * fade * glow.ribbon.x;
    let foot = exp(-v * 9.0) * glow.ribbon.y;
    return streak + foot;
}

// A jump's arc: a line along its middle with a halo fading to nothing at its sides, a stretch of
// brighter light running along it the way the piece went, and its two ends faded in.
fn arc(u: f32, v: f32, t: f32) -> f32 {
    let across = abs(v - 0.5) * 2.0;
    let line = exp(-pow(across / glow.ring.z, 2.0)) * glow.ring.x;
    let halo = exp(-across / max(glow.ring_motion.x, 0.001)) * glow.ring.w * (1.0 - across);
    let d = u - fract(t * glow.ring_motion.z) * 1.4 + 0.2;
    let run = 1.0 + glow.ring_motion.y * exp(-pow(d * 6.0, 2.0));
    let ends = smoothstep(0.0, 0.05, u) * smoothstep(1.0, 0.95, u);
    return (line + halo) * run * ends;
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let t = globals.time;
    let u = in.uv.x;
    let v = clamp(in.uv.y, 0.0, 1.0);
    var light: f32;
    if glow.part.x < 0.5 {
        light = ring(u, v, in.uv_b.x, t);
    } else if glow.part.x < 1.5 {
        light = ribbon(u, v, t);
    } else {
        light = arc(u, v, t);
    }
    return vec4<f32>(glow.color.rgb * light * breath(t), 0.0);
}
