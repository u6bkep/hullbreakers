# Art style and asset standards

The rules every model follows, whoever (or whatever agent) builds it. How
models are built is in [assets.md](assets.md).

## The look: simple low-to-mid-poly
- **Silhouette first.** Things are read at range, against black space, often
  half in shadow. A model has to be recognisable as a dark outline with a few
  lights on it. Big, chunky, distinct masses; nothing thin or fussy.
- **Clean forms, chamfered edges, leaning low-poly.** The main masses get a
  small chamfer so their edges catch light; small details don't. Flat faces
  shade exactly flat, curved parts are smooth with few segments, and
  deliberate polygons show their facets. Up close a model may look faceted;
  at play distance it must look the same as a finer one (see "Spending
  triangles"). No greeble noise, no surface clutter to make something look
  "detailed".
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
  They're unbevelled; a surface is either one smooth skin (smooth shading over
  few polygons) or faceted like the asteroids (flat triangles), never chamfered
  like a machine. See "Organic primitives" in assets.md.
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

| Kind | Triangles | Bevel width | Near | Detail size | Bevel segments |
|---|---|---|---|---|---|
| Handheld (weapon, tool) | 1k–4k | 2–4 mm | 0.3 m | 1 px = 0.4 mm | 1 |
| Weapon part (a gun's barrel, magazine, stock, sight, muzzle device) | 40–1.2k | 1.5–4 mm | 0.3 m | 1 px = 0.4 mm | 1 |
| Small robot (≤ 1 m) | 2k–5k | 5–10 mm | 2 m | 2 px = 5.1 mm | 1 |
| Large robot (2–4 m) | 3k–8k | 1–3 cm | 3 m | 2 px = 7.6 mm | 1 |
| Small creature (≤ 1 m) | 2k–5k | (organic, none) | 2 m | 2 px = 5.1 mm | 1 |
| Large creature (2–4 m) | 3k–8k | (organic, none) | 3 m | 2 px = 7.6 mm | 1 |
| Prop, console, crate | 0.5k–3k | 5–20 mm | 1.5 m | 2 px = 3.8 mm | 1 |
| Flora cluster | 1k–5k | (organic, none) | 2 m | 2 px = 5.1 mm | 1 |
| Ship module | 5k–30k | 3–15 cm | 5 m | 2 px = 12.7 mm | 1 |
| Whole ship, inside and out | 10k–25k | 1.5–6 cm | 1.5 m | 2 px = 3.8 mm | 1 |

Bevels scale with the object: a bevel should read as a highlight from the
distance the object is usually seen from. **Near** is the closest a kind is
usually seen from (a held weapon's rear is about 0.3 m from the eye); the
rules below are judged there.

The budgets have come down twice, each time from measuring the models in them,
not from a target:

| Asset | Before the rules | One-pixel rules (1a31409) | Low-poly lean |
|---|---|---|---|
| Gunner (large robot) | 19,320 | 8,172 | 5,272 |
| Swarmer (small robot) | 7,810 | 6,762 | 3,558 |
| SMG (handheld) | 3,852 | 3,756 | 1,772 |

Props, flora and ship modules have no asset yet, so their budgets are
unchanged; lower them when the first ones are measured. The creature budgets
are the robots' for now. The first two fauna came in well inside them: the
Limpet (small) at 3.0k, half of it the rock it hides in, and the Grazer
(large) at 5.6k, 2.4k of it the bell with its canals painted per face.

**Whole ships** were measured on the first one, the carrier (60 m, two decks, a
hangar, a bridge, and nine rooms and corridors): 16,860 triangles, split into
parts by region so the game can frustum-cull them:

| exterior | engines | hangar | ramp | lower deck | upper deck | bridge | console | crates |
|---|---|---|---|---|---|---|---|---|
| 4,604 | 1,272 | 1,828 | 412 | 3,604 | 3,344 | 984 | 232 | 580 |

The budget's top, 25k, is about 1.5x the carrier: room for a bigger ship or
more dressed rooms, not a target. Where the carrier's triangles go: a deck's
walls, floor and ceiling are about 600; the other 3k is its dressing (ribs and
beams overhead, conduits, light fixtures, door frames, wall plates). Outside,
its 60-odd bevelled armour plates are 2.6k of the 4.6k. Hazard stripes were the one
surprise: with both colours as prisms, 22 cm wide, they were 2.1k of the
hangar's 3.0k. A black backing plate with only the yellow stripes as prisms,
30 cm wide, looks the same and brought the hangar to 1.8k (the ship from 19.1k
to 17.1k). Under 10k, a ship this size is bare walls.

A ship's **near is 1.5 m**, because its inside is seen from a step away; its
outside is seen from 10–400 m. One kind can't serve both, so a ship finishes
its parts differently: interior parts bevel 1.5 cm and leave small details
unbevelled; exterior parts bevel 3–5 cm. Round parts give `segments`
explicitly (the octagonal engine bells, hexagonal pipes): left to the 3.8 mm
detail size, a 1.3 m nozzle would get over 40 segments to look round from
1.5 m, where nobody sees it.

A ship is drawn instead of its collision boxes, so its walkable surfaces are
fixed by the layout, not by the builder; see "Ships" in assets.md.

## Spending triangles

Triangles go where they can be seen, and **the style leans low-poly**: we
accept a more faceted look up close in exchange for fewer triangles, as long
as every model still reads cleanly and nothing changes at play distance
(10–15 m for enemies). Hard facets are part of the look, not a defect to
smooth over.

The game shows everything at an 85° vertical field of view, so at 1440 lines
**one pixel at distance d is d × 1.27 mm**. A kind's **detail size** is a
number of pixels at its near distance: **two** for everything in the world,
**one** for handhelds, which are held in front of the camera all game. That
is 7.6 mm on a large robot, 5.1 mm on a small one, and 0.4 mm on a weapon in
first person. The rules follow from it. hb applies all of them; the numbers
live in `tools/assets/kinds.json`.

1. **Chamfer, don't round.** One bevel segment for every kind. A 2-segment arc
   costs about 2.5× a chamfer's triangles and only differs where the bevel is
   several pixels wide. On the Gunner it was 46% of the model. Handhelds kept
   2 until the low-poly lean; dropping it halved the SMG (3,756 → 1,772). At
   0.5 m the two can't be told apart. At 0.3 m the rear edges show a crisp
   chamfer band where there was a rolled highlight.
2. **No bevel narrower than the detail size.** hb drops it, and the edge stays
   hard, so the face still shades flat. With the two-pixel detail size this
   takes the bevels off small things on world objects, such as bolts, slats,
   strips, lens rims and grille bars. On a large robot it takes everything under
   7.6 mm: a vent slat now costs 12 triangles instead of 44, and a bolt 28
   instead of 60. The main masses keep their chamfers (the Gunner's are
   12–14 mm). A bevel authored below the detail size is silently dropped, so
   don't author a main-body bevel just under it; if a part should be
   unbevelled, say `bevel=0`.
3. **Round things get their segment count from their radius.** Leave
   `segments` out of a lathe or cylinder. hb picks the fewest (even, at least
   8) that keep the silhouette within the detail size of a true circle, or
   half of it if it glows, because a lit edge against a dark one shows its
   corners. On a large robot a knuckle of 9 cm gets 8, the Gunner's 6.6 cm eye
   lens 10, and its 14 cm weak-spot core 14. Those facets always shade smooth.
   Give `segments` only for a deliberate polygon, such as the Swarmer's
   octagonal pods.
4. **A deliberate polygon's facet seams aren't bevelled.** When a lathe has
   explicit `segments` and its facets meet at `smooth_angle` or more, the seams
   between them are hard edges with no bevel. Its end caps and profile steps
   are still bevelled. The hard facet is the look; a chamfer along each one
   cost more than the rest of the lathe. This rule alone took the Swarmer from
   6,762 to 4,990, and its facets now shade exactly flat (the chamfers had bent
   them).
5. **Hidden edges aren't bevelled.** An edge buried inside another primitive
   of the same part, deeper than its own bevel, is left unbevelled
   automatically. This only happens when every bevelled edge at both its ends
   is buried too, because where a bevelled and an unbevelled edge meet,
   Blender bends the normals. In practice that means a whole buried primitive,
   or a buried end ring of a cylinder (its smooth seams separate it from the
   rest); a box half sunk into another keeps all its bevels. Parts move, so
   only the part's own primitives count. An edge hidden by *another* part,
   such as a knuckle's end inside its parent's fork plates, is the builder's
   call: give that primitive `bevel=0`.

What these rules don't cover is still the builder's call:
- how many primitives a detail needs (a 3-box vent costs three boxes);
- how finely a profile is stepped. The Gunner's fork lobes went from 30° to
  45° steps, about 200 triangles across six joints, invisible past 3 m.

Bevelled stays the default for the main masses. Their chamfers are what
catch the light at play distance.

## Lighting the models are judged under
The game is mostly dark: one sun, a dim fill from the sky, suit headlamps, and
emissives. Judge every model in-engine with `asset_view` (assets.md), including
the shadow-side shot. If a model is a black blob from the shadow side, its
emissives or its value contrast need work, not the lighting.
