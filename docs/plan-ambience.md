# Plan: ambience (the look of a place)

Goal: every location in the game has an **ambience**: a description, as
data, of its sky and the light it gives. The sky is rebuilt from it when
the place changes. Ambiences are hand-written or generated from a seed, as
the game needs; a scenario says which one it's in. Visitable objects
(planets you land on, stations you fly to) are not part of this; an
ambience is only what's at infinity.

Replaces `client/sky.rs`: one CPU-baked 1024² 8-bit cubemap, the same
everywhere, with single-texel stars that smear and shimmer at 1440p.
Design discussion 2026-10-14/16; the user's rulings: **stylized, not
photographic**; bodies are background only; build it all in one go,
with a preview scenario.

## Status (2026-10-17)

Built, all of it, uncommitted on `main`: the data and validator
(`sim/ambience/`), seven named skies (`assets/ambience/`) and four
archetypes, the layers (`client/ambience/`), the sky preview and its
console, the screenshot tour and `tools/sky-sheet`, and a `sky` stress
situation. The looks are **drafts awaiting the user's sign-off** on the
contact sheets.

How it differs from the plan below:

- **No loading veil over the bake.** A bake is a few milliseconds of GPU
  (see Performance), well inside a frame. Only the very first bake waits a
  few frames for its shaders to compile, and then the backdrop is black.
- **The bake is a render pass per face, not a compute shader.**
  `Rg11b10Ufloat` isn't a storage format (TheBook, Bevy rendering
  (lighting)).
- **The render world tells the main world when a bake is done** (an atomic
  in `SkyBake`), and only then is the fill filtered again. Without that,
  the first bake (shaders still compiling) left the fill filtered from an
  empty map.
- **Fewer stars than planned:** about 15,000 per sky, not 30,000. 24,000
  read as noise in the band.
- **Rocky bodies are coloured by regions** bigger than a facet (continents),
  not one random colour per facet (that looked like camouflage).
- **The sun's disc has a wide soft glow** as well as its corona: at 1–2°
  across the disc alone didn't read as a sun.
- **The console sets the sky only when it's worked** (or its file is
  saved). That lets the screenshot tour set skies directly.

## What an ambience is

`sim::ambience::Ambience`: plain data (serde), cosmetic only. The sim
never reads it; scenarios choose it, clients draw it. Sections:

- **sun**: direction, colour, illuminance, angular size, look (disc and
  corona). One sun casts shadows; a companion star is a body with an
  optional unshadowed light of its own.
- **light**: space's ambient colour and brightness, and the fill (the
  environment light's intensity). Explicit numbers, so a place can be
  darker than another on purpose; the validator holds them in range.
- **stars**: seed, count, how strongly they gather along the band, the
  brightness falloff, a small colour palette, how many get a glint.
- **band** (optional): a galactic band: its pole, width, a brighter bulge
  in some direction, glow colour, how much dust crosses it.
- **nebulae**: each a direction, angular size, two or three colours, a
  shape seed, how many tone steps, dust, and whether its rim is lit
  toward the sun.
- **bodies**: each a kind (rocky, gas giant, ice, moon), direction,
  angular radius, palette, optional rings, optional atmosphere rim. Lit by
  the sun.

Where they come from:

- **Named**, in `assets/ambience/<name>.ron`, for places with a look of
  their own.
