# Art style and asset standards

The rules every model follows, whoever (or whatever agent) builds it. How
models are built is in [assets.md](assets.md).

## The look: simple mid-poly
- **Silhouette first.** Things are read at range, against black space, often
  half in shadow. A model has to be recognisable as a dark outline with a few
  lights on it. Big, chunky, distinct masses; nothing thin or fussy.
- **Clean forms, chamfered edges.** Hard edges get a small bevel so they
  catch light, flat faces shade exactly flat, and curved parts are smooth
  with modest segment counts. No greeble noise, no surface clutter to make
  something look "detailed".
- **No textures yet.** Materials only (PBR metallic-roughness). Detail comes
  from form, material breaks between parts, and emissive accents.
- **Emissives carry meaning.** Lights mark function, and are what you see of
  a thing in shadow, so every model that matters in play should have some.
  Three roles, from loud to quiet:
  - `emissive`: eyes, sensors, interactables. The thing's identity in the dark.
  - `emissive_secondary`: power lights, status strips, thruster glow. Worked
    into the design (a strip along a seam, a ring in a nozzle) so they read
    as part of the machine; they also give it a readable outline from every
    side.
  - `emissive_core`: **weak spots only**: an exposed, glowing core that takes
    extra damage. The game pulses it. Reserve it for weak spots so players
    learn what it means; mark its centre with a `SOCKET_weak`.
    Weak spots are the same hot magenta (`#ff40c8`) on every enemy, the one
    colour rule in the game: it's a signal to learn, not a faction.
- **Value contrast between masses.** Neighbouring parts differ in lightness
  (light body, dark grip; pale plating, dark joints) so the form reads even
  without colour.

## Colour
Colour is not a faction code. As in Deep Rock Galactic, *anything that moves
might be hostile*; which things aren't is learned over time, and hinted by
design language rather than by paint:
- **Robots** share a mechanical language: exposed joints, sensor eyes, panel
  seams, industrial wear. They come in many colours.
- **Flora and fauna** are organic: no straight lines, bioluminescent accents.
- **Crew gear** is clean, human-scaled, and practical.

Every asset brief names its palette, and the builder works from it. Left to
themselves, agents converge on the same choice (in the pipeline bake-off, both
independently made an orange gun with a cyan strip), so variety has to be
asked for.

## Conventions
- Metres. +Y up, forward is −Z, +X is right (glTF as Bevy reads it).
- The origin is where the thing is held, stands, or mounts: the grip of a
  weapon, the feet of a robot, the mounting face of a ship module.
- **Parts** that move, swap, or detach are separate named nodes, pivoted at
  their joint or socket.
- **`SOCKET_<name>`** are empty nodes marking attachment points (`SOCKET_muzzle`,
  `SOCKET_grip`, `SOCKET_mag`, `SOCKET_eye` ...). A swappable part hangs from
  its socket, so it moves with whatever it is mounted on.
- **`COL_<name>`** are convex meshes that become physics colliders and are
  never drawn. Use a few simple hulls; one hull over an L shape is mostly air.
- Materials are named by **role** from the shared library (see assets.md),
  never `Material.001`.

## Budgets (triangles)

| Kind | Triangles | Bevel width |
|---|---|---|
| Handheld (weapon, tool) | 2k–6k | 2–4 mm |
| Small robot or creature (≤ 1 m) | 3k–8k | 5–10 mm |
| Large robot (2–4 m) | 8k–20k | 1–3 cm |
| Prop, console, crate | 0.5k–3k | 5–20 mm |
| Flora cluster | 1k–5k | (organic, none) |
| Ship module | 5k–30k | 3–15 cm |

Bevels scale with the object: a bevel should read as a highlight from the
distance the object is usually seen from.

## Lighting the models are judged under
The game is mostly dark: one sun, a dim fill from the sky, suit headlamps, and
emissives. Judge every model in-engine with `asset_view` (assets.md), including
the shadow-side shot. If a model is a black blob from the shadow side, its
emissives or its value contrast need work, not the lighting.
