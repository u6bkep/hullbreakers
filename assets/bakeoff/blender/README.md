# Bake-off: SMG frame, built with Blender 5.2 (headless Python)

## Rebuild

```sh
./build.sh
```

It deletes `smg.glb` and `preview/*.png`, runs
`blender --background --factory-startup --python build_smg.py -- --render` (builds,
exports, renders the five previews), then `python3 -I verify_glb.py smg.glb`.
To export only, with no renders (about 0.7 s): `blender -b --factory-startup -P build_smg.py`.

Files:
- `build_smg.py` builds the whole scene from code. It never reads a .blend, and
  `--factory-startup` plus `read_factory_settings(use_empty=True)` keep the user's prefs and add-ons out of it.
- `verify_glb.py` is an independent checker. It parses the GLB with the Python stdlib only (no Blender, no pygltflib).
- `smg.glb` is the output: 3,856 triangles plus a 24-triangle collider.
- `preview/` holds Cycles renders: side, 3/4 front, 3/4 rear, mag dropped, and a bevel close-up.

## Setup and friction

There was no setup. `/usr/bin/blender` (5.2.2 LTS) was already installed, and the glTF
exporter and Cycles ship with it. No venv or pip was needed. The verifier is stdlib only
because `pygltflib` wasn't installed, and I didn't want to add a dependency just to check the file.

API drift was the only setup cost. I spent two short probe runs on it:
- `Mesh.set_sharp_from_angle` / `shade_smooth_by_angle` (the 4.1+ replacement for auto-smooth)
  can't be called from `bpy.types.Mesh` in 5.2. I marked sharp edges by hand in bmesh
  (`edge.smooth = False` where `calc_face_angle() > threshold`).
- The engine enum only listed `BLENDER_EEVEE` until Cycles was assigned. Assigning `'CYCLES'` worked.
- Exporter kwarg names had to be checked against `bpy.ops.export_scene.gltf.get_rna_type()`.
  Your training-data memory of these names is a few versions stale.

## Wall-clock and iterations

- About 9 minutes of wall-clock from reading the spec to the final verified build, including the README.
- A full rebuild takes about 37 s on 32 cores, almost all of it Cycles CPU rendering 5 frames at 96 spp
  with denoising. Export alone takes 0.7 s.
- 6 build runs in total:
  - Build 1: geometry right the first time and all structural checks passed.
  - Renders 1–3: lighting was too hot and the paint washed out to peach under AgX.
    The magazine fell through the ground plane in the reload shot. The framing was fixed.
  - Build 4: fixed shading (see below).
  - Build 5: final clean `./build.sh`.
- 4 verifier revisions. The flat-shading check needed three attempts before it told apart
  "bevel strip, meant to be smooth" from "flat face with a smoothing gradient".

## What was verified (by `verify_glb.py`, reading the GLB directly)

- **Container.** GLB v2 header, one embedded BIN buffer, no external URIs, no images or textures.
- **Node names.** The six part nodes each have a mesh. The five `SOCKET_*` nodes have none. `COL_body` has a mesh. Names are unique.
- **Hierarchy.** The tree is printed with world positions:
  ```
  smg
    COL_body
    receiver
      grip
      sight_rail
        SOCKET_sight
      SOCKET_barrel
        barrel_shroud
          SOCKET_muzzle
      SOCKET_mag
        magazine
      trigger_guard
    SOCKET_grip
  ```
  - Each swappable part (barrel, magazine) is the only child of its socket, and its pivot
    is at the socket. A randomised loadout swaps the socket's child. A reload animates
    `magazine` along its local −Y.
  - `SOCKET_muzzle` hangs off the barrel, so a longer barrel carries its muzzle with it.
  - `SOCKET_sight` sits on top of `sight_rail`.
- **Materials.**
  - All five are present by name, and nothing else is.
  - `metal_dark` has metallic 1 and roughness 0.4.
  - `paint_primary` has roughness 0.55.
  - `metal_bare` has metallic 1 and roughness 0.28.
  - `polymer` has metallic 0 and roughness 0.85.
  - `emissive_accent` has emissiveFactor (0.15, 0.85, 1.0) and
    `KHR_materials_emissive_strength` = 8, declared in `extensionsUsed`.
  - `COL_body` has no material.
- **Triangles.** The total is 3,856, excluding the collider. By part: receiver 908, grip 284,
  trigger_guard 408, sight_rail 756, barrel_shroud 1,176, magazine 324. The collider is 24.
  All normals are unit length.
- **Bounds.** The render meshes span x −0.023..+0.025, y −0.082..+0.141, z −0.300..+0.128 m.
  That is 0.428 m long on Z, 0.22 m tall and 0.048 m wide.
- **Orientation.**
  - `SOCKET_muzzle` is at z = −0.300 (forward) on the centreline.
  - `SOCKET_sight` is at +Y.
  - The magazine socket is forward of and above the grip.
  - The ejection-port block is on +X (right). This proves the X handedness survived the Z-up to Y-up conversion.
  - `SOCKET_grip` is exactly at the origin.
  - All sockets have identity rotation, so local −Z is forward and +Y is up.
- **Flat shading.**
  - What it checks: the part's triangles are grouped into planes. A plane whose effective
    width (area / longest extent) is at least 6 mm counts as a real face. Every vertex
    normal on a real face must match the face normal to within 1°.
  - Result: receiver, grip, trigger_guard, magazine and sight_rail all pass with a maximum deviation of 0.02°.
  - Not covered: `barrel_shroud` is excluded on purpose, because its 20 lathe facets are meant to be smooth.

## Pain points (frank)

