# Building 3D assets

How models are made. *What* they should look like (style, conventions, budgets) is in
[art-style.md](art-style.md); read it first.

Every model is built from code: a short Python script drives Blender 5.2 headless through
the `hb` library, which exports a `.glb`, and a separate stdlib-only verifier checks the
result. Nobody opens the Blender UI and nothing reads a `.blend` file. The script, the
sidecar and the outputs are all committed.

```
assets/<group>/<name>/
  asset.json          # authored: kind, palette, expectations from the brief
  build.py            # authored: the model, using hb
  <name>.glb          # generated, committed: what the game loads
  <name>.intent.json  # generated, committed: which faces were modelled flat (the verifier reads it)
  preview/            # generated, git-ignored: renders and build.log
tools/assets/
  build               # the driver: build + verify one asset or all of them
  hb/                 # the library build scripts import
  materials.json      # the material roles
  kinds.json          # budgets per kind (the art-style table)
  verify_glb.py       # the verifier (plain python3, no Blender)
  run_build.py        # Blender entry point the driver uses
```

## Making an asset, end to end

1. **Brief.** You need: what it is, its kind (`handheld`, `small_robot`, `large_robot`,
   `prop`, `flora`, `ship_module`), rough size, the parts that move or swap, the sockets,
   and a palette. If the brief names no palette, ask; don't default to orange and cyan.

2. **`asset.json`, before modelling.** Write the expectations from the brief, not from
   your model, so they can catch you. The directory name must equal `name`.

   ```json
   {
     "name": "smg_frame",
     "kind": "handheld",
     "brief": "One or two lines of the brief.",
     "palette": {"paint_primary": "#c46c2c", "emissive": "#6cedff"},
     "expect": {
       "size": {"z": [0.40, 0.45], "y": [0.2, 0.24]},
       "parts": ["receiver", "magazine"],
       "colliders": ["COL_body"],
       "sockets": {
         "SOCKET_grip":   {"at": [0, 0, 0], "tol": 0.001},
         "SOCKET_muzzle": {"at": [0, 0.1, -0.3], "forward": [0, 0, -1]}
       },
       "tolerance": 0.01
     }
   }
   ```

   - `palette` maps a material role to an sRGB hex colour (as a colour picker shows it).
     `paint_primary`, `paint_secondary` and `emissive` must be in it if the model uses them.
     Any other role may be recoloured too. Nothing but the colour can change.
   - `expect.size` holds `[min, max]` metres for the render-mesh bounding box, per axis
     (any of x, y, z).
   - `expect.sockets`: each must exist. `at` is its world position (within `tol`, default
     `tolerance`, default 0.01 m); `forward` / `up` are its local −Z / +Y in world (within 5°).
     A socket position on the forward axis is what catches a model built backwards.
   - `expect.parts` / `expect.colliders` must exist.
   - Unknown keys anywhere in `asset.json` (a typo such as `foward`) are errors, not ignored.

3. **`build.py`.** See the API below and `assets/weapons/smg_frame/build.py` (60 lines).
   Sketch the side outline on paper in Bevy coordinates first: forward is **−Z**.

4. **Build and verify:** `tools/assets/build weapons/smg_frame` (the path is relative to
   `assets/`). It deletes the old outputs, runs the script in Blender, renders four
   previews, then runs the verifier. It exits non-zero on any failure. Use `--fast` to skip
   the previews (export alone takes about a second; the previews take about 15 s). Run it with
   no argument to rebuild everything.

5. **Look at the previews** in `preview/` (`right`, `left`, `front_3q`, `rear_3q`). They're
   for shape, proportion, colour blocking and bevels. They are not the in-game look: there's
   no bloom, the lights differ, and the emissives are shown at strength 1 so you can see
   their hue.

6. **In-engine check.** Judge it under the game's lighting with `asset_view`, including
   the shadow-side shot. The headless harness is shared, so check nobody else is using it
   first (`tools/hl env`); see the harness docs.

   ```sh
   cargo build --bin asset_view
   tools/hl start && HL_BIN=asset_view tools/hl game <out_dir> weapons/smg_frame/smg_frame.glb
   ```

   Model paths are relative to `assets/`; pass several to see them side by side. `out_dir`
   is relative to the repo root. `asset_view` writes `side`, `front_high`, `rear_low`, `top`
   and `shadow_side` PNGs there and exits. Stop the harness with `tools/hl stop` when done.
   `HL_BIN` has to be set on the `game` command, since that is where `tools/hl` reads it.
   If a model is a black blob from the shadow side, fix its emissives or value contrast.

7. **Commit** `asset.json`, `build.py`, `<name>.glb` and `<name>.intent.json` together.

## The hb API

All coordinates are **Bevy space, metres: +Y up, −Z forward, +X right**, the same as the
`.glb` and the game. Everything is placed in the **asset's frame** (origin = where it's
held, stands or mounts), including the geometry of parts parented under other parts. hb
works out every parent-local transform. Never touch Blender's axes.

```python
import hb

a = hb.Asset(__file__)        # reads asset.json beside the script; fresh empty scene
```

