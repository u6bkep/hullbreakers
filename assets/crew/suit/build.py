"""Crew hardsuit, third person. Bevy space: +Y up, -Z forward, +X right. Origin = between
the feet on the floor. Side outlines are plane "zy": points are (z, y).

A human in work gear, read against the robots by contrast: human proportions, a round
helmet with a faceted gold visor, a light hard shell over a dark soft suit, and dark
rubber bellows at the joints where the robots show axles and fork plates.

Rigid parts, each pivoted at its joint, named like the Gunner's:
  pelvis -> torso -> head
                  -> backpack
                  -> upper_arm_l -> forearm_l -> hand_l   (and _r)
  pelvis -> thigh_l -> shin_l -> foot_l                   (and _r)
Every hinge turns about the part's local X. Rest pose: standing, low-ready carry, each
fist closed round a pistol grip at SOCKET_hand_l / _r (a gun's SOCKET_grip goes there).
paint_secondary is the player's identity colour (the game recolours it): shoulder caps,
helmet crest, backpack panel. Lights: warm white visor rim and headlamp (emissive), lime
status lights and thruster rings (emissive_secondary).
"""
import math

import hb

a = hb.Asset(__file__)

SHELL, ID, SOFT, BELLOWS, DARK, BARE = ("paint_primary", "paint_secondary", "polymer", "rubber",
                                        "metal_dark", "metal_bare")
LAMP, LIGHT = "emissive", "emissive_secondary"
SIDES = (("l", -1), ("r", 1))
SMOOTH = 40
B = 0.01                                    # main-mass chamfer

LX = 0.105                                  # leg line |x|
HIP_Y, KNEE_Y, ANKLE_Y = 0.92, 0.50, 0.10
WAIST = (0, 1.04, 0)
NECK = (0, 1.50, -0.015)
HC = -0.02                                  # helmet axis z
SH = (0.205, 1.405, 0.01)                   # shoulder, elbow, wrist (right side; mirror for left)
EL = (0.235, 1.115, -0.035)
WR = (0.215, 1.10, -0.27)
GRIP = (WR[0] - 0.012, WR[1], WR[2] - 0.075)   # the grip point inside the fist


def mx(p, s):
    return (s * p[0], p[1], p[2])


def _norm(v):
    n = math.sqrt(sum(c * c for c in v))
    return tuple(c / n for c in v)


def _cross(u, v):
    return (u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0])


def ring(c, d, r, n):
    """n points round centre c, radius r, in the plane normal to d."""
    d = _norm(d)
    ref = (1, 0, 0) if abs(d[0]) < 0.9 else (0, 1, 0)
    u = _norm(_cross(d, ref))
    v = _cross(d, u)
    out = []
    for k in range(n):
        t = 2 * math.pi * (k + 0.5) / n
        out.append(tuple(c[i] + r * (math.cos(t) * u[i] + math.sin(t) * v[i]) for i in range(3)))
    return out


def tube(part, p0, p1, r0, r1, mat, n=10, bevel=None):
    """A faceted tube (convex hull of two rings) from p0 to p1, for limbs off the axes."""
    d = tuple(p1[i] - p0[i] for i in range(3))
    part.hull(ring(p0, d, r0, n) + ring(p1, d, r1, n), mat=mat, bevel=bevel)


def slab(part, p0, p1, w, h, mat, bevel=None):
    """A bar from p0 to p1 lying in xz (half width w across it, half height h in y)."""
    dx, dz = p1[0] - p0[0], p1[2] - p0[2]
    n = math.hypot(dx, dz)
    lx, lz = -dz / n * w, dx / n * w
    pts = [(p[0] + sg * lx, p[1] + sy * h, p[2] + sg * lz) for p in (p0, p1) for sg in (-1, 1) for sy in (-1, 1)]
    part.hull(pts, mat=mat, bevel=bevel)


# ---------------------------------------------------------------- pelvis
pelvis = a.part("pelvis", pivot=(0, HIP_Y, 0), bevel=B, smooth_angle=SMOOTH)
pelvis.prism([(-0.105, 1.07), (0.105, 1.07), (0.12, 0.95), (0.09, 0.835), (-0.08, 0.835), (-0.115, 0.95)],
             span=(-0.15, 0.15), mat=SOFT)                                      # soft suit trunk
