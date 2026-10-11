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

| Kind | Triangles | Bevel width | Near | Bevel segments |
|---|---|---|---|---|
| Handheld (weapon, tool) | 2k–6k | 2–4 mm | 0.3 m | 2 |
| Small robot or creature (≤ 1 m) | 3k–8k | 5–10 mm | 2 m | 1 |
| Large robot (2–4 m) | 5k–12k | 1–3 cm | 3 m | 1 |
| Prop, console, crate | 0.5k–3k | 5–20 mm | 1.5 m | 1 |
| Flora cluster | 1k–5k | (organic, none) | 2 m | 1 |
| Ship module | 5k–30k | 3–15 cm | 5 m | 1 |

Bevels scale with the object: a bevel should read as a highlight from the
distance the object is usually seen from. **Near** is the closest a kind is
usually seen from (a held weapon's rear is about 0.3 m from the eye); the
rules below are judged there.

The large-robot budget was 8k–20k until the rules below; the Gunner, unchanged
in look, went from 19.3k to 8.2k under them. The other kinds have no asset that
measures them yet, so their numbers are unchanged.

## Spending triangles

Triangles go where they can be seen. The game shows everything at an 85°
vertical field of view, so at 1440 lines **one pixel at distance d is
d × 1.27 mm** (3.8 mm at a large robot's 3 m near; 2.5 mm at a small robot's
2 m; 0.4 mm for a weapon in first person). That pixel at the kind's near distance
is its **detail size**, and the rules follow from it. hb applies all of them;
the numbers live in `tools/assets/kinds.json`.

Measured on the Gunner before the rules (19,320 triangles): the unbevelled
shapes were 4,416 of them; the bevels were the other 14,904, and the second
bevel segment alone was 8,912. Small things cost as much as big ones: a 2 cm
vent slat took 108 triangles, the same as the torso's core box.

1. **Chamfer, don't round.** One bevel segment is the default (two for
   handhelds). A 2-segment arc costs about 2.5× a chamfer's triangles and only
   differs where the bevel is several pixels wide: a 12 mm bevel is 3 px at 3 m
   and less than 1 px at 12 m. On the Gunner it was 46% of the model; renders
   at 12 m differ by barely more than render noise, at 35 m by less, and at
   3 m only the highlight inside the chamfers changes. On a first-person weapon the bevels
   are 4–10 px wide all game and an arc catches highlights a chamfer doesn't, so
   handhelds keep 2.
2. **No bevel narrower than the detail size.** A bevel under one pixel at the
   near distance is never seen, but costs as much as a visible one. hb drops it
   (the edge stays hard, so the face still shades flat). Small lights, strips
   and lens rims usually fall here; a bevel authored below it is silently dropped.
3. **Round things get their segment count from their radius.** Leave
   `segments` out of a lathe or cylinder: hb picks the fewest (even, at least 8)
   that keep the silhouette within one pixel of a true circle at the near
   distance, or half a pixel if it glows, because a lit edge against a dark one
   shows its corners. On a large robot a bolt of 3 cm radius gets 8,
   a knuckle of 9 cm 12, and the Gunner's 6.6 cm eye lens 14. Those facets always shade smooth. Give `segments`
   only for a deliberate polygon, such as the Swarmer's octagonal pods.
4. **Hidden edges aren't bevelled.** An edge buried inside another primitive
   of the same part, deeper than its own bevel, is left unbevelled
   automatically, as long as every bevelled edge at both its ends is buried
   too (where a bevelled and an unbevelled edge meet, Blender bends the
   normals). In practice that means a whole buried primitive, or a buried end
   ring of a cylinder (its smooth seams separate it from the rest); a box half
   sunk into another keeps all its bevels. Parts move, so only the part's own
   primitives count. This one is small (184 triangles on the Gunner) but free.

What these rules don't cover, and are still the builder's call: how many
primitives a detail needs (a 3-box vent costs three boxes), and deliberate
faceting. A hard facet seam is bevelled like any hard edge, so a faceted lathe
pays a bevel along every segment: the Swarmer's faceted lathes are 4.9k of its
6.8k triangles, 3.2k of that in bevels.

## Lighting the models are judged under
The game is mostly dark: one sun, a dim fill from the sky, suit headlamps, and
emissives. Judge every model in-engine with `asset_view` (assets.md), including
the shadow-side shot. If a model is a black blob from the shadow side, its
emissives or its value contrast need work, not the lighting.
