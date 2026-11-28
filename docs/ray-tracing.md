# Plan: ray tracing in the client's lighting

Status: a plan, for the maintainer to decide whether and how far to go (#80). Nothing here is
built. What it says of Bevy and wgpu was read from their sources at the versions in
`Cargo.lock` (Bevy 0.20.1, `bevy_solari` 0.20.1, wgpu 30.0.2), each with the file and line it
comes from (paths are inside the crate as published; a path with no crate named is in
`bevy_solari-0.20.1/src/`), and tried on the maintainer's card
(2026-11-27). What could not be checked says so.

## What was tried, and how

A trial branch, kept out of the pull request, turned Bevy's ray-traced lighting (Solari) on for
`--showcase`: `bevy`'s `bevy_solari` feature, `SolariPlugins`, `SolariLighting` on the orbit
camera, and for every mesh drawn with a `StandardMaterial` a copy Solari can trace
(`RaytracingMesh3d`). About 200 lines, all in one new module.

- **The card**: as the adapter reports it, `NVIDIA GeForce RTX 2080 Ti`, driver `NVIDIA`
  `615.78.08`, backend Vulkan, on Linux. A first-generation RTX card (2018). It offers every
  feature Solari asks for: the plugin loaded without its warning (below).
- **The run**: the dev profile (the client at `opt-level` 1, its dependencies at 3), on the host
  under `gamescope --backend headless -r 1000`, `--showcase --frame-times --max-fps 0 --frames
  1101`, at a 1280×720 window or fullscreen at 2560×1440. Unpaced, so a frame's time is the
  work's, not the display's: at the window's 60 the card idles between frames, clocks down, and
  each pass reads three or four times slower.
- **The numbers**: the first 200 frames left out, then over 900 frames the mean (and the 95th
  percentile) of the time between frames by the wall clock, and the mean of the sum of every
  drawing pass's time on the card, as Bevy's `RenderDiagnosticsPlugin` times them. The client's
  own frame counter cannot be used here: in a scripted showcase time goes on by a thirtieth of a
  second a frame, so `--frame-times`' `frame_time` always reads 33.3 ms.
- **The light compared**: today's (the defaults: SSAO high, TAA, bloom, depth of field, shadows
  medium, contact shadows); and traced, with shadow maps, SSAO and contact shadows turned off
  (what Solari replaces), TAA, bloom and depth of field kept.

The pictures are not in the repository (the pull request says where they are). What they show is
described where it matters.

## 1. What it is for

Seen in the trial, in the room, against today's light:

- **Every piece casts its shadow.** Today, at the showcase's distance, a chessman, a stone or a
  draughtsman casts almost no shadow on the felt that can be seen; the chess clock's is soft and
  faint. Traced, every piece casts a long, crisp shadow away from the window, exactly its shape,
  and the clock a hard one. This is the largest change, and the one a player would notice.
- **Shadows under and between pieces** come with it: where two pieces stand close, one's shadow
  falls on the other.
- **Polished things** (the stones, the lacquered draughtsmen) show the light's highlight and a
  grainy reflection of what is round them, not only of the room's cubemap. Without a denoiser
  (section 4) the grain is visible on every glossy surface.
- **Light carried from surfaces**: the felt's green and the wood's warm brown reaching the pieces'
  undersides was not distinguishable at this scale from the room's own fill light.
- **The window's soft shadow**: a directional light's shadow is as soft as its disc
  (`sun_disk_angular_size`, read in `bevy_solari-0.20.1/src/scene/binder/lights.rs:122-133`); a
  window would be a larger disc, or an emissive pane. Not tried.

What also changed, and would need work before any of it is worth showing:

- **The balance of the light is not today's.** Solari lights everything from the specular
  cubemap at its sharpest level (`scene/extract.rs:187-191`, `scene/sampling.wesl:144`), not
  from the diffuse cubemap the pipeline filters for the fill, so the same environment is lit
  differently: the clock's faces came out white where today they are grey, the wood paler and
  flatter.