pelvis.box((-0.162, 0.985, -0.125), (0.162, 1.045, 0.13), mat=DARK)            # tool belt
pelvis.box((-0.04, 0.975, -0.137), (0.04, 1.052, -0.12), mat=BARE, bevel=0)    # buckle
for _, s in SIDES:
    pelvis.prism([(-0.085, 0.99), (0.075, 0.99), (0.065, 0.875), (-0.07, 0.875)],
                 span=sorted((s * 0.135, s * 0.172)), mat=SHELL)                # hip plates over the joints
    pelvis.box((s * 0.08, 0.92, 0.11), (s * 0.15, 1.0, 0.16), mat=SOFT)       # rear belt pouches
a.collider("COL_pelvis", parent=pelvis, points=[(x, y, z) for x in (-0.172, 0.172) for y in (0.835, 1.07)
                                                for z in (-0.137, 0.16)])

# ---------------------------------------------------------------- torso
torso = a.part("torso", pelvis, pivot=WAIST, bevel=B, smooth_angle=SMOOTH)
torso.lathe([(0.118, 1.02), (0.13, 1.045), (0.116, 1.07), (0.13, 1.095), (0.118, 1.12)], axis="y",
            center=(0, 0), mats=[BELLOWS] * 4, bevel=0)                         # waist bellows
CHEST = [(-0.115, 1.10, 0.15), (0.115, 1.10, 0.15), (-0.152, 1.24, 0.175), (0.13, 1.24, 0.175),
         (-0.15, 1.36, 0.183), (0.135, 1.36, 0.183), (-0.10, 1.465, 0.15), (0.10, 1.47, 0.15)]
chest_pts = [(sx * w, y, z) for z, y, w in CHEST for sx in (-1, 1)]
torso.hull(chest_pts, mat=SHELL, bevel=0.015)                                  # hard upper torso
torso.box((-0.075, 1.205, -0.172), (0.075, 1.30, -0.13), mat=DARK)             # chest control unit
for x in (-0.045, -0.015):
    torso.box((x - 0.008, 1.27, -0.176), (x + 0.008, 1.283, -0.168), mat=LIGHT, bevel=0)  # status lights
torso.box((0.02, 1.22, -0.177), (0.06, 1.285, -0.168), mat=SOFT, bevel=0)     # dial block
torso.lathe([(0.128, 1.44), (0.128, 1.495), (0.108, 1.495)], axis="y", center=(0, NECK[2]),
            mats=[DARK, DARK], bevel=0)                                         # neck seal ring
for _, s in SIDES:                                                              # shoulder bearings
    torso.cylinder("x", (SH[1], SH[2]), 0.068, sorted((s * 0.15, s * 0.198)), mat=DARK, bevel=0)
a.collider("COL_torso", parent=torso, points=chest_pts + [(x, y, z) for x in (-0.165, 0.165)
                                                           for y in (1.02, 1.47) for z in (0.12, 0.29)])

# ---------------------------------------------------------------- backpack: EVA thrusters and grapple reel
pack = a.part("backpack", torso, pivot=(0, 1.25, 0.13), bevel=B, smooth_angle=SMOOTH)
pack.prism([(0.11, 1.03), (0.11, 1.47), (0.15, 1.485), (0.255, 1.46), (0.272, 1.40), (0.272, 1.08),
            (0.25, 1.03)], span=(-0.165, 0.165), mat=SHELL)                     # pack shell
pack.box((-0.12, 1.255, 0.262), (0.12, 1.445, 0.285), mat=ID, bevel=0.008)                  # identity panel
pack.box((-0.12, 1.47, 0.14), (0.12, 1.493, 0.25), mat=ID, bevel=0)           # top cap (seen from above/front)
for _, s in SIDES:
    pack.box((s * 0.158, 1.07, 0.16), (s * 0.172, 1.42, 0.245), mat=DARK, bevel=0)      # side rails
    pack.box((s * 0.171, 1.12, 0.19), (s * 0.177, 1.38, 0.205), mat=LIGHT, bevel=0)     # side status strips
    # down thrusters at the bottom corners, rear thrusters at the top corners: lit throats
    pack.lathe([(0.032, 1.045), (0.044, 0.975), (0.034, 0.975), (0.022, 1.0), (0, 1.0)], axis="y",
               center=(s * 0.12, 0.2), mats=[DARK, DARK, LIGHT, LIGHT], bevel=0)
    pack.lathe([(0.032, 0.255), (0.042, 0.322), (0.032, 0.322), (0.021, 0.30), (0, 0.30)], axis="z",
               center=(s * 0.115, 1.39), mats=[DARK, DARK, LIGHT, LIGHT], bevel=0)