- **Generated**: `ambience::generate(archetype, seed)`, for places that
  only need to be different (a jump run's nodes, later). Archetypes give a
  generated sky a character ("dim red dwarf", "nebula interior", "by a gas
  giant").
- `Scenario` gets an `ambience` field: how its world's ambience is made.

**On the wire**: the scenario spawns it as a `Staged` entity with an
`Ambience` component, replicated (`.replicate()`, since the preview edits
it live). It's a few hundred bytes, so the whole description is sent, not
a seed: clients don't need the files, and hand-written ambiences don't
have to go through the generator.

**Validation**: `ambience::problems(&Ambience) -> Vec<String>`, like
`design::problems`:

- **The magenta rule** (art-style.md) for every sky colour: at least 45°
  of hue (Oklch) from the weak-spot `#ff40c8`, unless it's near grey.
  Today's violet nebula, at 316°, breaks it (26° away).
- **A luminance cap** on the backdrop, so models still read as dark
  outlines and the HUD's hairlines still glow over it. Brightness belongs
  in small hot points (stars, the sun), not in big areas.
- No body overlapping the sun, and fill and ambient in range.

Tests run it over every shipped `.ron` and over a thousand seeds of each
archetype.

## Drawing it (`client::ambience`, in place of `client::sky`)

`client/sky.rs` is deleted first, along with `SUN_DIRECTION`. Its call
sites: `render.rs` (the sun's direction, the cabin light probes),
`client/mod.rs`, `bin/asset_view.rs`, `profile.rs` (`no_skybox`),
`devices::interior_ambient`, and the `tools/stress` header.

There are four layers, each drawn the way it needs:

1. **Backdrop**: the band, dust and nebulae, baked on the GPU by a
   compute shader whenever the ambience changes. One shader writes two
   cubemaps:
   - the **display** map, 1536² a face in `Rg11b10Ufloat` (HDR, 56 MB;
     about screen density at 1440p and an 85° FOV), shown by Bevy's own
     `Skybox`, which already draws a cubemap at infinity at the end of the
     opaque pass;
   - the **light** map, 256² in `Rgba16Float` (power of two, as
     `GeneratedEnvironmentMapLight` needs), which includes the bodies, so
     a gas giant lights you with its own colour.

   The stylized look: density quantized into a few tone steps with soft
   edges, dust as hard dark shapes, rims lit toward the sun.
2. **Stars**: instanced sprites at infinity. A custom material's vertex
   shader puts them on the far plane and sizes them in pixels, so they
   stay crisp and the same size at any FOV. Each client generates the
   catalogue from the stars section with a fixed seed.
   - **Look:** round cores of 1–3 px, the brightest few dozen with a
     four-point glint, colours from a small palette.
   - **Brightness:** HDR, so the bright ones bloom.
   - **Hidden behind bodies:** each star tests the few bodies directly.
3. **Bodies and the sun**: impostors at infinity, a quad with a
   ray–sphere test in the fragment shader, drawn every frame, so their
   edges are sharp at any zoom.
   - **Style:** a banded terminator, rocky bodies flat-faceted (the normal
     snapped to icosphere faces, like the asteroids), gas giants banded,
     rings as an annulus.
   - **Shared shading:** one WGSL module (`ambience_common.wgsl`) is
     imported by both the bake and the impostors. What lights you is what
     you see.
4. **Lighting**:
   - The two sun entities (the world's and the hands') take the sun's
     direction, colour and illuminance.
   - `GlobalAmbientLight` and each camera's fill take the ambience's values.
   - `interior_ambient` reads space's ambient and fill from the ambience,
     not from "as first seen" locals, which would go stale on a change.

**When the ambience changes**:
- The loading veil covers the bake.
- The environment light is generated again: `Filtering` restarts, so
  `settle_environment_maps` lets it filter for 30 frames and then removes
  it, as now.
- The cabins' uniform interior light is unchanged; it moves into the new
  module.

**`asset_view`** uses `assets/ambience/reference.ron`, a fixed sky, so
judging models doesn't depend on wherever the game happens to be.

**Profiling switches**: `no_skybox` becomes `no_backdrop`, `no_stars` and
`no_bodies`.

## The preview scenario

`ScenarioKind::Sky` (`"sky"`, "Sky preview"), its button in the free slot
of the mission wall's top row (`hull.ron`, at `(-3.54, 1.92, -5.15)`).

- **World**: our ship drifting, the crew in EVA beside it, and a test
  stand of things to judge the light on: a hull section, a rock and a
  dormant Gunner. They're for the shadow side and the silhouette against
  the bright parts of the sky.
- **The sky console**: a stand floating by the test stand, its own small
  design (`assets/ships/sky_console/`), worked like the mission wall.
  Everything is in the world: no keys, no config. Its devices replicate
  like any ship's, so in co-op anyone can work it. The authority builds
  the ambience from the console's state and replicates it.
  - **Buttons** (`ControlAction::Sky(..)`, handled by the scenario):
    `◀ Sky` / `Sky ▶` step through the named ambiences and then the
    archetypes; `◀ Seed` / `Seed ▶` / `Random` change a generated sky's
    seed.
  - **Levers, one per layer**: stars, band, nebulae, bodies, sun. Each
    layer can be seen alone, or with one taken away. They double as perf
    A/B switches you can see working.
  - **A sun junction**: where the sun sits (the ambience's own place,
    high, low, behind the stand, edge-on), plus a way that orbits it
    slowly, so the light sweeps across the test stand.
  - **Readout**: the scenario's status board (the HUD's live lines)
    shows the sky's name, or its archetype and seed, which layers are
    on, and anything the validator says about it.
- **Hot reload**: while the preview is on, the authority watches
  `assets/ambience/*.ron` and replicates an edited file's ambience as
  soon as it's saved. The console stays where it is, so you edit a file
  and look at the result.
- **`HULLBREAKERS_AMBIENCE=<name>|<archetype>:<seed>`** is for scripted
  screenshots only: the shots tool can't press buttons. Play uses the
  console.
- **Shots**: `HULLBREAKERS_SHOTS_SHIP=sky` visits every named ambience
  and a few seeds of each archetype. In each it takes the same views:
  toward the sun, away from it, along the band, the test stand against
  the brightest part of the sky, and a narrow FOV for star sharpness. A
  script tiles the shots into one contact sheet per ambience. This is how
  looks are reviewed, by the user and by agents.

## Starting looks

One named ambience per scenario, drafted in the preview and signed off by
the user on contact sheets:

- **Field**: home, a calm sky.
- **The derelict**: dim, near a red dwarf, little fill, which also suits
  "dark until its power is back".
- **Gallery**: the reference sky, today's look minus the violet.
- **Boarding, Grapple run, Recovery, Escort**: each with its own
  character; proposed in the preview.

Then three or four archetypes for the generator.

## Order of work

All of it is one piece of work, done in this order so each step can be
checked in the preview:

1. **Data**: `Ambience`, the RON files, the validator, the generator,
   tests; the `Scenario` field; replication (a netplay test that a client
   gets it).
2. **Preview scenario, sky console and shots tool** first, so
   everything after is looked at there. The layer levers come in as
   each layer is built.
3. **Delete `sky.rs`; the backdrop bake and the lighting.** The light
   cubemap, the sun entities and `interior_ambient` all come from the
   ambience, and `asset_view` uses the reference sky.
4. **Stars.**
5. **Bodies and the sun.**
6. **The looks**: each scenario's, then the archetypes.
7. **Write-up**: rows in `docs/perf.md` (before and after, with
   `tools/stress`), entries in TheBook for any Bevy friction, the
   lighting section of art-style.md, and the README.

## Decisions

- **The backdrop is baked, not drawn per pixel.**
  - *Chose, because:* it's static, so it costs once per place, and frame
    time is what's scarce (the carrier fight is 4–9 ms of GPU).
  - *Rejected:* drawing it live costs every frame for nothing that moves.
    Revisit if the backdrop is ever animated.
  - *Cost:* about 56 MB of VRAM, and hard edges only as sharp as the face
    size.
- **Stars are sprites.**
  - *Chose, because:* they sit at their exact sub-pixel position, at a
    size that's constant in pixels.
  - *Rejected:* a texture, which is today's problem. Also a per-pixel
    hash, which pays for every sky pixel and needs care at cube-face
    seams; sprites pay per star.
- **Bodies are drawn live as impostors.**
  - *Chose, because:* their edges are what the eye checks.
  - *Rejected:* baking them, which limits the edges to the face size.
    Also meshes, which nothing needs: they're at infinity, and visitable
    bodies are a separate system.
- **Bevy's `Skybox` is kept for the backdrop.**
  - *Chose, because:* given a good image, it already does the job.
  - *Rejected:* our own fullscreen pass, which would do the same thing
    in more code.
- **The whole description is replicated, not a seed.**
  - *Chose, because:* hand-written ambiences and hot reload need it, and
    it's small.
  - *Rejected:* a seed only, which would force every look through the
    generator.
- **The data lives in `sim`.**
  - *Chose, because:* scenarios choose it. It stays cosmetic: nothing in
    the sim reads it.

## Defaults chosen without asking (easy to change)

- **Names**: "ambience" for the system, "Sky preview" for the scenario.
- **Sizes**: display faces 1536² `Rg11b10Ufloat`, the light map 256², both
  tuning values.
- **Stars**: about 15,000, 25–40 with glints.
- **Fill**: written explicitly in each ambience, not measured off the
  baked map (no GPU readback); the validator holds it in range.
- **The console's layout**: the buttons, five layer levers and a
  six-way sun junction described above. Adding a control is a line in
  its design file.

## Risks

- **Compute passes in Bevy 0.19**: a render-graph node to write the
  bake. The pattern to follow is Bevy's own environment map filter
  (`bevy_pbr/src/light_probe/generate.rs`). TheBook gets an entry if it
  bites.
- **Sprites at the far plane** under MSAA and any depth prepass: check
  they aren't dropped or clipped.
- **A sky that looks good may hurt reading models in silhouette.** The
  luminance cap and the test-stand shots are there to catch it.
- **Startup**: the field's bake at launch, under the veil.

## Not in this plan

Visitable bodies, ambient sound, dust and particles near the camera,
exposure that adapts (the stars fading when you look at a sunlit hull),
and jump transitions. Each would be a new section of `Ambience`; none is
built here.
