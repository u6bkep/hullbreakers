"""Preview renders: Cycles on the CPU (works headless with no display), auto-framed.

These are for checking shape, proportion and colour blocking quickly. They are NOT the
in-engine look: no bloom, different lights, different tone mapper. Judge the final model
in asset_view (docs/assets.md). View transform is 'Standard', not AgX, so colours read
close to their values instead of washing out.
"""
import math
import os

import bpy
from mathutils import Vector

from .space import to_blender

# Directions from the model toward the camera, in Bevy space (+Y up, -Z forward, +X right).
VIEWS = {
    "right": (1.0, 0.12, 0.0),
    "left": (-1.0, 0.12, 0.0),
    "front_3q": (0.6, 0.4, -0.7),
    "rear_3q": (-0.55, 0.35, 0.75),
}
RES = (1280, 800)
LENS = 50.0
SAMPLES = 48


def _bounds(asset):
    dg = bpy.context.evaluated_depsgraph_get()
    lo, hi = Vector((math.inf,) * 3), Vector((-math.inf,) * 3)
    for node in asset.nodes.values():
        if node.kind != "part":
            continue
        ob = node.obj.evaluated_get(dg)
        me = ob.to_mesh()
        for v in me.vertices:
            w = ob.matrix_world @ v.co
            for k in range(3):
                lo[k], hi[k] = min(lo[k], w[k]), max(hi[k], w[k])
        ob.to_mesh_clear()
    return lo, hi


def _look(ob, target):
    ob.rotation_euler = (target - ob.location).to_track_quat("-Z", "Y").to_euler()


def render(asset):
    scene = bpy.context.scene
    out_dir = os.path.join(asset.dir, "preview")
    os.makedirs(out_dir, exist_ok=True)
    for f in os.listdir(out_dir):
        if f.endswith(".png"):
            os.remove(os.path.join(out_dir, f))
    for node in asset.nodes.values():
        if node.kind == "collider":
            node.obj.hide_render = True
    # The .glb is already written. Without bloom, strength 8 just clips to white; at 1 the
    # preview shows the emissive's hue instead.
    for m in bpy.data.materials:
        bsdf = next((n for n in m.node_tree.nodes if n.type == "BSDF_PRINCIPLED"), None)
        if bsdf is not None and bsdf.inputs["Emission Strength"].default_value > 1.0:
            bsdf.inputs["Emission Strength"].default_value = 1.0

    lo, hi = _bounds(asset)
    center = (lo + hi) / 2
    radius = max((hi - lo).length / 2, 1e-3)

    scene.render.engine = "CYCLES"
    scene.cycles.device = "CPU"
    scene.cycles.samples = SAMPLES
    scene.cycles.use_denoising = True
    scene.render.resolution_x, scene.render.resolution_y = RES
    scene.render.film_transparent = False
    scene.view_settings.view_transform = "Standard"
    scene.view_settings.look = "None"
    scene.view_settings.exposure = 0.0
    scene.view_settings.gamma = 1.0

    world = bpy.data.worlds.new("preview_world")
    scene.world = world
    bg = world.node_tree.nodes["Background"]
    bg.inputs["Color"].default_value = (0.05, 0.055, 0.065, 1.0)
    bg.inputs["Strength"].default_value = 1.0

    me = bpy.data.meshes.new("preview_ground")
    s = radius * 30
    z = lo.z - 0.002
    me.from_pydata([(-s, -s, z), (s, -s, z), (s, s, z), (-s, s, z)], [], [(0, 1, 2, 3)])
    ground = bpy.data.objects.new("preview_ground", me)
    gm = bpy.data.materials.new("preview_ground")
    b = gm.node_tree.nodes["Principled BSDF"]
    b.inputs["Base Color"].default_value = (0.03, 0.03, 0.035, 1.0)
    b.inputs["Roughness"].default_value = 0.8
    me.materials.append(gm)
    scene.collection.objects.link(ground)

    cam_d = bpy.data.cameras.new("preview_cam")
    cam_d.lens = LENS
    cam_d.clip_start = radius * 0.01
    cam_d.clip_end = radius * 200
    cam = bpy.data.objects.new("preview_cam", cam_d)
    scene.collection.objects.link(cam)
    scene.camera = cam
    half_v = math.atan((cam_d.sensor_width / 2) / LENS * RES[1] / RES[0])
    dist = radius / math.sin(half_v) * 1.02

    lights = []
    for name, energy, size in (("key", 1.0, 1.0), ("fill", 0.35, 1.5), ("rim", 0.8, 0.8)):
        ld = bpy.data.lights.new(f"preview_{name}", "AREA")
        ld.size = radius * size
        lo_ = bpy.data.objects.new(f"preview_{name}", ld)
        scene.collection.objects.link(lo_)
        lights.append((lo_, energy))

    up = Vector((0, 0, 1))
    for view, d_g in VIEWS.items():
        d = to_blender(d_g).normalized()
        cam.location = center + d * dist
        _look(cam, center)
        right = d.cross(up).normalized() * -1      # camera's right, in world
        # Lights follow the camera: key high camera-right, fill low camera-left, rim behind.
        place = {
            "key": (d + right * 0.9 + up * 0.9),
            "fill": (d - right * 1.0 + up * 0.1),
            "rim": (-d + up * 0.8 - right * 0.3),
        }
        for ob, energy in lights:
            key = ob.name.removeprefix("preview_")
            ob.location = center + place[key].normalized() * dist * 1.2
            _look(ob, center)
            ob.data.energy = energy * 22.0 * (dist * 1.2) ** 2
        scene.render.filepath = os.path.join(out_dir, f"{view}.png")
        bpy.ops.render.render(write_still=True)
        print(f"hb: rendered {scene.render.filepath}")