### Nodes

| call | makes |
|---|---|
| `a.part(name, parent=None, pivot=(0,0,0), forward=None, up=None, bevel=None, smooth_angle=30, bevel_segments=2)` | A mesh node, pivoted at `pivot`. The pivot is the joint or socket it moves about. |
| `a.socket("SOCKET_x", parent=None, at=(0,0,0), forward=None, up=None)` | An empty attachment point. |
| `a.collider("COL_x", points=None, around=None, parent=None)` | A convex hull of `points`, or of the vertices of the part(s) in `around`. It has no material. |
| `a.export()` | Finishes and exports everything. It must be the last call. |

- `parent` is a node returned by these calls or its name; `None` means the root (named
  after the asset).
- Orientation: local −Z points along `forward` and +Y toward `up`. If neither is given, the
  node keeps its parent's orientation. If only one is given, the other defaults to the world
  axis (forward −Z, up +Y), so a forward along ±Y needs an explicit `up`. A part that swaps at a socket hangs from that socket
  with its pivot at the socket, so a replacement part only needs its own origin at its
  attach point.
- `bevel` is the part's bevel width in metres. It defaults to the kind's `default_bevel`;
  outside the kind's range you get a note, not an error. `bevel=0` gives no bevel.
- Names: parts are `snake_case`; then `SOCKET_<snake>` and `COL_<snake>`. A duplicate name
  is an error, never a silent `.001`.

### Geometry (methods on a part, all return the part)

Material arguments are role names from `tools/assets/materials.json`; anything else is an
error. `bevel=` on a primitive overrides the part's width for that primitive only (use it
for thin strips and small details).

| call | shape |
|---|---|
| `p.prism(points, plane="zy", span=(a, b), mat=, smooth=False)` | A closed 2D outline extruded. `plane` names the two axes the points are in, in order. `"zy"` means the points are `(z, y)`, a side view, and the extrusion runs across x from `a` to `b`. |
| `p.box(lo, hi, mat=)` | An axis-aligned box from two opposite corners, in any order. |
| `p.frame(lo, hi, wall, axis, mat=)` | A rectangular sleeve: outer box `lo..hi`, open along `axis`, with walls `wall` thick. |
| `p.lathe(profile, axis="z", center=(0,0), mat= or mats=[...], segments=20)` | A surface of revolution. `profile` is `[(radius, t)]` with `t` along the axis. `center` gives the other two axes' coordinates in xyz order (`axis="z"` → `(x, y)`). `mats` takes one role per profile segment. Open ends are capped. |
| `p.cylinder(axis, center, radius, span, mat=, segments=20)` | A capped cylinder. |
| `p.hull(points, mat=)` | The convex hull of `points` (at least 4, not flat). Every face is flat and coplanar sides merge into one face. For wedges, sloped plates and chamfered blocks that a box or prism can't tilt into. |

Primitives are separate closed shells. They may overlap, but they are never welded or
booleaned together. There's no boolean operation; build cut-outs from separate pieces.

### How finishing works (what `export()` does to each part)

Every face is tagged **flat** or **curved** by the primitive that made it:

- Flat: box, frame, prism and hull faces; lathe end caps; lathe radial steps (segments with
  constant `t`).
- Curved: lathe bands (cylinders and cones), and the side faces of `prism(..., smooth=True)`.
  Use that for an outline that approximates a curve.

Then:

1. **Hard edges.** Every edge of a flat face is marked sharp, so flat faces always shade
   exactly flat; no angle threshold can leave a shallow chamfer smoothed. Curved surfaces
   stay smooth across edges shallower than `smooth_angle` (30° by default: a 20-segment
   lathe has 18° facets) and are hard across sharper ones.
2. **Bevel.** Hard edges sharper than 1° are bevelled. Each primitive gets its own width,
   through bevel weights on a weight-limited Bevel modifier. `harden_normals` is on, the
   outer miter is Arc, and widths clamp where they would overlap.
3. **Weighted normals** (face area, keep sharp).

All three are applied on export (`export_apply`). The `.glb` is then read back and its node
names, parents, meshes and world transforms are compared with what was placed, and `<name>.intent.json` records the plane of every flat face, so the
verifier can check that those faces really do shade flat in the file.

## Materials

