"""Preview renders of the carrier from where players see it: outside, and inside its rooms.

    blender --background --factory-startup --python-exit-code 1 \
        --python assets/ships/carrier/render_views.py [-- view ...]

Imports the exported carrier.glb (so it shows what the game loads), puts a point light at
each of the game's ceiling lights (the design's, from layout.json), a dim sun
outside, and renders each view to preview/view_<name>.png (git-ignored). Like the hb
previews these are for form and colour blocking: Cycles, no bloom, emissives shown at
strength 1 so their hue reads. Judge the real look in-engine.
"""
import json
import math
import os
import sys

import bpy
from mathutils import Vector

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(HERE, "preview")

# The game's ceiling lights (the design's lights, in layout.json).
with open(os.path.join(HERE, "layout.json")) as _f:
    LAMPS = [tuple(p) for p in json.load(_f)["lights"]]

# name: (eye, look at, vertical fov in degrees, outside?) in Bevy space (+Y up, bow -Z).
VIEWS = {
    "outside_3q": ((-62.0, 30.0, -58.0), (0.0, 2.0, 2.0), 40, True),
    "outside_rear_3q": ((48.0, 16.0, 70.0), (0.0, 3.0, 8.0), 40, True),
    "hangar_door": ((9.0, 4.0, 52.0), (-0.5, 2.6, 30.0), 40, True),
    "hangar": ((3.8, 1.75, 29.2), (-2.0, 2.6, 15.0), 85, False),
    "hangar_ramp": ((-4.6, 5.0, 14.4), (2.0, 1.0, 26.0), 85, False),
    "bridge": ((1.2, 5.0, -22.5), (-0.3, 4.3, -30.0), 85, False),
    "lower_corridor": ((0.4, 1.7, 12.5), (0.0, 1.6, -20.0), 85, False),
    "upper_corridor": ((-0.4, 5.0, 12.5), (0.0, 4.9, -20.0), 85, False),
}


def g2b(p):
    """Bevy (x, y, z) -> Blender (x, -z, y)."""
    return Vector((p[0], -p[2], p[1]))


def main():
    want = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else list(VIEWS)
    bpy.ops.wm.read_factory_settings(use_empty=True)
    bpy.ops.import_scene.gltf(filepath=os.path.join(HERE, "carrier.glb"))
    for m in bpy.data.materials:
        if not m.node_tree:
            continue
        for n in m.node_tree.nodes:
            if n.type == "BSDF_PRINCIPLED" and n.inputs["Emission Strength"].default_value > 1.0:
                n.inputs["Emission Strength"].default_value = 1.0
            if n.type == "EMISSION" and n.inputs["Strength"].default_value > 1.0:
                n.inputs["Strength"].default_value = 1.0

    scene = bpy.context.scene
    scene.render.engine = "CYCLES"
    scene.cycles.device = "CPU"
    scene.cycles.samples = 48
    scene.cycles.use_denoising = True
    scene.render.resolution_x, scene.render.resolution_y = 1280, 800
    scene.view_settings.view_transform = "Standard"
    scene.view_settings.look = "None"
    world = bpy.data.worlds.new("space")
    scene.world = world
    world.node_tree.nodes["Background"].inputs["Color"].default_value = (0.012, 0.014, 0.02, 1.0)
    world.node_tree.nodes["Background"].inputs["Strength"].default_value = 1.0

    sun = bpy.data.objects.new("sun", bpy.data.lights.new("sun", "SUN"))
    sun.data.energy = 2.5
    sun.data.angle = math.radians(1.0)
    scene.collection.objects.link(sun)
    d = g2b((-0.45, -0.75, -0.5)).normalized()           # light travelling down, from upper right fore
    sun.rotation_euler = d.to_track_quat("-Z", "Y").to_euler()
    for k, p in enumerate(LAMPS):
        ld = bpy.data.lights.new(f"lamp{k}", "POINT")
        ld.energy = 350.0
        ld.color = (1.0, 0.92, 0.8)
        ld.shadow_soft_size = 0.1
        ob = bpy.data.objects.new(f"lamp{k}", ld)
        ob.location = g2b(p)
        scene.collection.objects.link(ob)

    cam = bpy.data.objects.new("cam", bpy.data.cameras.new("cam"))
    scene.collection.objects.link(cam)
    scene.camera = cam
    cam.data.clip_start = 0.05
    cam.data.clip_end = 1000.0
    cam.data.sensor_fit = "VERTICAL"
    os.makedirs(OUT, exist_ok=True)
    for name in want:
        eye, at, fov, outside = VIEWS[name]
        cam.location = g2b(eye)
        cam.rotation_euler = (g2b(at) - g2b(eye)).to_track_quat("-Z", "Y").to_euler()
        cam.data.angle = math.radians(fov)
        # outside, a little sky fill so the shadow side isn't pure black
        world.node_tree.nodes["Background"].inputs["Strength"].default_value = 4.0 if outside else 1.0
        scene.render.filepath = os.path.join(OUT, f"view_{name}.png")
        bpy.ops.render.render(write_still=True)
        print(f"render_views: wrote {scene.render.filepath}")


main()
