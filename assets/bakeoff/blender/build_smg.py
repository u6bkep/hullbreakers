"""Build the bake-off SMG frame in Blender (headless) and export smg.glb.

Run:  blender --background --factory-startup --python build_smg.py [-- --render]

Coordinate note: Blender is Z-up. The glTF exporter (export_yup=True) maps
Blender (x, y, z) -> glTF (x, z, -y). So in this script:
    +X = right, +Y = FORWARD (becomes glTF -Z), +Z = up (becomes glTF +Y).
All dimensions are metres. The world origin is SOCKET_grip.
"""
import math
import os
import sys

import bmesh
import bpy
from mathutils import Matrix, Vector

HERE = os.path.dirname(os.path.abspath(__file__))
OUT_GLB = os.path.join(HERE, "smg.glb")
PREVIEW_DIR = os.path.join(HERE, "preview")
ARGS = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else []

# Hard-surface parts: EVERY profile corner is a hard (bevelled) edge, even shallow 15 deg
# ones, otherwise the small faces next to them pick up a smoothing gradient.
FLAT_SHARP_ANGLE = math.radians(5.0)
# Lathed parts: 20 segments = 18 deg between facets, which must stay smooth.
ROUND_SHARP_ANGLE = math.radians(30.0)

# --------------------------------------------------------------------------
# scene reset
# --------------------------------------------------------------------------
bpy.ops.wm.read_factory_settings(use_empty=True)
scene = bpy.context.scene
scene.unit_settings.system = "METRIC"
scene.unit_settings.scale_length = 1.0

# --------------------------------------------------------------------------
# materials
# --------------------------------------------------------------------------


def make_material(name, base, metallic, roughness, emissive=None, strength=0.0):
    m = bpy.data.materials.new(name)
    # Blender 5.x: materials always use nodes; Principled BSDF is created by default.
    bsdf = next(n for n in m.node_tree.nodes if n.type == "BSDF_PRINCIPLED")
    bsdf.inputs["Base Color"].default_value = (*base, 1.0)
    bsdf.inputs["Metallic"].default_value = metallic
    bsdf.inputs["Roughness"].default_value = roughness
    if emissive is not None:
        bsdf.inputs["Emission Color"].default_value = (*emissive, 1.0)
        bsdf.inputs["Emission Strength"].default_value = strength
    m.diffuse_color = (*base, 1.0)
    return m


MAT = {
    # linear-space colours
    "paint_primary": make_material("paint_primary", (0.55, 0.15, 0.025), 0.0, 0.55),
    "metal_dark": make_material("metal_dark", (0.05, 0.055, 0.06), 1.0, 0.4),
    "metal_bare": make_material("metal_bare", (0.62, 0.62, 0.64), 1.0, 0.28),
    "polymer": make_material("polymer", (0.025, 0.025, 0.03), 0.0, 0.85),
    # strength > 1 makes the exporter emit KHR_materials_emissive_strength
    "emissive_accent": make_material(
        "emissive_accent", (0.02, 0.05, 0.06), 0.0, 0.4, emissive=(0.15, 0.85, 1.0), strength=8.0
    ),
}

# --------------------------------------------------------------------------
# geometry helpers (all build into a bmesh in WORLD coords, then get shifted
# so that the object's origin sits at its mount socket)
# --------------------------------------------------------------------------


