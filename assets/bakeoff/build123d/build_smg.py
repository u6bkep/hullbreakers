"""Stage 1: model the SMG frame in build123d and export a raw glTF with build123d's
own exporter (export_gltf -> OCCT RWGltf_CafWriter).

Modelling frame (CAD, millimetres): +Z up, +Y forward (muzzle), +X right.
Origin = SOCKET_grip (centre of the hand on the grip).

Each part is modelled in gun space, then shifted so its own origin is its attach
point (socket on the receiver), then rotated into the glTF frame (+Y up, -Z
forward). The root gets a +90 deg X rotation that cancels the -90 deg build123d's
exporter applies, so the exported root is identity and the data is already Y-up.

Writes: <out>/raw.glb and <out>/layout.json (socket positions + part attach points,
in glTF metres) for the post-processor.
"""
import json
import sys
from pathlib import Path

import numpy as np
from build123d import (Axis, Box, Color, Compound, Cylinder, Location, Polyline,
                       Pos, Unit, chamfer, export_gltf, extrude, fillet,
                       make_face)
from scipy.spatial import ConvexHull

OUT = Path(sys.argv[1] if len(sys.argv) > 1 else ".")
OUT.mkdir(parents=True, exist_ok=True)

# Exporter colours double as material keys: postprocess.py maps each exact
# baseColor back to a named PBR material, because OCCT names them mat_0..mat_N
# and drops metallic/roughness/emissive.
KEY = {
    "paint_primary":   (0.70, 0.42, 0.16),
    "metal_dark":      (0.16, 0.17, 0.19),
    "metal_bare":      (0.62, 0.63, 0.66),
    "polymer":         (0.05, 0.05, 0.055),
    "emissive_accent": (0.20, 0.85, 1.00),
}

def mat(shape, name, label):
    shape.color = Color(*KEY[name])
    shape.label = label
    return shape

def side_profile(pts_yz, width):
    """Closed polygon in the YZ plane (y forward, z up) extruded symmetrically in X."""
    face = make_face(Polyline(*[(0, y, z) for y, z in pts_yz], close=True))
    return extrude(face, amount=width / 2, both=True)

def cad_to_gltf(v):
    x, y, z = v
    return (x / 1000.0, z / 1000.0, -y / 1000.0)

# ---------------------------------------------------------------- key dimensions
BORE_Z = 85.0                      # barrel axis height
REC_Y0, REC_Y1 = -88.0, 165.0      # receiver rear / front
REC_Z0, REC_Z1 = 55.0, 110.0       # receiver bottom / top
REC_W = 34.0
MAG_Y = 86.0                       # magazine centre (y)
RAIL_Y0, RAIL_Y1 = -55.0, 140.0
RAIL_TOP = 121.0
MUZZLE_Y = 322.0

SOCKETS = {                         # CAD mm, gun space
    "SOCKET_grip":   (0.0, 0.0, 0.0),
    "SOCKET_mag":    (0.0, MAG_Y, REC_Z0),
    "SOCKET_barrel": (0.0, REC_Y1, BORE_Z),
    "SOCKET_muzzle": (0.0, MUZZLE_Y, BORE_Z),
    "SOCKET_sight":  (0.0, (RAIL_Y0 + RAIL_Y1) / 2, RAIL_TOP),
}
# Where each part's local origin sits (its attach point).
ATTACH = {
    "receiver":      (0.0, 0.0, REC_Z0),
    "grip":          (0.0, 0.0, 0.0),
    "magazine":      SOCKETS["SOCKET_mag"],
    "barrel_shroud": SOCKETS["SOCKET_barrel"],
    "sight_rail":    (0.0, (RAIL_Y0 + RAIL_Y1) / 2, REC_Z1),
    "trigger_guard": (0.0, 40.0, REC_Z0),
    "COL_body":      (0.0, 0.0, 0.0),
}