The roles are defined once, in `tools/assets/materials.json`: `paint_primary`,
`paint_secondary`, `metal_dark`, `metal_bare`, `polymer`, `rubber`, `glass` (alpha-blended),
and `emissive` (emitted colour × 8, exported with `KHR_materials_emissive_strength`; the
exporter normalises the colour so its brightest channel is 1 and scales the strength to
match, so the file's strength is 8 only for colours with an `ff` channel).

- The defaults are neutral greys; the palette in `asset.json` gives the colours.
- The glTF material is named after the role.
- Backface culling is on (`doubleSided: false`).
- A new role is a change to `materials.json` (and to this list), never a one-off in a
  script.

## What the verifier checks

`python3 -I tools/assets/verify_glb.py assets/<group>/<name>` uses the Python stdlib only.
The driver runs it. A FAIL exits 1; a WARN doesn't.

- **Container:** GLB v2 with one embedded buffer. No images, textures, cameras or lights.
- **Tree:** one scene root named after the asset, with an identity transform and no mesh. No node has
  scale or a raw matrix. Every node is reachable. It prints the tree with world positions.
- **Names:**
  - Unique, and every node is a snake_case part, `SOCKET_*` or `COL_*`.
  - Sockets have no mesh. Parts and colliders have a mesh named after their node.
  - The parts and colliders named in `asset.json` exist.
  - WARN if there's no collider.
- **Materials:**
  - Every material is a library role, and each one is used.
  - Metallic and roughness match the library (to 0.001), and so do alpha mode and `doubleSided`.
    Base colour and emitted colour × strength match the palette, or the default.
    Other material extensions (ior, transmission, ...) are not checked.
  - Palette-required roles that are used appear in the palette.
  - The extension is declared in `extensionsUsed`.
  - WARN for palette entries the model doesn't use.
- **Triangles:**
  - The render total (colliders excluded) is within the kind's maximum from `kinds.json`.
    WARN if it's under the minimum.
  - Every primitive has normals, all unit length, and winding agrees with them.
  - Every connected shell has positive volume (catches an inside-out part).
- **Flat shading:**
  - The intent file is for this asset and lists exactly the GLB's parts.
  - Every triangle lying on a plane listed in `intent.json` has all its vertex normals
    within 1° of that plane's normal. A part none of whose planes are found FAILs.
  - WARN when some modelled-flat plane has no triangles left. Coplanar faces count as one
    plane, so a vanished face that shares a plane with a surviving one isn't reported. That usually means the bevel ate a step
    narrower than twice its width.
- **Bounds and orientation:** the bounding-box size per axis is within `expect.size`.
  Each expected socket is present, at `at`, with its `forward` and `up`.
- **Colliders:** each is one closed hull with volume (every edge shared by exactly two
  triangles), convex with outward faces within 0.02 mm, with no material and no children.

## Gotchas (each cost someone time)

- **Blender exits 0 after a Python exception** unless it gets `--python-exit-code 1`. The
  driver passes it. If you run Blender yourself, pass it too.
- **Blender ignores `PYTHONPATH`.** `run_build.py` puts `tools/assets` on `sys.path`, which is
  why build scripts are run through it (by the driver) and not directly.
- **Name clashes are silent in Blender** (`grip` becomes `grip.001`). hb turns them into errors.
- **The parent inverse.** Setting `obj.parent` keeps a hidden `matrix_parent_inverse`, which
  puts things in plausible-looking wrong places. hb sets it to identity, writes the
  parent-local basis, and checks every node's world matrix after the scene updates.
- **Axes.** Blender is Z-up, and the exporter maps Blender (x, y, z) to glTF (x, z, −y).
  `hb/space.py` is the only place that knows this; your script never should.
- **The exporter drops things silently**: hidden objects under the visibility filters, and
  anything excluded by a selection or collection filter. hb turns all the filters off,
  refuses to export if any object is hidden, foreign or not linked, and compares the
  written file's nodes and parents with the scene.
- **`doubleSided`** is exported as true unless the material has backface culling on. hb
  turns it on.
- **Smoothing API in 5.2:** `Mesh.set_sharp_from_angle(angle=)` exists on mesh *instances*
  (`me.set_sharp_from_angle(...)`) but not as `bpy.types.Mesh.set_sharp_from_angle`.
  hb doesn't use it, because it takes one angle per mesh; it marks `edge.smooth = False`
  per edge in bmesh, which writes the `sharp_edge` attribute. Bevel weights are the
  `bevel_weight_edge` edge attribute.
- **Exporter keyword names drift** between Blender versions. Check them with
  `bpy.ops.export_scene.gltf.get_rna_type().properties`.
- **Harmless noise:** `ERROR MeshOptimizer is not available ...` is printed on every export.
  It's an optional compression library that we don't use.
- **The render engine enum** lists only EEVEE until something assigns `'CYCLES'`;
  assigning it works. Previews use Cycles on the CPU because it needs no display.
- **Preview colour.** The previews use the Standard view transform. AgX, Blender's default,
  washes paint out to pastel. They still aren't a colour reference for Bevy.
- **The bevel eats narrow steps.** A flat ring or ledge narrower than about twice the
  bevel width disappears (the verifier WARNs). Widen the step or lower that primitive's
  `bevel=`.

## Known limits

- **Only one emissive role.** An asset can't have two different light colours yet. Add a
  role to `materials.json` when a brief needs one.
- **No booleans.** Doorways and cut-outs would have to be built from pieces. If ship modules
  need booleans, expect bevel pinching and normal cleanup at the seams.
- **No joint limits or articulation data** in the export. Robots get parts pivoted at their
  joints; the limits will need glTF extras or a sidecar when the game wants them.
- **Organic shapes** (flora, fauna) are beyond prisms and lathes. They'll need new
  primitives, such as curve sweeps or Geometry Nodes, with their own shading rules.
- **The previews have no bloom** and don't use Bevy's lighting. That's what `asset_view`
  is for.