class Part:
    def __init__(self, name, mats):
        self.name = name
        self.bm = bmesh.new()
        self.mats = mats  # list of material names, index = material slot

    def slot(self, matname):
        return self.mats.index(matname)

    def profile(self, pts_yz, x0, x1, mat):
        """Extrude a closed side profile (y, z points, CCW or CW) across x0..x1."""
        bm = self.bm
        a = [bm.verts.new((x0, y, z)) for (y, z) in pts_yz]
        b = [bm.verts.new((x1, y, z)) for (y, z) in pts_yz]
        n = len(pts_yz)
        faces = [bm.faces.new(a[::-1]), bm.faces.new(b)]
        for i in range(n):
            j = (i + 1) % n
            faces.append(bm.faces.new((a[i], a[j], b[j], b[i])))
        for f in faces:
            f.material_index = self.slot(mat)
        bmesh.ops.recalc_face_normals(bm, faces=faces)
        return faces

    def box(self, lo, hi, mat):
        (x0, y0, z0), (x1, y1, z1) = lo, hi
        return self.profile([(y0, z0), (y1, z0), (y1, z1), (y0, z1)], x0, x1, mat)

    def tube_y(self, outer, inner, z0, z1, mat):
        """Rectangular sleeve: outer/inner = (x0, x1, y0, y1) rings, open along Z."""
        bm = self.bm

        def ring(r, z):
            x0, x1, y0, y1 = r
            return [bm.verts.new(p) for p in ((x0, y0, z), (x1, y0, z), (x1, y1, z), (x0, y1, z))]

        ob, ot, ib, it = ring(outer, z0), ring(outer, z1), ring(inner, z0), ring(inner, z1)
        faces = []
        for i in range(4):
            j = (i + 1) % 4
            faces.append(bm.faces.new((ob[i], ob[j], ot[j], ot[i])))  # outer wall
            faces.append(bm.faces.new((it[i], it[j], ib[j], ib[i])))  # inner wall
            faces.append(bm.faces.new((ot[i], ot[j], it[j], it[i])))  # top lip
            faces.append(bm.faces.new((ib[i], ib[j], ob[j], ob[i])))  # bottom lip
        for f in faces:
            f.material_index = self.slot(mat)
        bmesh.ops.recalc_face_normals(bm, faces=faces)
        return faces

    def lathe_y(self, prof, segs, center_xz, mats):
        """Surface of revolution about an axis parallel to Y through center_xz.
        prof: list of (radius, y); r == 0 collapses to a pole.
        mats: material name per profile segment (len(prof) - 1)."""
        bm = self.bm
        cx, cz = center_xz
        rings = []
        for r, y in prof:
            if r == 0.0:
                rings.append([bm.verts.new((cx, y, cz))])
            else:
                rings.append([
                    bm.verts.new((cx + r * math.cos(2 * math.pi * k / segs), y,
                                  cz + r * math.sin(2 * math.pi * k / segs)))
                    for k in range(segs)
                ])
        faces = []
        for i in range(len(rings) - 1):
            ra, rb = rings[i], rings[i + 1]
            mi = self.slot(mats[i])
            for k in range(segs):
                kn = (k + 1) % segs
                if len(ra) == 1:
                    f = bm.faces.new((ra[0], rb[kn], rb[k]))
                elif len(rb) == 1:
                    f = bm.faces.new((ra[k], ra[kn], rb[0]))
                else:
                    f = bm.faces.new((ra[k], ra[kn], rb[kn], rb[k]))
                f.material_index = mi
                faces.append(f)
        bmesh.ops.recalc_face_normals(bm, faces=faces)
        return faces

    def finish(self, origin, parent=None, bevel=0.003, bevel_segs=2, sharp=FLAT_SHARP_ANGLE):
        """Make the object. origin is the WORLD position of the part's pivot."""
        bm = self.bm
        bmesh.ops.remove_doubles(bm, verts=bm.verts, dist=1e-6)
        bmesh.ops.translate(bm, verts=bm.verts, vec=-Vector(origin))
        for f in bm.faces:
            f.smooth = True
        # Mark sharp edges by angle (Mesh.set_sharp_from_angle no longer exists in 5.x).
        for e in bm.edges:
            if len(e.link_faces) == 2 and e.calc_face_angle(0.0) > sharp:
                e.smooth = False
        me = bpy.data.meshes.new(self.name)
        bm.to_mesh(me)
        bm.free()
        for mn in self.mats:
            me.materials.append(MAT[mn])
        ob = bpy.data.objects.new(self.name, me)
        scene.collection.objects.link(ob)
        if bevel > 0:
            bv = ob.modifiers.new("bevel", "BEVEL")
            bv.width = bevel
            bv.segments = bevel_segs
            bv.limit_method = "ANGLE"
            bv.angle_limit = sharp
            bv.harden_normals = True
            bv.miter_outer = "MITER_ARC"
            bv.use_clamp_overlap = True
        wn = ob.modifiers.new("weighted_normal", "WEIGHTED_NORMAL")
        wn.keep_sharp = True
        wn.mode = "FACE_AREA"
        wn.weight = 50
        place(ob, origin, parent)
        return ob