# ---------------------------------------------------------------- receiver
rec = side_profile([
    (REC_Y0, 60), (REC_Y0, 98), (REC_Y0 + 14, REC_Z1), (REC_Y1 - 12, REC_Z1),
    (REC_Y1, 100), (REC_Y1, 64), (REC_Y1 - 14, REC_Z0), (REC_Y0 + 10, REC_Z0),
], REC_W)
rec = chamfer(rec.edges(), 3.0)
# magwell collar below the receiver
magwell = Pos(0, MAG_Y, 47) * Box(30, 40, 18)
magwell = chamfer(magwell.edges().group_by(Axis.Z)[0], 3.0)
rec = rec + magwell
# side glow grooves + rear cap groove
groove_len, groove_y = 110.0, 30.0
for sx in (-1, 1):
    rec = rec - Pos(sx * REC_W / 2, groove_y, 95) * Box(3.0, groove_len, 5.0)
rec = mat(rec, "paint_primary", "receiver_body")
glow = Compound([Pos(sx * (REC_W / 2 - 1.0), groove_y, 95) * Box(1.6, groove_len - 1, 4.0)
                 for sx in (-1, 1)])
glow = mat(glow, "emissive_accent", "receiver_glow")
receiver = [rec, glow]

# ---------------------------------------------------------------- grip
rake = np.tan(np.radians(16))
g_bot, g_top, g_depth = -52.0, REC_Z0 + 4, 34.0
def grip_y(z):  # rear edge y at height z, centred so the hand centre is y=0 at z=0
    return -g_depth / 2 + rake * z
grip = side_profile([
    (grip_y(g_bot) - 2, g_bot), (grip_y(g_bot) + g_depth + 4, g_bot),
    (grip_y(g_top) + g_depth, g_top), (grip_y(g_top), g_top),
], 28.0)
grip = fillet(grip.edges().filter_by(Axis.X, reverse=True).filter_by(Axis.Y, reverse=True), 7.0)
grip = chamfer(grip.edges().group_by(Axis.Z)[0], 2.5)
grip = mat(grip, "polymer", "grip")

# ---------------------------------------------------------------- trigger guard + trigger
tg_front, tg_bot = 66.0, 24.0
tg_rear = grip_y(30) + g_depth - 3
guard = side_profile([
    (tg_rear, 30), (tg_rear + 6, tg_bot), (tg_front, tg_bot), (tg_front + 6, tg_bot + 8),
    (tg_front + 6, REC_Z0 + 1), (tg_front - 2, REC_Z0 + 1), (tg_front - 1, tg_bot + 8),
    (tg_front - 4, tg_bot + 6), (tg_rear + 9, tg_bot + 6), (tg_rear + 3, 36),
], 9.0)
guard = chamfer(guard.edges().filter_by(Axis.X, reverse=True), 1.5)
guard = mat(guard, "metal_dark", "trigger_guard_body")
trig = side_profile([(36, REC_Z0 + 1), (44, REC_Z0 + 1), (40, 42), (41, 34), (37, 34), (35, 42)], 6.0)
trig = mat(trig, "metal_bare", "trigger")
trigger_guard = [guard, trig]

# ---------------------------------------------------------------- magazine
mag_body = Pos(0, MAG_Y, -11.75) * Box(22, 28, 133.5)    # z -78.5 .. 55 (top hidden in well)
mag_body = chamfer(mag_body.edges().filter_by(Axis.Z), 3.0)
mag_body = mat(mag_body, "metal_dark", "magazine_body")
mag_plate = Pos(0, MAG_Y + 1.5, -82) * Box(28, 36, 8)   # top z=-78; flat top must clear the body footprint
mag_plate = chamfer(mag_plate.edges(), 2.0)
mag_plate = mat(mag_plate, "metal_bare", "magazine_plate")
magazine = [mag_body, mag_plate]

# ---------------------------------------------------------------- sight rail
rail_len = RAIL_Y1 - RAIL_Y0
rail = Pos(0, (RAIL_Y0 + RAIL_Y1) / 2, REC_Z1 + 3) * Box(18, rail_len, 6)
rail = chamfer(rail.edges().group_by(Axis.Z)[-1], 1.5)
n_ribs = 8
pitch = (rail_len - 12) / (n_ribs - 1)
ribs = None
for i in range(n_ribs):
    r = Pos(0, RAIL_Y0 + 6 + i * pitch, RAIL_TOP - 2.5) * Box(22, 7, 5)
    r = chamfer(r.edges().group_by(Axis.Z)[-1], 1.5)
    ribs = r if ribs is None else ribs + r
