"""Grazer: a passive space jellyfish/manta hybrid about 3.2 m across. Bevy space: +Y is the
bell's apex, the way it swims; -Z forward, +X right. Origin at the bell's centre, the centre
of the game's physics body (an ellipsoid of radii 1.6, 0.7, 1.6, src/sim/fauna.rs).

Read at range: a broad pale dome (smooth skin, its top gently curved so a crew member can
stand on it) with deep-teal canals radiating from the apex, a ring of cyan lights round its
margin, eight soft flaps hanging from the rim, and six long translucent teal fronds trailing
below with violet glowing tips. Under the bell, in its cavity, a magenta heart glows: seen
from below and from the side between the flaps, never from above (the bell is opaque).
Lights, loud to quiet: the heart (emissive_core, SOCKET_weak at its centre), the rim spots
(emissive), the frond tips (emissive_secondary).

Moving parts (the game drives them; each pivoted at its joint):
  rim_0..rim_7   flaps round the bell's edge, k*45 deg clockwise from the front seen from above
                 (k=2 is +X). Local -Z runs down the flap, local X along the rim (the tangent),
                 local +Y inward: a positive rotation about local X curls the flap in.
  frond_<i>_a/b/c  six fronds round a ring of radius 0.5 under the bell (i*60 + 30 deg), three
                 segments each, _a on the bell, _b on _a, _c on _b. Local -Z runs down the
                 segment, local +Y outward from the bell's axis.
  heart          hangs from the cavity's roof, pivoted at its top, so it can pulse.
"""
import math

from mathutils import Vector

import hb

a = hb.Asset(__file__)
SKIN, TEAL, FROND, SPOT, TIP, CORE = "skin", "flesh", "membrane", "emissive", "emissive_secondary", "emissive_core"


def around(k):
    """Unit direction in plan at k*45 degrees, clockwise from the front seen from above."""
    t = math.radians(45 * k)
    return Vector((math.sin(t), 0.0, -math.cos(t)))


# ---------------------------------------------------------------- the bell
# A lathed dome: the outer surface from the apex down past its widest to the rim, round the
# lip, and back up the subumbrella (the cavity's roof) to the middle. Under the brief's
# numbers the margin flares a little outside the physics ellipsoid below y = -0.15.
OUTER = [(0.15, 0.677), (0.4, 0.66), (0.55, 0.64), (0.75, 0.60), (1.0, 0.505), (1.15, 0.42), (1.32, 0.29),
         (1.45, 0.13), (1.53, -0.05), (1.555, -0.2), (1.52, -0.36)]
LIP = [(1.47, -0.415)]
INNER = [(1.36, -0.36), (1.15, -0.27), (0.85, -0.185), (0.5, -0.13), (0.22, -0.105)]
APEX, ROOF = 0.68, -0.1
N = 48                    # round the bell; a sector is centred on every 7.5 degrees


def plan_angle(c):
    """Degrees clockwise from the front, seen from above, of a point."""
    return math.degrees(math.atan2(c[0], -c[2])) % 360.0


def bell_role(c):
    r = math.hypot(c[0], c[2])
    if r < 1.44 and c[1] < 0.0:
        return TEAL                                          # the subumbrella, the cavity's roof
    ang = plan_angle(c)
    off = min(ang % 45.0, 45.0 - ang % 45.0)                 # degrees from the nearest canal
    if off < 4.0 and 0.12 < r < 1.33:
        return TEAL                                          # eight radial canals, one per flap
    if abs(off - 22.5) < 4.0 and 0.55 < r < 1.16:
        return TEAL                                          # and eight short ones between them
    return SKIN


def ring(r, y):
    return [(r * math.sin(2 * math.pi * (k + 0.5) / N), y, -r * math.cos(2 * math.pi * (k + 0.5) / N))
            for k in range(N)]


bell = a.part("bell")
bell.loft([[(0.0, APEX, 0.0)]] + [ring(r, y) for r, y in OUTER + LIP + INNER] + [[(0.0, ROOF, 0.0)]],
          mat=bell_role, crease=60)
# The rim lights: sixteen cyan spots set into the margin, between the canals.
for k in range(16):
    t = math.radians(22.5 * k + 11.25)
    d = Vector((math.sin(t), 0.0, -math.cos(t)))
    c = d * 1.552 + Vector((0.0, -0.12, 0.0))
    bell.blob(tuple(c), (0.05, 0.05, 0.024), mat=SPOT, forward=tuple(-d), up=(0, 1, 0), segments=8)

a.socket("SOCKET_saddle", bell, at=(0.0, APEX, 0.0), up=(0, 1, 0))
a.socket("SOCKET_weak", bell, at=(0.0, -0.3, 0.0))
a.socket("SOCKET_mouth", bell, at=(0.0, -0.55, 0.0), forward=(0, -1, 0), up=(0, 0, -1))

# ---------------------------------------------------------------- the heart
# A lumpy glowing sac hanging from the cavity's roof, its lower lobe below the lappets so it
# shows from the side.
heart = a.part("heart", bell, pivot=(0.0, ROOF, 0.0))
HN = 16


def sac(y, r, lump):
    return [(r * (1 + lump * math.sin(3 * t + 7 * y)) * math.sin(t), y, -r * (1 + lump * math.sin(3 * t + 7 * y)) * math.cos(t))
            for t in (2 * math.pi * k / HN for k in range(HN))]