def world_of(ob):
    return ob.matrix_world.translation.copy() if ob else Vector((0, 0, 0))


def place(ob, world_pos, parent):
    """Parent with identity inverse; local location = world - parent world."""
    if parent is not None:
        ob.parent = parent
        ob.matrix_parent_inverse = Matrix.Identity(4)
        ob.location = Vector(world_pos) - parent_world[parent.name]
    else:
        ob.location = Vector(world_pos)
    parent_world[ob.name] = Vector(world_pos)


parent_world = {}


def empty(name, world_pos, parent, size=0.02):
    e = bpy.data.objects.new(name, None)
    e.empty_display_type = "ARROWS"
    e.empty_display_size = size
    scene.collection.objects.link(e)
    place(e, world_pos, parent)
    return e


# --------------------------------------------------------------------------
# layout (world, Blender axes: +Y forward, +Z up)
# --------------------------------------------------------------------------
P_GRIP = (0.0, 0.0, 0.0)
P_BARREL = (0.0, 0.16, 0.10)     # receiver front face, bore axis
P_MAG = (0.0, 0.105, 0.05)       # top of magwell, receiver underside
P_SIGHT_RAIL = (0.0, 0.0, 0.13)  # top of receiver
BARREL_LEN = 0.14
RW = 0.022                        # receiver half width

root = empty("smg", (0, 0, 0), None, 0.05)
sock_grip = empty("SOCKET_grip", P_GRIP, root)

# ---- receiver -------------------------------------------------------------
rec = Part("receiver", ["paint_primary", "metal_dark", "emissive_accent"])
rec.profile([
    (-0.12, 0.065), (-0.105, 0.05), (0.13, 0.05), (0.16, 0.075),
    (0.16, 0.122), (0.13, 0.13), (-0.10, 0.13), (-0.12, 0.112),
], -RW, RW, "paint_primary")
# rear end cap / buffer, dark metal
rec.profile([(-0.128, 0.07), (-0.115, 0.07), (-0.115, 0.108), (-0.128, 0.104)],
            -0.016, 0.016, "metal_dark")
# magwell sleeve (hollow so the mag visibly seats into it)
rec.tube_y((-0.017, 0.017, 0.078, 0.132), (-0.0135, 0.0135, 0.0835, 0.1265), 0.022, 0.052, "metal_dark")
# ejection port block on the right side
rec.box((RW - 0.002, 0.02, 0.09), (RW + 0.003, 0.075, 0.118), "metal_dark")
# glowing status strips, one per side
for s in (-1, 1):
    x_in, x_out = s * (RW - 0.001), s * (RW + 0.0015)
    rec.box((min(x_in, x_out), -0.09, 0.064), (max(x_in, x_out), -0.01, 0.07), "emissive_accent")
receiver = rec.finish(P_GRIP, root, bevel=0.003)

# ---- grip -----------------------------------------------------------------
g = Part("grip", ["polymer"])
dy = 0.012  # shift so the hand centre sits on the origin
g.profile([(y + dy, z) for (y, z) in [
    (0.024, 0.056), (0.006, -0.068), (-0.004, -0.082), (-0.046, -0.082),
    (-0.054, -0.072), (-0.034, 0.036), (-0.052, 0.044), (-0.058, 0.052), (-0.058, 0.068),
    (0.024, 0.068)]], -0.015, 0.015, "polymer")
grip = g.finish(P_GRIP, receiver, bevel=0.004)

# ---- trigger guard (+ trigger blade) -------------------------------------
tg = Part("trigger_guard", ["paint_primary", "metal_bare"])
tg.profile([
    (0.078, 0.054), (0.078, 0.012), (0.068, 0.0), (0.024, 0.0), (0.024, 0.007),
    (0.064, 0.007), (0.071, 0.015), (0.071, 0.054)], -0.007, 0.007, "paint_primary")
