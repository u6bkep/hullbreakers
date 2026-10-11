"""Grapple launcher: one-handed crew tool. Bevy space: +Y up, -Z forward, +X right.
Origin = the hand on the grip (the SMG's grip, so the same fist holds it).
Side outlines are plane "zy": points are (z, y).

The cable story: it comes up the hose from the suit's winch, wraps the reel drum on top
(the game spins `reel` about its local X while reeling), runs forward through the housing
and out of the launch tube to the harpoon's tail (SOCKET_cable)."""
import math

import hb

a = hb.Asset(__file__)
BW = 0.026                                   # housing half width
AXIS_Y = 0.10                                # launch axis height
REEL_C = (0.032, 0.126)                      # reel axle (z, y); the axle runs along X
REEL_R = 0.036                               # reel flange radius

# ---- housing
body = a.part("body", bevel=0.003)
body.prism([(0.078, 0.058), (0.078, 0.086), (-0.006, 0.086), (-0.016, 0.130), (-0.132, 0.130), (-0.168, 0.124),
            (-0.178, 0.112), (-0.178, 0.072), (-0.166, 0.050), (0.060, 0.050)],
           span=(-BW, BW), mat="paint_primary")
for s in (-1, 1):                            # reel cheeks, holding the axle
    body.prism([(0.078, 0.062), (0.078, 0.092), (0.050, 0.140), (0.014, 0.140), (-0.012, 0.092),
                (-0.012, 0.062)], span=(s * BW, s * (BW + 0.007)), mat="paint_secondary")
    body.box((s * (BW - 0.001), 0.098, -0.150), (s * (BW + 0.0016), 0.105, -0.020),
             mat="emissive_secondary", bevel=0)                   # cable-tension strip
    body.box((s * (BW - 0.001), 0.094, -0.154), (s * (BW + 0.001), 0.109, -0.016),
             mat="metal_dark", bevel=0)                           # its dark backing
body.box((-0.020, 0.046, -0.072), (0.020, 0.056, 0.058), mat="metal_dark")   # belly plate
body.box((-0.012, 0.128, -0.118), (0.012, 0.1335, -0.080), mat="emissive", bevel=0)  # ready lamp
body.box((-0.016, 0.126, -0.124), (0.016, 0.1305, -0.074), mat="metal_dark", bevel=0)  # lamp bezel
n_housing = len(body.prims)

# launch tube, collar and mouth
body.lathe([(0, -0.170), (0.024, -0.170), (0.024, -0.214), (0.028, -0.218), (0.028, -0.235),
            (0.016, -0.235), (0.016, -0.226), (0, -0.226)], axis="z", center=(0, AXIS_Y),
           mats=["metal_dark", "metal_dark", "metal_bare", "metal_bare", "metal_bare", "metal_dark",
                 "metal_dark"])
# gas charge bottle under the nose, on a saddle
body.box((-0.010, 0.040, -0.160), (0.010, 0.052, -0.098), mat="metal_dark")
body.lathe([(0, -0.094), (0.012, -0.094), (0.016, -0.100), (0.016, -0.168), (0.011, -0.176),
            (0.0, -0.176)], axis="z", center=(0, 0.032),
           mats=["metal_bare", "metal_bare", "paint_secondary", "metal_bare", "metal_bare"])
n_nose = len(body.prims)

# ---- grip and guard: the SMG's, so the glove's fist fits
grip = a.part("grip", body, bevel=0.004)
grip.prism([(-0.036, 0.056), (-0.018, -0.068), (-0.008, -0.082), (0.034, -0.082), (0.042, -0.072),
            (0.022, 0.036), (0.040, 0.044), (0.046, 0.052), (0.046, 0.068), (-0.036, 0.068)],
           span=(-0.015, 0.015), mat="polymer")

guard = a.part("trigger_guard", body, bevel=0.002)
guard.prism([(-0.078, 0.054), (-0.078, 0.012), (-0.068, 0.0), (-0.024, 0.0), (-0.024, 0.007),
             (-0.064, 0.007), (-0.071, 0.015), (-0.071, 0.054)], span=(-0.007, 0.007), mat="paint_secondary")