REEL = (0, 1.135)                                                              # (x, y)
pack.cylinder("z", REEL, 0.078, (0.25, 0.30), mat=DARK, bevel=0.008)          # grapple reel housing
pack.cylinder("z", REEL, 0.045, (0.29, 0.308), mat=BARE, bevel=0)             # reel hub
pack.box((-0.012, REEL[1] + 0.06, 0.296), (0.012, REEL[1] + 0.07, 0.304), mat=LIGHT, bevel=0)  # line light
pack.box((0.06, 1.17, 0.24), (0.10, 1.215, 0.29), mat=DARK, bevel=0)          # line fairlead
pack.cylinder("y", (0.095, 0.235), 0.014, (1.21, 1.47), mat=SOFT, bevel=0)    # line conduit up to the shoulder

# ---------------------------------------------------------------- head: helmet
head = a.part("head", torso, pivot=NECK, bevel=B, smooth_angle=SMOOTH)
HELM = [(0.118, 1.495), (0.15, 1.53), (0.166, 1.60), (0.166, 1.67), (0.152, 1.74), (0.118, 1.792),
        (0.062, 1.817), (0, 1.822)]
head.lathe([(0.10, 1.47), (0.10, 1.50)], axis="y", center=(0, NECK[2]), mat=BELLOWS, bevel=0)  # neck
head.lathe(HELM, axis="y", center=(0, HC), mats=[SHELL] * (len(HELM) - 1))     # shell


def visor_pts(R, y0, y1, back=-0.06, a0=58, steps=7):
    pts = []
    for k in range(steps):
        t = math.radians(-a0 + 2 * a0 * k / (steps - 1))
        for y, rr in ((y0, R), (y1, R * 0.985)):
            pts.append((rr * math.sin(t), y, HC - rr * math.cos(t)))
    pts += [(x, y, back) for x in (-R * 0.86, R * 0.86) for y in (y0, y1)]
    return pts


head.hull(visor_pts(0.178, 1.572, 1.695), mat=DARK, bevel=0)                    # visor backing
head.hull(visor_pts(0.184, 1.574, 1.693), mat="glass", bevel=0)                 # gold visor
head.hull(visor_pts(0.19, 1.556, 1.576, a0=60), mat=LAMP, bevel=0)              # lit lower rim
head.hull(visor_pts(0.186, 1.692, 1.712, a0=62), mat=DARK, bevel=0)              # brow lip
# identity crest over the top, front to back, following the shell's side outline
front = [(0.159, 1.705), (0.152, 1.74), (0.118, 1.792), (0.062, 1.817), (0, 1.822)]
back = [(0.062, 1.817), (0.118, 1.792), (0.152, 1.74), (0.166, 1.67), (0.166, 1.60)]
crest = [(HC - r - 0.012, y) for r, y in front] + [(HC + r + 0.012, y) for r, y in back]
crest += [(HC + 0.13, 1.60), (HC - 0.13, 1.69)]
head.prism(crest, span=(-0.036, 0.036), mat=ID, smooth=True, smooth_angle=50, bevel=0.008)
for _, s in SIDES:                                                              # ear pods (comms)
    head.lathe([(0.055, s * 0.15), (0.055, s * 0.176), (0.04, s * 0.188), (0, s * 0.188)], axis="x",
               center=(1.615, -0.005), mats=[DARK, DARK, DARK], bevel=0)
head.box((-0.06, 1.505, -0.17), (0.06, 1.545, -0.13), mat=DARK, bevel=0)      # chin vent
for x in (-0.03, 0.0, 0.03):
    head.box((x - 0.008, 1.512, -0.173), (x + 0.008, 1.538, -0.165), mat=SOFT, bevel=0)
LAMP_AT = (-0.168, 1.712, -0.135)
head.box((-0.192, 1.69, -0.13), (-0.145, 1.735, -0.03), mat=DARK, bevel=0.008)  # headlamp housing
head.cylinder("z", (LAMP_AT[0], LAMP_AT[1]), 0.017, (LAMP_AT[2], -0.125), mat=LAMP, bevel=0)  # lens
a.socket("SOCKET_headlamp", head, at=LAMP_AT, forward=(0, 0, -1))
a.socket("SOCKET_eye", head, at=(0, 1.62, -0.10), forward=(0, 0, -1), up=(0, 1, 0))
a.collider("COL_head", parent=head, points=[(x, y, z) for x in (-0.19, 0.19) for y in (1.47, 1.83)
                                            for z in (-0.20, 0.15)])