- **Grain everywhere**, strongest on pale pieces and polished stones, even with TAA. With ReSTIR
  on (`SolariLighting::restir`) there is less of it.
- **The clock's lit lamp lost its glow**: it is an emissive material tuned for today's light and
  bloom.
- **The garden went dark.** Its trees' leaves are cut out of quads by a mask (`look.rs`'s
  `AlphaMode::Mask`); Solari traces every triangle as opaque (`scene/blas.rs:229`) and knows no
  alpha (`scene/binder/assets.rs:31-43`), so the canopy became solid and shaded the whole table.
  The dappled shadows of today's garden were gone.
- **The pointer's glove disappeared** (below, section 5).

## 2. The ways to do it

### Bevy's own ray-traced lighting (Solari) as a whole

What Bevy 0.20 offers. `bevy_solari` is an optional crate behind `bevy`'s `bevy_solari` feature
(`bevy_internal-0.20.1/Cargo.toml:165-168`), "an experimental set of plugins for raytraced
lighting" (`bevy_solari-0.20.1/src/lib.rs:29-38`):

- `RaytracingScenePlugin` builds the acceleration structures (a BLAS per mesh, a TLAS per frame)
  and binds every traced mesh's material, texture and light (`scene/mod.rs:36-104`).
- `SolariLightingPlugin`: "raytraced direct and indirect lighting" (`realtime/mod.rs:42-46`) for
  each camera with `SolariLighting`: direct light sampled from the lights, indirect light from a
  world-space cache, optionally ReSTIR (`realtime/mod.rs:112-217`, defaults at 220-236).
- A path tracer "for validation purposes", not real-time and not added by default (`lib.rs:36`).

It **replaces all of a camera's light**, not a part of it: the camera skips Bevy's deferred
lighting (`realtime/extract.rs:17`), and lights its G-buffer itself before the opaque pass
(`realtime/mod.rs:96-101`). There is no setting for shadows alone. `max_bounces` (default 3)
is the nearest: at 1 the picture looked the same as at 3 and cost a third less; at 0 the
picture is black, since the path loop then runs no times (`realtime/initial_path.wesl:88`).

What it leaves of today's code: the effects after lighting (TAA, bloom, depth of field,
tonemapping) and anything drawn in the transparent pass. What it replaces: shadow maps, contact
shadows and SSAO (`realtime/mod.rs:44-45` asks for shadow maps off), the environment map's light,
and fog (applied in the lighting it skips; not checked in a picture).

### Ray queries added to today's light, one thing at a time

Bevy 0.20 has no traced shadows, ambient occlusion or reflections for its ordinary renderer; the
only ray tracing in it is Solari. Doing one at a time means a pass of our own:

- The scene: `RaytracingScenePlugin` can be added alone. It is the half that does not change
  the renderer (`SolariLightingPlugin::build` does that, `realtime/mod.rs:62`), and its bindings
  are a public resource and a shader library ours could import (`scene/mod.rs:8-10, 41-44`).
  It still makes every mesh buffer usable as BLAS input (`scene/mod.rs:60-63`) and builds a BLAS
  for every mesh that qualifies, traced or not (`scene/blas.rs:107-125`).
- The pass: a full-screen compute pass of our own, one ray towards the sun per pixel from the
  depth and normals of the prepass, writing a visibility mask; one ray to a hard sun is not noisy
  and needs no denoiser. Ambient occlusion (short rays round each pixel) and reflections need
  more rays and a filter.
- The hard part, **not solved by anything in Bevy 0.20**: putting that mask into the
  `StandardMaterial`'s light. Its shader samples the shadow maps itself and takes no mask from
  outside. The ways in are an `ExtendedMaterial` on every material (which Solari would no longer
  see, since it traces `StandardMaterial` only: `scene/types.rs:17`), or replacing a shader
  library of `bevy_pbr`'s, which breaks on every Bevy release. Not tried.

### More baked ahead by the asset pipeline