heart.loft([[(0.0, ROOF + 0.02, 0.0)], sac(-0.10, 0.07, 0.0), sac(-0.17, 0.15, 0.05), sac(-0.26, 0.2, 0.07),
            sac(-0.35, 0.2, 0.06), sac(-0.44, 0.15, 0.05), sac(-0.5, 0.12, 0.04), sac(-0.57, 0.13, 0.04),
            sac(-0.64, 0.09, 0.02), [(0.0, -0.675, 0.0)]], mat=CORE)

# ---------------------------------------------------------------- rim flaps
# Lappets: thin curved scallops that carry the bell's margin on down, each a band of the rim
# (most of its 45 degrees, gaps between them) with a rounded tip that turns a little inward.
# Pale like the bell, its canal's teal at the tip.
LAP = [(-0.33, 1.495, 16.0), (-0.39, 1.525, 16.5), (-0.44, 1.525, 15.0), (-0.48, 1.505, 11.5), (-0.505, 1.475, 6.5)]
LAP_THICK = 0.026
for k in range(8):
    d = around(k)
    root = d * 1.49 + Vector((0.0, -0.36, 0.0))
    down = (d * 0.25 + Vector((0.0, -1.0, 0.0))).normalized()
    flap = a.part(f"rim_{k}", bell, pivot=tuple(root), forward=tuple(down), up=tuple(-d))
    rings = []
    for y, r, half in LAP:
        pts = []
        for side in (1, -1):                                  # out along the outer face, back along the inner
            rr = r + side * LAP_THICK / 2
            arc = [half * (2 * q / 6 - 1) for q in range(7)]
            for deg in (arc if side > 0 else arc[::-1]):
                t = math.radians(45 * k + deg)
                pts.append((rr * math.sin(t), y, -rr * math.cos(t)))
        rings.append(pts)
    tip_y = LAP[-1][0] - 0.015
    rings.append([(1.45 * d.x, tip_y, 1.45 * d.z)])
    flap.loft(rings, mat=lambda c: TEAL if c[1] < -0.465 else SKIN)

# ---------------------------------------------------------------- fronds
# Each hangs from the cavity's roof and drifts a little outward in a slow S; three segments of
# one smooth curve, tapering ribbons (wider than thick) with a soft knuckle at each joint, the
# last ending in a glowing violet bead.


def catmull(p0, p1, p2, p3, t):
    t2, t3 = t * t, t * t * t
    return 0.5 * ((2 * p1) + (-p0 + p2) * t + (2 * p0 - 5 * p1 + 4 * p2 - p3) * t2 + (-p0 + 3 * p1 - 3 * p2 + p3) * t3)


JOINTS_Y = (-0.12, -0.68, -1.26, -1.93)
for i in range(6):
    t = math.radians(60 * i + 30)
    d = Vector((math.sin(t), 0.0, -math.cos(t)))
    side = Vector((0.0, 1.0, 0.0)).cross(d)
    flipx = 1 if i % 2 else -1
    sway = (0.0, 0.09, -0.08, 0.1)
    reach = (0.5, 0.64, 0.74, 0.66)
    pts = [d * reach[j] + side * sway[j] * flipx + Vector((0.0, JOINTS_Y[j], 0.0)) for j in range(4)]
    ext = [pts[0] * 2 - pts[1]] + pts + [pts[3] * 2 - pts[2]]
    widths = ((0.13, 0.034), (0.1, 0.028), (0.065, 0.02), (0.02, 0.012))
    parent = bell
    for j, tag in enumerate("abc"):
        p, q = pts[j], pts[j + 1]
        axis = (q - p).normalized()
        seg = a.part(f"frond_{i}_{tag}", parent, pivot=tuple(p), forward=tuple(axis), up=tuple(d))
        curve = [catmull(ext[j], ext[j + 1], ext[j + 2], ext[j + 3], u / 4) for u in range(5)]
        w0, w1 = widths[j], widths[j + 1]
        rad = [(w0[0] + (w1[0] - w0[0]) * u / 4, w0[1] + (w1[1] - w0[1]) * u / 4) for u in range(5)]
        t0 = (curve[1] - curve[0]).normalized()
        t1 = (curve[4] - curve[3]).normalized()
        path = [curve[0] - t0 * w0[0] * 0.7] + curve + [curve[4] + t1 * (w1[0] * 0.7 if j < 2 else 0.015)]
        rads = [0.0] + rad + [0.0]
        if j < 2:
            seg.sweep([tuple(v) for v in path], rads, mat=FROND, up=tuple(d))
        else:
            tip_y = q.y + 0.4
            seg.sweep([tuple(v) for v in path], rads, mat=lambda c, ty=tip_y: TIP if c[1] < ty else FROND, up=tuple(d))
            seg.blob(tuple(q + t1 * 0.025), (0.035, 0.045, 0.035), mat=TIP)
        parent = seg

# ---------------------------------------------------------------- collider
# One hull over the dome: the ellipsoid's upper half and a ring at the rim.
col = []
for k in range(12):
    t = 2 * math.pi * k / 12
    for lat in (0.0, 0.35, 0.7):
        rr = 1.6 * math.cos(lat)
        col.append((rr * math.sin(t), 0.7 * math.sin(lat), -rr * math.cos(t)))
    col.append((1.5 * math.sin(t), -0.4, -1.5 * math.cos(t)))
col.append((0.0, 0.7, 0.0))
a.collider("COL_bell", points=col)
a.export()