tg.profile([(0.040, 0.052), (0.047, 0.052), (0.050, 0.032), (0.046, 0.018),
            (0.041, 0.020), (0.043, 0.034)], -0.003, 0.003, "metal_bare")
trigger_guard = tg.finish(P_GRIP, receiver, bevel=0.002)

# ---- sight rail ------------------------------------------------------------
sr = Part("sight_rail", ["metal_dark"])
sr.box((-0.009, -0.085, 0.128), (0.009, 0.10, 0.136), "metal_dark")
for k in range(6):
    y0 = -0.075 + k * 0.03
    sr.box((-0.011, y0, 0.134), (0.011, y0 + 0.014, 0.141), "metal_dark")
sight_rail = sr.finish(P_SIGHT_RAIL, receiver, bevel=0.0015)
sock_sight = empty("SOCKET_sight", (0.0, 0.005, 0.141), sight_rail)

# ---- barrel shroud (mounted at SOCKET_barrel) ------------------------------
sock_barrel = empty("SOCKET_barrel", P_BARREL, receiver)
by = P_BARREL[1]
bs = Part("barrel_shroud", ["metal_dark", "metal_bare"])
bs.lathe_y([
    (0.0, by - 0.004), (0.022, by - 0.004), (0.022, by + 0.014), (0.019, by + 0.014),
    (0.019, by + 0.118), (0.015, by + 0.118), (0.015, by + BARREL_LEN),
    (0.0062, by + BARREL_LEN), (0.0062, by + BARREL_LEN - 0.02), (0.0, by + BARREL_LEN - 0.02),
], 20, (P_BARREL[0], P_BARREL[2]),
    ["metal_bare", "metal_bare", "metal_bare", "metal_dark", "metal_dark",
     "metal_bare", "metal_bare", "metal_dark", "metal_dark"])
# flat heat-sink fins on top: two chunky blades
for x0 in (-0.0075, 0.0035):
    bs.box((x0, by + 0.024, P_BARREL[2] + 0.012), (x0 + 0.004, by + 0.104, P_BARREL[2] + 0.026), "metal_dark")
barrel_shroud = bs.finish(P_BARREL, sock_barrel, bevel=0.002, sharp=ROUND_SHARP_ANGLE)
sock_muzzle = empty("SOCKET_muzzle", (P_BARREL[0], by + BARREL_LEN, P_BARREL[2]), barrel_shroud)

# ---- magazine (mounted at SOCKET_mag) ---------------------------------------
sock_mag = empty("SOCKET_mag", P_MAG, receiver)
my, mz = P_MAG[1], P_MAG[2]
mg = Part("magazine", ["metal_dark", "metal_bare", "emissive_accent"])
mg.profile([(y + my, z + mz) for (y, z) in [
    (-0.0195, 0.008), (0.0195, 0.008), (0.026, -0.12), (-0.012, -0.12)]],
    -0.0125, 0.0125, "metal_dark")
mg.profile([(y + my, z + mz) for (y, z) in [
    (-0.016, -0.118), (0.031, -0.118), (0.031, -0.132), (-0.016, -0.132)]],
    -0.0155, 0.0155, "metal_bare")
# ammo-count window on the right side
mg.box((0.012, my - 0.004, mz - 0.10), (0.0135, my + 0.008, mz - 0.03), "emissive_accent")
magazine = mg.finish(P_MAG, sock_mag, bevel=0.0025)

# ---- collider proxy ---------------------------------------------------------
col_pts = [
    (-0.13, 0.05), (-0.13, 0.13), (0.16, 0.13), (0.16, 0.05),            # receiver box
    (0.30, 0.08), (0.30, 0.122),                                          # muzzle
    (-0.046, -0.083), (0.018, -0.083),                                    # grip bottom
    (0.09, -0.083), (0.14, -0.083),                                       # mag bottom
]
cb = bmesh.new()
for (y, z) in col_pts:
    for x in (-0.023, 0.023):
        cb.verts.new((x, y, z))