No ray tracing at run time, so it serves every player and the headset. Blender (Cycles) already
runs in the pipeline. Bevy 0.20 reads what it would make: a lightmap per mesh in a second UV set
(`bevy_pbr-0.20.1/src/lightmap/mod.rs:84-88`), irradiance volumes (light carried through the air
round the table, `bevy_light-0.20.1/src/probe.rs:338`) and reflection probes. What it can give:
light bounced from the cloth and the wood onto what stands still (the table, the room, a board),
and a softer, truer fill on the pieces from a volume baked over the table. What it cannot: a
moving piece's shadow or its reflection in another, which is most of section 1. Cost: pipeline
work, a second UV set on the static models, and textures in the build.

## 3. Hardware and systems

**What Solari asks of the device**: five wgpu features, `EXPERIMENTAL_RAY_QUERY`,
`BUFFER_BINDING_ARRAY`, `TEXTURE_BINDING_ARRAY`,
`SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING` and
`PARTIALLY_BOUND_BINDING_ARRAY` (`bevy_solari-0.20.1/src/lib.rs:50-57`). Without them both plugins
log "not loaded. GPU lacks support for required features" and do nothing
(`scene/mod.rs:52-58`, `realtime/mod.rs:71-77`); but that check is made in `finish`, after
`build` has already made deferred rendering the default (`realtime/mod.rs:62`, below), so the
client has to know the features before it adds the plugins. How it learns them that early:
**not checked**. No limits beyond wgpu's defaults are asked; Solari keeps at most 5,000
textures (`scene/binder/assets.rs:23`). Bevy's default settings give the device every feature
the adapter has (priority *Functionality*, `bevy_render-0.20.1/src/settings.rs:89`, and
`src/renderer/mod.rs:321-322`).

**Through wgpu 30**:

- `EXPERIMENTAL_RAY_QUERY` is "experimental ... may have major bugs ... subject to breaking
  changes" (`wgpu-types-30.0.2/src/features.rs:1048-1060`).
- On **Vulkan** it needs `VK_KHR_ray_query` and `VK_KHR_acceleration_structure`
  (`wgpu-hal-30.0.2/src/vulkan/adapter.rs:956-971`), and then asks for those with
  `VK_KHR_deferred_host_operations` and `VK_KHR_buffer_device_address`
  (`vulkan/adapter.rs:1373-1386`). The maintainer's driver offers them all.
- On **DX12** wgpu offers ray queries (ray tracing tier 1.1 and shader model 6.5,
  `dx12/adapter.rs:611-621`) but not `BUFFER_BINDING_ARRAY` (`dx12/adapter.rs:505-513`;
  `features.rs:803-808` lists Vulkan only). **So Solari runs only on Vulkan**, on Windows too.
  wgpu tries Vulkan first (`wgpu-core-30.0.2/src/instance.rs:84-90`) and keeps that order among
  discrete cards (`instance.rs:575-600`), so a Windows machine with a Vulkan driver gets Vulkan
  today. Not run on Windows: **not checked**.
- On Metal wgpu 30 has them (Bevy's release note
  `_release-content/release-notes/solari_metal.md:14`); not a platform of ours.

**Which cards**: those whose Vulkan driver has `VK_KHR_ray_query`: on NVIDIA from the RTX 20
series; AMD and Intel cards with ray-tracing hardware through their drivers and Mesa's. Which
models and drivers exactly, on Windows and Linux, and what share of players that leaves out:
**not checked** (no survey read). Only one card was tried.

**Software**: Mesa 26.2's llvmpipe in the toolbox also offers `VK_KHR_ray_query` and
`VK_KHR_acceleration_structure`, and Solari loads on it; but the picture at frame 40 was black,
and the client crashed with a segmentation fault before frame 120.