rail = mat(rail + ribs, "metal_dark", "sight_rail")

# ---------------------------------------------------------------- barrel shroud + barrel
def along_y(radius, y0, y1):
    return Pos(0, (y0 + y1) / 2, BORE_Z) * Cylinder(radius, y1 - y0, rotation=(90, 0, 0))
SH_Y1 = 288.0
shroud = along_y(16.0, REC_Y1 - 2, SH_Y1)
shroud = chamfer(shroud.edges().sort_by(Axis.Y)[-1], 3.0)
for i in range(3):   # chunky vent slots, both sides
    y = 195 + i * 26
    shroud = shroud - Pos(0, y, BORE_Z + 2) * Box(40, 16, 10)
collar = along_y(18.5, SH_Y1 - 24, SH_Y1 - 10)
collar = chamfer(collar.edges(), 2.0)
shroud = mat(shroud + collar, "metal_dark", "shroud")
barrel = along_y(7.0, SH_Y1 - 2, MUZZLE_Y - 14)
brake = along_y(10.5, MUZZLE_Y - 18, MUZZLE_Y)
brake = chamfer(brake.edges(), 2.0)
barrel = barrel + brake - along_y(4.5, MUZZLE_Y - 20, MUZZLE_Y + 1)
barrel = mat(barrel, "metal_bare", "barrel")
barrel_shroud = [shroud, barrel]

# ---------------------------------------------------------------- collider (convex prism)
# build123d has no 3D convex hull op; hull the side profile in 2D (scipy) and extrude.
hull_pts = np.array([
    (REC_Y0, 60), (REC_Y0, 98), (REC_Y0 + 14, RAIL_TOP), (RAIL_Y1, RAIL_TOP),
    (REC_Y1, 104), (SH_Y1, BORE_Z + 18.5), (MUZZLE_Y, BORE_Z + 10.5),
    (MUZZLE_Y, BORE_Z - 10.5), (SH_Y1, BORE_Z - 18.5), (REC_Y1, REC_Z0),
    (MAG_Y + 20, 38), (MAG_Y - 20, 38), (REC_Y0 + 10, REC_Z0),
])  # main body only (receiver + rail + shroud + magwell); grip and mag hang outside it
hull = hull_pts[ConvexHull(hull_pts).vertices]
col = side_profile([tuple(p) for p in hull], REC_W)
col.label = "COL_body_geo"   # no colour -> no material

# ---------------------------------------------------------------- assemble
def part_node(name, shapes):
    ax, ay, az = ATTACH[name]
    leaves = []
    for s in shapes:
        s = s.moved(Location((-ax, -ay, -az)))          # local origin = attach point
        s = s.rotate(Axis.X, -90)                        # CAD Z-up -> glTF Y-up
        leaves.append(s)
    node = Compound(children=leaves, label=name)
    node.location = Location(tuple(c * 1000 for c in cad_to_gltf(ATTACH[name])))
    return node

parts = {
    "receiver": receiver, "grip": [grip], "barrel_shroud": barrel_shroud,
    "magazine": magazine, "sight_rail": [rail], "trigger_guard": trigger_guard,
    "COL_body": [col],
}
nodes = [part_node(n, s) for n, s in parts.items()]
root = Compound(children=nodes, label="smg")
root.location = Location((0, 0, 0), (1, 0, 0), 90)   # cancels exporter's -90 deg X

ok = export_gltf(root, str(OUT / "raw.glb"), unit=Unit.MM, binary=True,
                 linear_deflection=0.25, angular_deflection=0.3)
assert ok, "export_gltf failed"

layout = {
    "sockets": {k: cad_to_gltf(v) for k, v in SOCKETS.items()},
    "attach": {k: cad_to_gltf(v) for k, v in ATTACH.items()},
    "material_keys": KEY,
}
(OUT / "layout.json").write_text(json.dumps(layout, indent=2))
print("wrote", OUT / "raw.glb")
