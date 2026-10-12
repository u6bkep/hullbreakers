"""Blender side of tools/assets/compose (run it through that, not directly).

    blender -b --factory-startup --python compose_scene.py -- OUT[:views] receiver.glb slot=part.glb ...

Imports the receiver, hangs each part on its socket (a muzzle device or an emitter on the
barrel's SOCKET_muzzle), prints the bounds in Bevy axes and renders orthographic views to
OUT_<view>.png. Colliders (COL_*) are hidden; emission is toned down so a glowing part
doesn't wash out the render.
"""
import sys

import bpy
from mathutils import Vector

args = sys.argv[sys.argv.index("--") + 1:]
out, receiver = args[0], args[1]
parts = dict(a.split("=", 1) for a in args[2:])
bpy.ops.wm.read_factory_settings(use_empty=True)


def load(path):
    """Imports a glb: its root and every object it brought."""
    before = set(bpy.data.objects)
    bpy.ops.import_scene.gltf(filepath=path)
    new = [o for o in bpy.data.objects if o not in before]
    roots = [o for o in new if o.parent is None]
    assert len(roots) == 1, f"{path}: {len(roots)} roots"
    return roots[0], new


def socket(objs, name):
    for o in objs:
        if o.name.split(".")[0] == name:
            return o
    raise KeyError(name)


_, receiver_objs = load(receiver)
bpy.context.view_layer.update()
everything = list(receiver_objs)
on_receiver = {"barrel": "SOCKET_barrel", "mag": "SOCKET_mag", "sight": "SOCKET_sight", "stock": "SOCKET_stock"}
barrel = None
for slot, path in parts.items():
    if slot not in on_receiver:
        continue
    root, objs = load(path)
    bpy.context.view_layer.update()
    root.matrix_world = socket(receiver_objs, on_receiver[slot]).matrix_world.copy()
    everything += objs
    if slot == "barrel":
        barrel = objs
for slot in ("muzzle", "emitter"):
    if slot in parts:
        if barrel is None:
            raise SystemExit(f"COMPOSE: a {slot} needs a barrel to hang on")
        bpy.context.view_layer.update()
        root, objs = load(parts[slot])
        bpy.context.view_layer.update()
        root.matrix_world = socket(barrel, "SOCKET_muzzle").matrix_world.copy()
        everything += objs
bpy.context.view_layer.update()
for o in everything:
    if o.name.startswith("COL_"):
        o.hide_render = True

# Bounds, in Bevy axes (Blender z up, -y forward -> Bevy y up, -z forward).
lo, hi = Vector((1e9,) * 3), Vector((-1e9,) * 3)
for o in everything:
    if o.type == "MESH" and not o.hide_render:
        for v in o.data.vertices:
            w = o.matrix_world @ v.co
            for k in range(3):
                lo[k] = min(lo[k], w[k])
                hi[k] = max(hi[k], w[k])
print("COMPOSE bounds bevy: x %.3f..%.3f  y %.3f..%.3f  z %.3f..%.3f  length %.3f" % (
    lo.x, hi.x, lo.z, hi.z, -hi.y, -lo.y, hi.y - lo.y))

for m in bpy.data.materials:
    if m.node_tree:
        bsdf = next((n for n in m.node_tree.nodes if n.type == "BSDF_PRINCIPLED"), None)
        if bsdf and bsdf.inputs["Emission Strength"].default_value > 1:
            bsdf.inputs["Emission Strength"].default_value = 1.0

scene = bpy.context.scene
scene.render.engine = "CYCLES"
scene.cycles.device = "CPU"
scene.cycles.samples = 24
scene.cycles.use_denoising = True
scene.render.resolution_x, scene.render.resolution_y = 1600, 700
scene.view_settings.view_transform = "Standard"
world = bpy.data.worlds.new("world")
scene.world = world
world.node_tree.nodes["Background"].inputs["Color"].default_value = (0.05, 0.055, 0.065, 1)
center, radius = (lo + hi) / 2, (hi - lo).length / 2
cam_data = bpy.data.cameras.new("camera")
cam_data.type = "ORTHO"
cam_data.ortho_scale = (hi - lo).length * 1.05
cam_data.clip_start, cam_data.clip_end = 0.01, 100
cam = bpy.data.objects.new("camera", cam_data)
scene.collection.objects.link(cam)
scene.camera = cam


def aim(ob, target):
    ob.rotation_euler = (target - ob.location).to_track_quat("-Z", "Y").to_euler()


lights = []
for name, energy in (("key", 1.0), ("fill", 0.4), ("rim", 0.8)):
    light = bpy.data.lights.new(name, "AREA")
    light.size = radius
    ob = bpy.data.objects.new(name, light)
    scene.collection.objects.link(ob)
    lights.append((ob, energy))

VIEWS = {"right": Vector((1, 0, 0.1)), "left": Vector((-1, 0, 0.1)), "top": Vector((0.02, 0.0, 1)),
         "q3": Vector((0.7, -0.6, 0.45)), "rq3": Vector((-0.6, 0.7, 0.4))}
wanted = out.split(":")[1].split(",") if ":" in out else list(VIEWS)
out = out.split(":")[0]
unknown = [v for v in wanted if v not in VIEWS]
if unknown:
    raise SystemExit(f"COMPOSE: unknown views {unknown}; have {', '.join(VIEWS)}")
up = Vector((0, 0, 1))
for view in wanted:
    d = VIEWS[view].normalized()
    dist = radius * 4
    cam.location = center + d * dist
    aim(cam, center)
    right = d.cross(up)
    if right.length < 0.2:
        right = Vector((1, 0, 0))
    right = right.normalized() * -1
    places = {"key": d + right * 0.9 + up * 0.9, "fill": d - right + up * 0.1, "rim": -d + up * 0.8}
    for ob, energy in lights:
        ob.location = center + places[ob.name].normalized() * dist
        aim(ob, center)
        ob.data.energy = energy * 22 * dist ** 2
    scene.render.filepath = f"{out}_{view}.png"
    bpy.ops.render.render(write_still=True)
    print(f"COMPOSE wrote {scene.render.filepath}")