bmesh.ops.convex_hull(cb, input=cb.verts)
col_me = bpy.data.meshes.new("COL_body")
cb.to_mesh(col_me)
cb.free()
col = bpy.data.objects.new("COL_body", col_me)
scene.collection.objects.link(col)
place(col, (0, 0, 0), root)
col.display_type = "WIRE"
col.hide_render = True

# --------------------------------------------------------------------------
# export
# --------------------------------------------------------------------------
bpy.ops.export_scene.gltf(
    filepath=OUT_GLB,
    export_format="GLB",
    export_yup=True,
    export_apply=True,          # bake bevel + weighted-normal modifiers
    export_normals=True,
    export_tangents=False,
    export_cameras=False,
    export_lights=False,
    export_extras=False,
    export_animation_mode="ACTIONS",
    export_animations=False,
)
print("EXPORTED", OUT_GLB)

# --------------------------------------------------------------------------
# preview renders (optional)
# --------------------------------------------------------------------------
if "--render" not in ARGS:
    sys.exit(0)

os.makedirs(PREVIEW_DIR, exist_ok=True)
for o in (sock_grip, sock_sight, sock_barrel, sock_muzzle, sock_mag, root):
    o.hide_render = True

engine = os.environ.get("SMG_ENGINE", "CYCLES")
scene.render.engine = engine
if engine == "CYCLES":
    scene.cycles.samples = 96
    scene.cycles.use_denoising = True
    scene.cycles.device = "CPU"
scene.render.resolution_x = 1280
scene.render.resolution_y = 800
scene.render.film_transparent = False
scene.view_settings.view_transform = "AgX"

world = bpy.data.worlds.new("w")
scene.world = world
bg = world.node_tree.nodes["Background"]
bg.inputs["Color"].default_value = (0.18, 0.19, 0.21, 1)
bg.inputs["Strength"].default_value = 0.35

# ground plane
bpy.ops.mesh.primitive_plane_add(size=4, location=(0, 0, -0.16))
ground = bpy.context.active_object
gm = bpy.data.materials.new("ground")
gm.node_tree.nodes["Principled BSDF"].inputs["Base Color"].default_value = (0.12, 0.12, 0.13, 1)
gm.node_tree.nodes["Principled BSDF"].inputs["Roughness"].default_value = 0.7
ground.data.materials.append(gm)


def area(name, loc, energy, size):
    ld = bpy.data.lights.new(name, "AREA")
    ld.energy = energy
    ld.size = size
    lo = bpy.data.objects.new(name, ld)
    scene.collection.objects.link(lo)
    lo.location = loc
    d = Vector((0.0, 0.08, 0.05)) - Vector(loc)
    lo.rotation_euler = d.to_track_quat("-Z", "Y").to_euler()


area("key", (0.9, -0.5, 0.9), 55, 0.8)
area("fill", (-0.9, 0.4, 0.5), 22, 1.0)
area("rim", (0.0, 1.2, 0.6), 35, 0.6)

cam_d = bpy.data.cameras.new("cam")
cam_d.lens = 60
cam = bpy.data.objects.new("cam", cam_d)
scene.collection.objects.link(cam)
scene.camera = cam
TARGET = Vector((0.0, 0.09, 0.03))


def shoot(fname, loc, target=TARGET, lens=60):
    cam_d.lens = lens
    cam.location = loc
    cam.rotation_euler = (target - Vector(loc)).to_track_quat("-Z", "Y").to_euler()
    scene.render.filepath = os.path.join(PREVIEW_DIR, fname)
    bpy.ops.render.render(write_still=True)
    print("RENDERED", fname)


shoot("side_right.png", (1.05, 0.09, 0.05))
shoot("three_quarter_front.png", (0.65, 0.75, 0.40))
shoot("three_quarter_rear.png", (-0.6, -0.7, 0.45))
# mag dropped for reload: move along the socket's local -Z (down)
magazine.location.z -= 0.07
shoot("mag_out.png", (0.85, 0.6, 0.12), target=Vector((0.0, 0.09, -0.01)))
magazine.location.z += 0.07
# close-up on bevels / shading
shoot("closeup_bevels.png", (0.28, 0.32, 0.22), target=Vector((0.0, 0.15, 0.09)), lens=70)