**The Steam Frame**: a Snapdragon 8 Gen 3 (Adreno 750) under Mesa's Turnip driver
([Phoronix](https://www.phoronix.com/news/Steam-Frame-Uses-Zink)). Mesa has a merge request
adding `VK_KHR_ray_query` to Turnip for a740 and later
([!28447](https://gitlab.freedesktop.org/mesa/mesa/-/merge_requests/28447)); whether it is in the
Frame's driver: **not checked**. Section 6 makes the answer matter little.

**Keeping both alive.** Today's light stays the default and for everyone whose card lacks the
features. Two findings decide how:

- **Adding Solari changes everyone's renderer.** `SolariLightingPlugin::build` makes deferred
  rendering the default for every opaque material (`realtime/mod.rs:62`). A deferred material is
  left out of the forward opaque pass (`bevy_pbr-0.20.1/src/material.rs:1350`), and a camera with
  a prepass but no deferred one panics in `bevy_pbr-0.20.1/src/prepass/mod.rs:1425`
  ("called `Option::unwrap()` on a `None` value"). Tried: the plugins added and no camera traced,
  as a *Graphics* choice of today's light would be, crashed the client at once. With the
  default put back to forward after the plugins it drew as today, at today's cost (1.05 ms on
  the card at 720p, against 1.06).
- So the choice is made **at start-up**: the plugins added always (in a build with the
  feature), and the default left deferred only when traced light is chosen and the device has
  the features, forward otherwise. Changing it while running would mean every material built
  again (the client already does so for the board's wood); not tried. A cargo feature keeps
  `bevy_solari` out of builds that do not want it (the Frame's, at least); it added one crate to
  the build (`bevy_solari`, MIT or Apache-2.0) and about a minute and a half to the first one.

## 4. Denoising, and what is not open

Solari has **no denoiser of its own**. The one it is made for is NVIDIA's DLSS Ray
Reconstruction: its passes write DLSS's guide buffers only in a build with the `dlss` feature
(`realtime/node.rs:6-7, 135-137`), and Bevy's example says "Using DLSS Ray Reconstruction for
denoising (and cheaper rendering via upscaling) is _highly_ recommended when using Solari"
(`bevy-0.20.1/examples/3d/solari.rs:166`). Bevy's own release note: "Denoising is not available
on Metal yet, DLSS is NVIDIA-only" (`solari_metal.md:22`). Without it there is TAA, which the
trial used, and ReSTIR, which takes grain out at a price (section 7). The grain left is the
picture described in section 1.

**DLSS**: Bevy's `dlss` feature pulls `dlss_wgpu` (`bevy_anti_alias-0.20.1/Cargo.toml:46-51`):
"Requires a NVIDIA RTX GPU, and the Windows/Linux Vulkan rendering backend"
(`bevy_anti_alias-0.20.1/src/dlss/mod.rs:5`), and a `DlssProjectId` given to the app (line 11).

- To build: `dlss_wgpu` 6.0.1's build script stops unless `DLSS_SDK` and `VULKAN_SDK` name the
  SDKs (`dlss_wgpu-6.0.1/build.rs:12-14`), and runs bindgen, which needs clang. Its readme: "The
  DLSS SDK cannot be redistributed by this crate", cloned from NVIDIA's repository at v310.9.1
  (`README.md:20-24`).
- Tried, with no SDK (none was downloaded): `cargo build --features bevy/dlss` failed before the
  SDK was reached, compiling `bindgen` 0.73.2: "the trait bound `syn::file::File:
  syn::parse_quote::ParseQuote` is not satisfied", with two versions of `syn` (2.0.119 and
  3.0.10) in the graph. A resolution of versions to fix first; the trial stopped there.
- To ship: the DLSS library beside the binary (`libnvidia-ngx-dlss.so.310.9.1` and, for Ray
  Reconstruction, `libnvidia-ngx-dlssd.so.310.9.1` on Linux; `nvngx_dlss.dll` and
  `nvngx_dlssd.dll` on Windows) and the licence's text (`README.md:34-42`).
- The licence, *NVIDIA RTX SDKs License* (read at [NVIDIA/DLSS v310.9.1,
  LICENSE.txt](https://raw.githubusercontent.com/NVIDIA/DLSS/v310.9.1/LICENSE.txt); not legal
  advice): the binaries may be distributed inside an application (1(c), 2(a)); the SDK must not be
  used "in a way that would make it subject to an open-source license" (4(e)); distribution on
  terms "at least as protective" as NVIDIA's (2(c)); only "for their use in systems with NVIDIA
  GPUs" (Supplement 1); NVIDIA's attribution on a game's splash screen, about box and credits
  (Supplement 7.1(b)); NVIDIA told before a commercial release (Supplement 4). Whether a client
  under MIT and Apache-2.0 may carry it beside its own code, and so whether a release may include
  it, is **the maintainer's question**, not answered here. A source build could never include it
  (the SDK is not ours to put in the repository), so a DLSS build would be a release made with the
  SDK present, for NVIDIA cards only.

What there is without it: TAA, ReSTIR, fewer bounces, and a denoiser of our own (a spatial and
temporal filter guided by depth and normals) — a project of its own, and the part most likely to
make the whole not worth doing. Bevy's note names MetalFX Ray Reconstruction and Open Image
Denoise 3.0 as "promising paths for cross-platform denoising in the future"
(`solari_metal.md:22`); neither is in Bevy 0.20.

## 5. The client's own materials and shaders

What Solari traces is narrow, and the client has much outside it:

- **Only `StandardMaterial`**, on a mesh with exactly position, normal, UV and tangent, a
  triangle list and 32-bit indices (`scene/types.rs:11-17`, checked in `scene/blas.rs:249-262`).
  The trial made a traceable copy of every mesh drawn with a standard material: 41 meshes for
  the room's showcase, 87 for the garden's. A real build would have the pipeline write them so,
  and the client make its own meshes so (the board's slab, the glyphs, the cards), and draw and
  trace the same mesh (`bevy_mesh-0.20.1/src/mesh.rs:250-259`). Which of today's meshes lack
  what: not counted. What the acceleration structures take of the card's memory: **not
  measured**.
- **Of the material**, only base color (no alpha), roughness, metallic, reflectance, emissive,
  and four textures (normal map, base color, emissive, metallic-roughness;
  `scene/binder/assets.rs:31-43`), sampled at full size whatever the distance
  (`scene/bindings.wesl:123`, "TODO: Mipmap"). No occlusion texture, clearcoat, transmission or
  UV transform.
- **No transparency**: every triangle is opaque to a ray (`scene/blas.rs:229`). So:
  - *see-through previews* (`AlphaMode::Blend`, `board.rs`) must not be traced, or they cast full
    shadows; untraced, they are drawn in the transparent pass by Bevy's ordinary light, with
    no shadow maps, and so are lit unlike what is round them (read, not seen in a picture);
  - *the engraving* of a board's coordinates is a blended decal (`coordinates.rs`), the same;
  - *the garden's leaves*, a mask, became solid (section 1). Masked materials need their own
    proxies (a coarser solid canopy) or to be left out of the traced scene.
- **No visibility, no render layers**: Solari traces every `RaytracingMesh3d`, hidden or on
  another layer (`scene/extract.rs:22-56` reads neither). In the trial's first run, which traced
  every mesh, two round black shadows lay across half the table: something hidden, or the
  pointer's glove (capsules on a layer of its own camera); tracing only what is shown on the
  table's layer took both away at once. The client must add and take away `RaytracingMesh3d` as a
  thing is shown and hidden, and never give it to what another camera draws (the glove, a world
  panel).
- **The glove itself was gone** with Solari on: its materials became deferred (section 3) and its
  camera has no deferred prepass, so it was drawn by nothing. Its materials, and the world
  panels', must be forward.
- **Lights**: Solari knows only directional lights and emissive meshes
  (`scene/binder/lights.rs:31-34`), no point or spot lights (the client has none today). An
  emissive mesh is a real light to it, of at most 65,535 triangles (`lights.rs:91-92`): the
  clock's lamp would light the clock's face. Its emissive values, tuned for bloom, need tuning
  again.
- **The highlight's glow, the jump arcs and the portal** (`src/highlight/`, `src/arcs.rs`,
  `src/portal/`) are materials of our own, drawn additively or blended after the light. The
  portals still drew in the trial; the glow and the arcs were not on the showcase's table. They
  are invisible to rays (a portal's surface casts no shadow and
  takes none), and where a piece sinks into a portal the trial showed a dotted brown streak
  across the block, where today there is a soft shadow; why was not found.
- **A private game's look** is data the client turns into `StandardMaterial`s, so it is traced
  as ours are, with the same limits (alpha above all). No private look was tried.

## 6. In a headset

Not for the headset. Two eyes at the headset's rate (72 to 120) halves or more the time a frame
has (8.3 ms at 120), each eye being a camera Solari lights separately, with its own buffers
(`SolariLightingResources` on each camera, `realtime/prepare.rs:114-115, 239`). Solari needs MSAA
off (`realtime/mod.rs:105-108`), where the eyes draw with 4× MSAA today (`scene.rs`, decision 16);
its grain and TAA's trails swim when each eye has its own, which is why the eyes have no TAA or
SSAO now. On the Frame's own processor, beyond the question of the driver (section 3), the Adreno
750 has a fraction of a desktop card's time to spend. Streamed from a PC with an RTX card it might
fit; whether Solari, or DLSS, works with the OpenXR plugin's eye cameras: **not checked**, and not
tried in a headset. Baked light (section 2) is what would improve the headset's table.

## 7. Speed

The target: the window's 60 frames a second at 2560×1440 (16.7 ms) on a mid-range card that
traces rays. Measured (section *What was tried*): the card's time is the sum of the drawing
passes; *traced* is Solari's own passes and the acceleration structures' builds.

| Scene, size | Light | Frame, mean (p95) | Card | of which traced |
|---|---|---|---|---|
| room, 1280×720 | today | 4.21 ms (5.04) | 1.06 ms | — |
| room, 1280×720 | traced | 3.99 ms (4.78) | 1.18 ms | 0.56 ms |
| room, 1280×720 | traced, ReSTIR | 4.17 ms (5.06) | 2.85 ms | 2.22 ms |
| room, 2560×1440 | today | 4.21 ms (5.09) | 2.70 ms | — |
| room, 2560×1440 | traced | 4.93 ms (5.99) | 3.95 ms | 2.10 ms |
| room, 2560×1440 | traced, 1 bounce | 4.55 ms (5.67) | 3.00 ms | 1.41 ms |
| room, 2560×1440 | traced, ReSTIR | 11.12 ms (13.07) | 9.34 ms | 7.60 ms |
| garden, 2560×1440 | today | 5.94 ms (6.97) | 3.07 ms | — |
| garden, 2560×1440 | traced | 8.11 ms (9.17) | 7.18 ms | 5.06 ms |

Read so: on this card, at 1440p, traced light without ReSTIR costs 1.3 ms more of the card's time
in the room (the shadow maps and SSAO it replaces, 0.9 ms, given back) and 4 ms more in the
garden; with ReSTIR 6.6 ms more. All are inside 16.7 ms, and at the default limit of 60 a frame
the difference is in power drawn, not in frames. The frame's own time at 720p is the processor's
(about 4 ms whatever the light), which is why traced can read faster there. The acceleration
structures cost 0.1 ms a frame to rebuild (`tlas_build`), and up to about 1 ms in a frame while
models load (`blas_build`, read from the per-second log). The lighting pass grows faster than the
pixels: 0.32 ms at 720p, 1.81 at 1440p (four times the pixels, 5.6 times the time).

**Not measured**: a game's table (a board, its previews, the glow) as against the showcase; a
mid-range card of today (the 2080 Ti is a high-end card of 2018, and no claim is made here of
which newer card it matches); memory; Windows; and DLSS, which renders at a lower resolution and
would make every traced row cheaper.

How to measure, for each phase: the trial's way, kept as a script, on the same scenes (the
room's and the garden's showcase, and one game's table from a scratch server), at 1440p,
unpaced, the card's pass times summed over 900 frames after 200, and the same with the
phase's option off.

## 8. Keeping up with Bevy

The client moves one Bevy minor version at a time, each its own pull request. Solari is
experimental in both layers: wgpu's ray queries are "expected to be subject to breaking changes"
(section 3), and Bevy's own release note for Solari in 0.20 is a stub ("Solari TODO",
`_release-content/release-notes/solari.md:1-7`), so what changed is learnt from the source. What
each upgrade would cost, beyond today's: the client's code that touches Solari (the trial's 200
lines; a finished phase perhaps twice that) built again against the new API, and **a run on a real
card** before the upgrade merges, since CI cannot see whether the traced picture is right (section
9). If a release breaks Solari, the option is greyed for that release rather than holding the
upgrade back. A DLSS build adds `dlss_wgpu`'s version, which follows wgpu's and the SDK's
(`dlss_wgpu-6.0.1/README.md:5-16`).

## 9. Testing

CI has no card that traces rays, and llvmpipe, though it offers the extensions, crashed (section
3); CI's own Mesa was not checked.

- **CI can**: build with the feature on Linux and Windows (and without it); test without a
  window the parts with no Bevy systems in them: which meshes are traced (shown, on the table's
  layer, opaque or masked), a mesh made traceable (its attributes and indices), the choice
  greyed when the device lacks a feature (a function of the feature set), and the default
  renderer that follows the choice; and a headless `App` test that adding the plugins with
  traced light off leaves the default forward (the crash in section 3 is exactly what that
  test would have caught).
- **Only a real card can**: whether the picture is right (shadows, grain, the light's balance),
  its cost, and that the client still draws on a card without the features. A `--frames N
  --screenshot` pair, today's and traced, of the room's and the garden's showcase, looked at
  for each pull request of a phase and each Bevy upgrade.

## 10. The phases

Each is what it adds, when it is done, its size, whether it stands if the next is never built,
and what would make it not worth doing (point 11 of the issue, at each phase's end).

### Phase 0: a trial that measures (begun here)

- **Adds**: answers, not a feature. This issue's trial did the showcase in the room and the
  garden. Left: the same on a game's table (Hex's, with its engraving and previews, from a
  scratch server); on a Windows machine with an RTX card (Vulkan chosen, Solari loads); on an
  AMD card if one is at hand; and the `syn` conflict behind the `dlss` feature found and settled,
  to know whether a DLSS build is possible at all, without shipping one.
- **Done when**: this document's *not checked* and *not measured* items are answered or
  declared unanswerable, with pictures.
- **Size**: no feature pull request; a branch, and one pull request to this document.
- **Stands alone**: yes; it is only knowledge.
- **Not worth doing if** the maintainer has already decided against the grain (section 4) and
  against DLSS: then go straight to the baked light, or stop.

### Phase 1: traced light as a choice, sun and shadows first

- **Adds**: *Lighting: Standard / Traced* on the *Graphics* page (`graphics.rs`), greyed where
  the device lacks the features, kept in `config.toml`, taking effect at the next start. Traced
  is Solari on the flat cameras with one bounce (section 7: the same picture for less), ReSTIR
  off. Since Solari cannot do shadows alone (section 2), "shadows first" means the work is
  aimed at them: every piece's shadow right, the light's balance matched to today's (the fill
  from the diffuse cubemap's level, exposure), the glove and world panels kept forward, traced
  meshes following what is shown, see-through and engraved materials left out, masked leaves
  given solid proxies or left out, the lamp retuned. Behind a cargo feature, on for Linux and
  Windows releases, off for the Frame.
- **Done when**: on the maintainer's card, the room's and garden's showcase and a Hex game drawn
  traced show every piece's shadow and nothing missing or wrongly dark against today's, a run
  with traced off draws exactly as today and costs the same, a card without the features is
  offered no choice, and the cost at 1440p is written down.
- **Size**: three or four pull requests: the feature and the choice (with the deferred default
  and the forward glove); traced meshes (conversion or the pipeline, visibility, layers, alpha);
  the light's balance and the lamp; the garden's leaves.
- **Stands alone**: yes, but its picture has grain. Whether that is acceptable is the phase's
  real question.
- **Not worth doing if** the grain on pale pieces and polished stones, with TAA alone, is worse
  to look at than today's faint shadows; or if, before it, today's shadow maps can be tuned to
  show the pieces' shadows (they are faint now, and that alone may be most of what a player
  would see; a cheaper look first).

### Phase 2: occlusion and reflections

- **Adds**: what Solari already traces once on, made clean: pieces reflected in a polished board
  and in each other, occlusion where pieces stand close. With Solari these are not separate
  switches; the work is noise: ReSTIR as a *Graphics* step (Traced, Traced high), and a
  denoiser: DLSS Ray Reconstruction in an NVIDIA build if the licence question is answered yes,
  or one of our own.
- **Done when**: a polished stone beside another shows it, without grain that moves when the
  camera does, at 60 frames a second at 1440p on the maintainer's card.
- **Size**: one pull request for ReSTIR as a step; two or three for DLSS (the build, the
  release, the licence's texts and attribution); a denoiser of our own, three or more, and
  uncertain.
- **Stands alone**: ReSTIR does; DLSS does on NVIDIA only.
- **Not worth doing if** DLSS cannot be shipped and a denoiser of our own is not wanted: ReSTIR
  alone cost 7.6 ms at 1440p on this card for a picture still somewhat grainy.

### Phase 3: light bounced from surfaces

- **Adds**: the felt's green and the wood's warm brown carried onto the pieces and the board:
  Solari's world cache and more bounces (back to 3), tuned for a table a metre across
  (`world_cache_position_base_cell_size`, 0.15 m by default at `realtime/mod.rs:232`, is larger
  than a piece).
- **Done when**: a pale piece beside the board shows the wood's warmth where Cycles' preview
  does, without light leaking under pieces, at the target speed.
- **Size**: one or two pull requests, mostly tuning and pictures.
- **Stands alone**: yes, as a step above phase 2.
- **Not worth doing if** it cannot be told from today's fill at a player's distance, as it could
  not in this trial; or if baked light gives the same for every player.

### Phase 4: the headset, if at all

- **Adds**: traced light in a PC-streamed headset with an RTX card.
- **Done when**: both eyes at the headset's rate with traced light, no swimming grain.
- **Size**: unknown; two at least, after Bevy's and the OpenXR plugin's support is checked.
- **Stands alone**: yes.
- **Not worth doing**, by the numbers above: not on the Frame's processor at all, and on a PC
  only if phase 2's denoiser exists and works per eye. Recommended to leave out.

### Beside the phases: baked light

- **Adds**: a lightmap for what stands still (table, room, the garden's ground), an irradiance
  volume over the table, baked by the pipeline in Cycles: truer fill and bounced light for
  every player, the headset included.
- **Done when**: the room and the garden drawn with it, side by side with today's, on any card.
- **Size**: two or three pull requests, mostly in `assets-src/`.
- **Stands alone**: yes, and needs nothing above.
- **Not worth doing if** the pieces' shadows are what matter (it cannot give a moving piece's).

## Recommendation

For the maintainer to decide; this is the plan's view.

1. First, the cheap look: today's pieces cast almost no visible shadow in the showcase, and
   traced light's main gain was exactly that. If tuning the sun's shadow maps and contact
   shadows gives most of it, it does so for every player and the headset.
2. If traced light is still wanted, finish phase 0 (a game's table, Windows, the `dlss` build)
   and decide on DLSS's licence before phase 1: without a denoiser, Solari's picture has grain
   that today's does not, and phases 2 and 3 depend on removing it.
3. Phase 1 is worth it only as an option for RTX-class cards, at start-up, with today's light
   the default; never for the headset.
4. Baked light is the better way to light carried from surfaces, since it costs nothing at run
   time and reaches everyone.