# ---------------------------------------------------------------- arms
for side, s in SIDES:
    sh, el, wr, g = mx(SH, s), mx(EL, s), mx(WR, s), mx(GRIP, s)
    up = a.part(f"upper_arm_{side}", torso, pivot=sh, bevel=B, smooth_angle=SMOOTH)
    up.cylinder("x", (sh[1], sh[2]), 0.074, sorted((s * 0.196, s * 0.232)), mat=DARK, bevel=0)  # bearing
    outer, inner = s * 0.298, s * 0.198
    cap = [(-0.10, 1.32), (-0.112, 1.42), (-0.07, 1.485), (0.06, 1.485), (0.103, 1.43), (0.095, 1.32)]
    cap_pts = [(inner, y, z) for z, y in cap] + [(outer, 1.33 + (y - 1.33) * 0.8, z * 0.78) for z, y in cap]
    up.hull(cap_pts, mat=ID, bevel=0.012)                                       # shoulder cap
    tube(up, (s * 0.228, 1.35, 0.005), (el[0], el[1] + 0.03, el[2] + 0.004), 0.06, 0.053, SOFT)
    tube(up, (s * 0.231, 1.25, -0.012), (s * 0.234, 1.16, -0.026), 0.064, 0.06, SHELL, bevel=0.008)  # arm band
    fore = a.part(f"forearm_{side}", up, pivot=el, bevel=B, smooth_angle=SMOOTH)
    fore.cylinder("x", (el[1], el[2]), 0.054, (el[0] - 0.05, el[0] + 0.05), mat=BELLOWS, bevel=0)  # elbow
    tube(fore, (el[0], el[1] - 0.005, el[2] - 0.02), (wr[0], wr[1], wr[2] + 0.04), 0.05, 0.043, SOFT)
    tube(fore, (el[0], el[1] - 0.004, el[2] - 0.07), (wr[0], wr[1], wr[2] + 0.03), 0.057, 0.05, SHELL,
         bevel=0.008)                                                           # gauntlet shell
    fore.lathe([(0.052, wr[2] - 0.005), (0.052, wr[2] + 0.03)], axis="z", center=(wr[0], wr[1]),
               mat=DARK, bevel=0)                                               # suit cuff ring
    fore.box((wr[0] - 0.014, wr[1] + 0.047, wr[2] + 0.045), (wr[0] + 0.014, wr[1] + 0.056, wr[2] + 0.08),
             mat=LIGHT, bevel=0)                                                # wrist status light

    # the fist, round a vertical pistol grip raked like the SMG's (bottom further back).
    # Local frame: x toward the palm side (outboard), measured from the grip axis.
    hand = a.part(f"hand_{side}", fore, pivot=wr, bevel=0, smooth_angle=SMOOTH)

    def P(x, y, z, g=g, s=s):
        return (g[0] + s * x, g[1] + y, g[2] + z - 0.13 * y)       # raked with the grip

    core = [P(x, y, z) for y in (0.044, -0.074) for x, z in ((0.046, -0.03), (0.046, 0.05), (-0.018, 0.052),
                                                             (-0.02, 0.0), (0.0, -0.03))]
    hand.hull(core, mat=SOFT)                                                   # glove palm and back
    hand.hull([P(x, y, z) for x in (0.012 - 0.024, 0.012 + 0.024) for y in (-0.028, 0.028)
               for z in (0.03, 0.105)], mat=SOFT)                               # glove wrist
    hand.hull([P(x, y, z) for x in (0.046, 0.055) for y in (-0.03, 0.04) for z in (-0.028, 0.035)],
              mat=SHELL, bevel=0)                                               # knuckle guard
    for yc, h, k in ((-0.012, 0.0095, 1.0), (-0.034, 0.0095, 0.97), (-0.056, 0.0085, 0.93)):  # three curled fingers
        hand.hull([P(x, yc + sy * h, z * k) for sy in (-1, 1) for x, z in
                   ((0.046, -0.022), (0.046, -0.045), (-0.006, -0.054), (-0.006, -0.03))], mat=SOFT, bevel=0)
        hand.hull([P(x * k, yc + sy * h, z * k) for sy in (-1, 1) for x, z in
                   ((-0.002, -0.054), (-0.026, -0.044), (-0.03, -0.01), (-0.014, -0.01), (-0.002, -0.03))],
                  mat=SOFT, bevel=0)
    slab(hand, P(0.036, 0.032, -0.03), P(0.017, 0.032, -0.066), 0.009, 0.01, SOFT, bevel=0)  # index, off the trigger
    slab(hand, P(0.02, 0.034, 0.045), P(-0.026, 0.036, 0.016), 0.011, 0.011, SOFT, bevel=0)  # thumb
    slab(hand, P(-0.028, 0.036, 0.018), P(-0.026, 0.036, -0.03), 0.009, 0.01, SOFT, bevel=0)
    a.socket(f"SOCKET_hand_{side}", hand, at=g, forward=(0, 0, -1), up=(0, 1, 0))

