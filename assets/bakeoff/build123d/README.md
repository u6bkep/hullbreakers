# SMG frame: build123d pipeline (bake-off entry)

Code-CAD (build123d 0.13 on OpenCascade 8.0), exported with build123d's own
`export_gltf`, then rewritten by a pure-Python post-processor (numpy + pygltflib).
No Blender at any step.

## Rebuild

```sh
./assets/bakeoff/build123d/build.sh
```

This creates the venv if it is missing, then runs four stages:

| stage | script | does |
|---|---|---|
| 1 | `build_smg.py` | B-rep modelling, chamfers/fillets, `export_gltf` -> `work/raw.glb` + `work/layout.json` |
| 2 | `postprocess.py` | raw.glb -> `smg.glb` (materials, merged primitives, flattened hierarchy, sockets) |
| 3 | `verify.py` | checks smg.glb on its own, exits non-zero on any failure |
| 4 | `preview.py` | pyrender (EGL, headless) PNGs -> `preview/` |

Env overrides: `SCRATCH` (venv and intermediates; defaults to this session's scratch dir), `UV`.
A full rebuild takes about 4 s with the venv present and about 7 s from a clean venv (with a warm uv cache).

## Setup and its friction

- `uv venv -p 3.12` + `uv pip install -r requirements.txt` (build123d, cadquery-ocp-novtk,
  numpy, scipy, pygltflib, trimesh, pyrender, pillow). It took 3.6 s, with no compiling and no errors.
- **Python 3.14 caused no friction.** I expected missing OCP wheels, but a dry run on system 3.14.7
  resolves `cadquery-ocp-novtk 8.0.1.1.0` fine. I used 3.12 anyway so that pyrender and PyOpenGL
  are on well-trodden ground.
- pyrender worked headless with `PYOPENGL_PLATFORM=egl` on the first try.
- Setup was the smallest problem in this pipeline.

## Time and iterations

- Wall clock: about 10 minutes from reading the spec to a verified asset, plus writing this README.
- Build/inspect iterations: 2 API probes (to learn what `export_gltf` actually writes), then
  3 model builds. The first build succeeded. The second shrank the collider and refined the
  tessellation (it was under 2k tris). The third fixed a magazine corner poking through the base plate.
  There were 2 post-process fixes: duplicate node names, and trimesh splitting meshes per primitive
  inside verify. There was also 1 diagnostic round to confirm that the white facets in the previews
  are specular highlights, not broken normals. I checked the slot floor normals numerically: +Y as expected.

## Result

- `smg.glb`, 103 KB, single embedded buffer.
- **2564 visible triangles** + 40 collider triangles. By node: barrel_shroud 1600, grip 308,
  sight_rail 276, receiver 212, trigger_guard 96, magazine 72, COL_body 40.
- Length 0.410 m (Z), height 0.207 m, width 0.037 m. The muzzle is at z = -0.322 and the receiver rear is at z = +0.088.
- Hierarchy: the scene root is `smg`, with an identity transform. Its children are the 6 parts, `COL_body`
  and 5 empty `SOCKET_*` nodes, all as direct children. Each swappable part's node origin is its attach point:
  - `magazine` sits at `SOCKET_mag`, so a reload is a translation along local -Y.
  - `barrel_shroud` sits at `SOCKET_barrel`.
  - `sight_rail` is centred on the receiver top.

  A replacement barrel, magazine or sight therefore only has to be authored with its own origin at its attach point.
- Materials:

  | material | settings |
  |---|---|
  | `paint_primary` | burnt orange, metallic 0, roughness 0.55 |
  | `metal_dark` | metallic 1, roughness 0.4 |
  | `metal_bare` | metallic 1, roughness 0.28 |
  | `polymer` | near-black, roughness 0.85 |
  | `emissive_accent` | cyan, emissive (0.03, 0.69, 1.0) × `KHR_materials_emissive_strength` 8 |

  `COL_body` has no material.

## Verification (`verify.py`, all PASS)

It reads the GLB through a hand-written chunk parser (struct + json) and through trimesh's loader.
It does not use pygltflib, which is the library that wrote the file. It checks:

- **Container:** the GLB header, a single embedded BIN chunk, and no URIs, textures or images.
- **Nodes and hierarchy:**
  - Every required name appears exactly once.
  - There is a single scene root `smg` with an identity transform.
  - The root's children are exactly the 6 parts, 5 sockets and `COL_body`.
  - The sockets have no mesh and no children.
  - Each part's mesh carries the part's name.
- **Materials:**
  - All five named materials exist and every one is used.
  - Metallic and roughness are as specified, and there are no textures.
  - The emissive factor is set, `emissiveStrength` is above 1, and the extension is declared in `extensionsUsed`.
  - `COL_body` has no material.
- **Triangles:** the visible count is between 2k and 6k (2564).
- **Bounds and orientation:**
  - The length along Z is between 0.40 and 0.45. The gun is longest on Z, then Y, then X, and symmetric about X = 0.
  - `SOCKET_grip` is at the origin.
  - `SOCKET_muzzle` is at the -Z extreme, so the gun points -Z.
  - The sight is above the barrel axis, which is above the grip, so +Y is up.
  - The magazine is below the origin.
  - The magazine and barrel_shroud node origins match their sockets.
  - `COL_body` is convex.
- **Normals:**
  - All normals are unit length.
  - The winding agrees with the stored normal on all 2604 triangles.
  - 1392 triangles shade exactly flat (vertex normal = face normal). The other 1212 have varying normals; these are cylinders and fillets.

## Pain points (frank)

**Normals and bevels.** This part went well. OCCT tessellates per B-rep face and writes normals
from the analytic surface. Flat faces and chamfers are therefore exactly flat, cylinders and fillets
are smooth, and every face boundary is a hard split. This is "split normals" for free, with no
weighted-normal fiddling. Chamfering all edges of an extruded profile is one line,
`chamfer(part.edges(), 3)`, and it worked on every part the first time.

- **Edge selection is the real skill.** "The long vertical edges of the grip" became
  `edges().filter_by(Axis.X, reverse=True).filter_by(Axis.Y, reverse=True)`. That is fine for
  prisms but would get painful on organic or compound shapes.
- **Tessellation density is global.** You set one `angular_deflection` for the whole export, so the
  cylinders took 62% of the triangles (shroud + barrel = 1600) while the boxes cost almost nothing.
  There is no per-part LOD knob without tessellating parts separately.
- **Hidden overlaps are invisible in code.** Separate solids that intersect (the magazine inside the
  base plate) poke through at the chamfers, and you only find out from a render.

**Materials.** This needed the most fixing.

- build123d has only `shape.color`, with no PBR fields.
- OCCT writes the materials as `mat_0..N` with metallic 1, roughness 1, `doubleSided: true`,
  no emissive and no names.
- `Color()` input is sRGB and is written out linearised.

So I used each colour as a lookup key and the post-processor maps the exact colours back to named
materials. It also sets metallic, roughness and emissive, adds `KHR_materials_emissive_strength`, and
turns off `doubleSided`. This works but is brittle: two materials with the same colour would collide.

**Sockets and hierarchy.**

- An empty `Compound` cannot be created at all (it asserts), and a `Vertex` exports as a degenerate
  mesh node. So build123d has no way to emit an empty node, and the sockets are added in post.
- Every leaf shape becomes its own child node, and the exporter repeats names: a part node `grip`
  has a child mesh node `grip`.
- `shape.moved()` and `shape.rotate()` are **not** baked into the geometry. They end up as TopLoc
  transforms on the leaf nodes, so each leaf node carries an inverse rotation and translation.
- The post-processor therefore flattens each part's subtree into one mesh on the part node, baking
  the leaf transforms.

**Export.**

- `export_gltf` writes **one primitive per B-rep face**: about 320 primitives for this gun, and 26 for
  a single chamfered box. That means one draw call each in Bevy. OCCT has
  `RWGltf_CafWriter::SetMergeFaces`, but build123d does not expose it.
- The post-processor merges everything into one primitive per material per part, which gives 11
  primitives in total.
- `TEXCOORD_0` is forced on, with OCCT surface-parameter UVs that are useless here. I dropped it.
- Net result: the post-processor (178 lines) rewrites essentially every top-level glTF array. It is
  effectively a second exporter that uses OCCT only as a tessellator.

**Coordinate conventions.** The CAD frame is Z-up. `export_gltf` silently adds a -90° X rotation on the
root node, so a naive export gives a rotated root. I modelled in Z-up with +Y as forward, rotated each
part into glTF axes, and gave the root a +90° X rotation that cancels the exporter's. That is a working
convention but obscure; the docstring only hints at it. Units are clean: model in mm, `unit=Unit.MM`,
and the output is in metres.

**Preview.** I did not use trimesh's loader for rendering, because by default it re-merges vertices
and recomputes normals, so it would not show the file's real shading. I wrote a small pygltflib to
pyrender path that feeds the stored normals in directly. There is no bloom, so the emissive strength
is clamped in previews. The renders are good for silhouette and normal checks but say nothing about
how Bevy's tonemapping will look. Bright facets on the metal corner chamfers and slot floors are
specular highlights; I confirmed this numerically.

**Collider.** build123d has no 3D convex hull, so I took a 2D hull of the side profile with scipy and
extruded it. The result is a convex prism over receiver, rail, shroud and magwell. The grip and
magazine hang outside it, because a single convex hull over the whole L-shape is mostly empty space.
A dropped gun's grip may therefore clip into the floor. **This only partly meets the spec:** it is one
convex body, as asked, but it does not cover the whole gun.

## What would break when scaling

- **Robots (articulated rigid parts): good fit.** Parts are solids with exact attach points, and the
  part-origin = socket convention scales. Joints and pivots are just more empty nodes. Two gaps:
  - Animation and skinning are outside build123d; you would add them in post or in the engine.
  - Pivot placement must be computed rather than eyeballed.
- **Organic flora: wrong tool.** B-rep CAD has no sculpting, no subdivision surfaces, no vertex colour,
  no UV unwrapping and no alpha-cut cards. Leaves and trunks as lofts or sweeps are possible, but
  they tessellate into dense, unoptimised meshes, and the per-face-primitive export would explode. Use
  a different generator (procedural mesh in Python or Rust, or Blender).
- **Ship modules: good fit, with a couple of costs.** Snap-grid dimensions, boolean cutouts (doors,
  ports) and chamfered panels are what CAD is best at.
  - Large hulls with many booleans will hit OCCT's fillet and boolean fragility.
  - The global tessellation setting will spend triangles on round things.
  - Interiors need many material zones, and the colour-as-material-key hack should be replaced by
    tagging material names in `shape.label` or a sidecar.
- **Across all three: own the exporter.** The post-processor is the real exporter. The long-term version
  should tessellate with OCP (`BRepMesh_IncrementalMesh` + `BRep_Tool.Triangulation`) and write glTF
  directly, rather than patching `export_gltf`'s output.

## Spec items not fully met

- The collider covers the main body only; see above.
- The emissive bloom strength (8) is unverified in-engine; the previews have no bloom.
- Normals are per-B-rep-face split, not "weighted". For this geometry that is equivalent or better,
  since every planar face is exactly flat.