guard.prism([(-0.040, 0.052), (-0.047, 0.052), (-0.050, 0.032), (-0.046, 0.018), (-0.041, 0.020),
             (-0.043, 0.034)], span=(-0.003, 0.003), mat="metal_bare")  # trigger blade

# ---- reel: pivoted on its axle; local X (= world X here) is the spin axis
reel = a.part("reel", body, pivot=(0, REEL_C[1], REEL_C[0]), bevel=0.002)
FX = 0.023                                   # flange outer face
# A deliberate octagon: its corners are what show the spin.
reel.lathe([(0, -FX), (REEL_R, -FX), (REEL_R, -FX + 0.007), (0.026, -FX + 0.007), (0.026, FX - 0.007),
            (REEL_R, FX - 0.007), (REEL_R, FX), (0, FX)], axis="x", center=(REEL_C[1], REEL_C[0]),
           mats=["metal_bare", "metal_bare", "metal_bare", "rubber", "metal_bare", "metal_bare", "metal_bare"],
           segments=8)
for s in (-1, 1):                            # axle hubs through the cheeks, keyed so they show the spin
    reel.cylinder("x", (REEL_C[1], REEL_C[0]), 0.012, (s * (BW + 0.006), s * (BW + 0.011)), mat="metal_dark")
    reel.box((s * (BW + 0.0105), REEL_C[1] - 0.0025, REEL_C[0] - 0.010),
             (s * (BW + 0.0125), REEL_C[1] + 0.0025, REEL_C[0] + 0.010), mat="metal_bare", bevel=0)

# ---- hose to the suit, out the back
HOSE = (0, 0.072, 0.078)
hose = a.part("hose", body, pivot=HOSE, bevel=0.002)
hose.lathe([(0, 0.078), (0.015, 0.078), (0.015, 0.090), (0.0095, 0.090), (0.0095, 0.116), (0.0145, 0.116),
            (0.0145, 0.126), (0, 0.126)], axis="z", center=(0, 0.072),
           mats=["metal_bare", "metal_bare", "metal_bare", "rubber", "metal_dark", "metal_dark", "metal_dark"])
a.socket("SOCKET_hose", hose, at=(0, 0.072, 0.126), forward=(0, 0, 1))

# ---- harpoon, sitting in the tube mouth
MUZZLE = (0, AXIS_Y, -0.235)
a.socket("SOCKET_muzzle", body, at=MUZZLE)
harp = a.part("harpoon", "SOCKET_muzzle", pivot=MUZZLE, bevel=0.002)
harp.lathe([(0, -0.200), (0.010, -0.200), (0.010, -0.244), (0.015, -0.249), (0.015, -0.262),
            (0.0025, -0.310), (0.0, -0.310)], axis="z", center=(0, AXIS_Y),
           mats=["metal_dark", "metal_dark", "metal_bare", "metal_bare", "metal_bare", "metal_bare"])
harp.cylinder("z", (0, AXIS_Y), 0.0115, (-0.236, -0.243), mat="emissive", bevel=0)   # locator band
for ang in (0, 90, 180, 270):                # barbed flukes, swept back
    c, sn = math.cos(math.radians(ang)), math.sin(math.radians(ang))
    tx, ty = c, -sn                          # tangent
    pts = []
    for (r, z) in ((0.009, -0.300), (0.013, -0.258), (0.031, -0.246), (0.028, -0.258)):
        for h in (-0.0022, 0.0022):
            pts.append((r * sn + h * tx, AXIS_Y + r * c + h * ty, z))
    harp.hull(pts, mat="metal_bare", bevel=0)
a.socket("SOCKET_cable", harp, at=(0, AXIS_Y, -0.200))

a.socket("SOCKET_grip", at=(0, 0, 0))
pts = [p for pr in body.prims[:n_housing] for p in pr.verts]
a.collider("COL_body", points=pts + hose.points(), around=[reel])
a.collider("COL_nose", points=[p for pr in body.prims[n_housing:n_nose] for p in pr.verts], around=[harp])
a.collider("COL_grip", around=[grip, guard])
a.export()