### Normals and bevels
- The Bevel (harden normals) + Weighted Normal (face area, keep sharp) stack is the
  right tool, and it works headless with `export_apply=True`. But **the sharp-angle
  threshold is a trap.** My first pass used 30° everywhere. The receiver's 15°
  front-top chamfer and the grip's beavertail bends stayed "smooth" and showed a
  13–25° normal gradient across small flat faces. The renders didn't make this
  obvious. Only the numeric check found it. The fix was per-part thresholds:
  5° for hard-surface profiles and 30° for lathed parts, whose 18° facets must stay smooth.
- One threshold per object is coarse. A part that is both boxy and round needs either
  hand-marked sharp edges or two objects. Here the barrel's top fins share its 30° rule,
  which happens to work because they are boxes.
- I had to write my own "is it actually flat?" check. Blender gives you nothing
  headless that says the normals are right. The viewport is the usual QA tool, and you don't have it.
- I couldn't drive the bevel width per edge from code without bevel-weight attributes,
  so each part has one width. The thin emissive strips get clamped by `use_clamp_overlap`.
  That is fine here, but you get no say in it.

### Materials
- This was easy. The Principled BSDF maps 1:1 onto glTF metallic-roughness.
- An emission strength above 1 automatically produces `KHR_materials_emissive_strength`.
- Blender 5.x materials always have node trees, so code that sets `use_nodes` is now dead code.
- Colours are linear in Blender and linear in glTF, so there's no conversion. But AgX in the
  preview renders made the paint look much paler than the factor suggests.
  **The previews aren't a colour reference for Bevy.** Different tonemapper, no bloom
  (Cycles has no bloom without compositor nodes), different lights.

### Sockets and hierarchy
- Empties export as plain glTF nodes. Parenting works, and object names become node names and mesh names.
- The trap is `matrix_parent_inverse`. Set `obj.parent` and Blender's operator
  path would bake a hidden inverse. From raw `bpy.data` I set the inverse to identity and
  wrote `location` as parent-local by hand (a tiny `place()` helper). If you forget,
  sockets end up at plausible-looking but wrong positions.
- The exporter adds no wrapper root and doesn't reorder nodes. The node order in the file
  doesn't match creation order, which is harmless.

### Export
- `export_apply=True` bakes modifiers. Custom split normals survive, and the GLB came out clean on the first try.
- The exporter is silent about what it dropped. For example, if the collider had
  `hide_render` set and `use_renderable=True`, it would vanish without a warning.
  I left the visibility filters off on purpose.
- The exporter logs are verbose INFO lines. They're fine, but I grepped for errors myself.

### Coordinate conventions
- Blender is Z-up. The exporter's `export_yup` maps (x, y, z) to (x, z, −y). So
  "gun points −Z in Bevy" means **"model it pointing +Y in Blender"**, and "up" is +Z.
  - Once it's written in the script header it's easy. It's also easy to get backwards.
  - The verifier checks it explicitly in glTF space: muzzle at −Z, ejection port at +X.
- Socket "down" for the mag drop is Blender −Z, which is glTF −Y. Same mapping, still a mental hop.

### Preview
- Cycles CPU worked headless with no fuss. Each 1280×800 frame took about 7 s at 96 spp
  with OIDN on 32 cores. I didn't try EEVEE headless; Cycles was the safe choice with no display.
- The camera and lights are hand-placed with `to_track_quat`. Each framing fix costs a full re-render cycle.
- Previews can't show the real failure mode, which is shading gradients on flat faces under Bevy's lights.
  The numeric normal check was more valuable than the pictures.

## What breaks when scaling up

### Robots (articulated rigid parts)
- The parent/socket pattern here scales. Joints would be empties with a pivot at the hinge.
- What hurts:
  - The `matrix_parent_inverse` bookkeeping, and the axis mapping applied to every joint axis.
  - Joint limits and axes don't export from empties. They'd need glTF `extras`
    (`export_extras=True` plus custom properties) or a sidecar file. Either way the convention is ours to invent.
- Rigging through an Armature with rigid skinning is possible, but heavier, and it's harder to verify from a script.

### Organic flora
- Profile extrusion and lathe don't reach organic shapes. You'd want Geometry Nodes
  (curves to mesh, instancing, randomised leaves).
  - Driving Geometry Nodes from Python means building node graphs in code.
    It's verbose and the API changes between versions.
- Smooth-shading rules invert. My flat-face checker becomes meaningless, and LOD and triangle budget become the problem.
- Without textures, flora will look plain. That's a spec limit, not a Blender limit.

### Ship modules
- Large modular kits need exact snapping, so grid-locked dimensions and connector sockets matter.
  - Blender floats are fine for this, but you need a shared dimension module across scripts.
  - Every module repeats the bevel threshold problem above.
- Batch-exporting many modules is easy: one scene per module, or `use_selection`.
- Booleans (doorways, cut-outs) are where Blender scripting gets fragile:
  - Results depend on operand order and solver ('EXACT' vs 'FAST').
  - Bevels after booleans pinch.
  - Weighted normals across boolean seams need cleanup that a human would normally do in the viewport.
- Shared materials across many GLBs are duplicated per file. Nothing in this pipeline
  deduplicates them; that would have to happen on the Bevy side.

## Spec items not fully met
- **Bloom in previews.** Cycles renders don't show bloom. The emissive is correct in the
  file (strength 8), but the previews can't show the glow.
- **Bevel size.** Bevels are 2–4 mm on the main bodies, 1.5 mm on the rail teeth, and clamped
  smaller on the 1–1.5 mm emissive strips.
- Nothing else in the spec was dropped.