# ---------------------------------------------------------------- legs and mag boots
for side, s in SIDES:
    x = s * LX
    thigh = a.part(f"thigh_{side}", pelvis, pivot=(x, HIP_Y, 0), bevel=B, smooth_angle=SMOOTH)
    thigh.lathe([(0.084, 0.94), (0.088, 0.86), (0.074, 0.585)], axis="y", center=(x, 0.0),
                mats=[SOFT, SOFT], bevel=0)                                     # soft suit
    thigh.lathe([(0.094, 0.84), (0.095, 0.72), (0.084, 0.63)], axis="y", center=(x, -0.004),
                mats=[SHELL, SHELL], bevel=0.008)                               # thigh shell
    thigh.box((s * 0.17, 0.66, -0.045), (s * 0.20, 0.79, 0.05), mat=SOFT)     # thigh pocket
    thigh.box((s * 0.197, 0.75, -0.02), (s * 0.204, 0.77, 0.025), mat=LIGHT, bevel=0)   # pocket tab light

    shin = a.part(f"shin_{side}", thigh, pivot=(x, KNEE_Y, 0), bevel=B, smooth_angle=SMOOTH)
    shin.lathe([(0.07, 0.6), (0.078, 0.575), (0.068, 0.55), (0.078, 0.525), (0.068, 0.50), (0.078, 0.475),
                (0.066, 0.45)], axis="y", center=(x, 0.0), mats=[BELLOWS] * 6, bevel=0)  # knee bellows
    shin.prism([(-0.05, 0.585), (-0.09, 0.585), (-0.106, 0.515), (-0.095, 0.44), (-0.05, 0.44)],
               span=(x - 0.066, x + 0.066), mat=SHELL)                         # knee guard
    shin.lathe([(0.074, 0.45), (0.072, 0.32), (0.06, 0.205)], axis="y", center=(x, 0.005),
               mats=[SHELL, SHELL], bevel=0.008)                                # shin shell
    shin.lathe([(0.06, 0.22), (0.06, 0.17)], axis="y", center=(x, 0.0), mat=SOFT, bevel=0)  # ankle

    foot = a.part(f"foot_{side}", shin, pivot=(x, ANKLE_Y, 0), bevel=B, smooth_angle=SMOOTH)
    foot.lathe([(0.072, 0.115), (0.072, 0.215)], axis="y", center=(x, 0.0), mat=SOFT)   # boot cuff
    foot.prism([(0.10, 0.04), (0.105, 0.14), (0.05, 0.17), (-0.045, 0.15), (-0.13, 0.105), (-0.175, 0.085),
                (-0.18, 0.04)], span=(x - 0.068, x + 0.068), mat=DARK)          # boot
    foot.prism([(-0.10, 0.045), (-0.10, 0.122), (-0.14, 0.102), (-0.178, 0.088), (-0.186, 0.045)],
               span=(x - 0.072, x + 0.072), mat=SHELL)                          # toe cap
    foot.box((x - 0.074, 0.0, -0.19), (x + 0.074, 0.03, 0.112), mat=BARE, bevel=0.008)   # mag plate
    foot.box((x - 0.078, 0.03, -0.192), (x + 0.078, 0.045, 0.114), mat=LIGHT, bevel=0)    # mag glow seam
    foot.box((x - 0.05, 0.06, 0.09), (x + 0.05, 0.13, 0.13), mat=SOFT, bevel=0.008)       # heel coil housing
    a.socket(f"SOCKET_foot_{side}", foot, at=(x, 0, -0.04), forward=(0, -1, 0), up=(0, 0, -1))

    a.collider(f"COL_thigh_{side}", parent=thigh,
               points=[(x + sx, y, z) for sx in (-0.095, 0.095) for y in (0.6, 0.94) for z in (-0.1, 0.09)])
    a.collider(f"COL_shin_{side}", parent=shin,
               points=[(x + sx, y, z) for sx in (-0.08, 0.08) for y in (0.0, 0.6) for z in (-0.19, 0.12)])
a.export()
