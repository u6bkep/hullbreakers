"""Grappler: a heavy hostile walker that harpoons a player and reels them in. Bevy space:
+Y up, -Z forward, +X right (its right arm is the harpoon launcher). Origin = between the
feet on the ground. Side outlines are plane "zy": points are (z, y).

A hunched cargo hauler: the head sits low and forward between two high shoulder humps, and
a big cable drum rides on its back above them. A cable runs from the launcher up the back
of the right arm and over to the drum; the drum's magenta power cell, the weak spot, sits
under it in the backpack, where the torso hides it from the front.

Rigid parts, each pivoted at its joint, for procedural animation:
  pelvis -> torso -> head
         -> torso -> upper_arm_l -> claw_l         (forearm + three-fingered claw)
         -> torso -> upper_arm_r -> harpoon_r      (the launcher, barrel along -Z)
         -> torso -> winch                         (the drum; the game spins it about X)
  pelvis -> thigh_l -> shin_l -> foot_l   (and _r)
Hinges run along X. The child carries the knuckle (a cylinder on the pivot axis); the
parent carries the fork plates and the bare-metal axle bolt, so a rotation reads as a joint.
The cable is cut into one straight run per part, so each run moves with what it lies on.
"""
import math

import hb

a = hb.Asset(__file__)

PAINT, ACCENT, DARK, BARE = "paint_primary", "paint_secondary", "metal_dark", "metal_bare"
LIGHT = "emissive_secondary"
SIDES = (("l", -1), ("r", 1))
BOLT = dict(mat=BARE, bevel=0)

LX = 0.24                                   # leg line, |x|: a wide stance
HIP = (0.92, 0.02)                          # (y, z)
KNEE, ANKLE = (0.52, -0.10), (0.15, 0.0)    # (y, z)
AX = 0.50                                   # arm line, |x|
SHOULDER = (1.64, -0.06)                    # (y, z)
ELBOW = (1.28, -0.06)
WAIST = (0, 1.08, 0.02)
NECK = (0, 1.62, -0.27)
EYE_Y, EYE_FRONT = 1.72, -0.525
DRUM, DRUM_R = (1.78, 0.22), 0.24           # winch axis (y, z), flange radius
CORE, CORE_R = (0, 1.30, 0.40), 0.13        # the weak spot
SMOOTH = 40                                 # curved bands meeting shallower than this stay smooth
CABLE_R = 0.016


def bar(p0, p1, w0, w1):
    """Quad in zy from p0 to p1 (z, y), half widths w0 at p0 and w1 at p1."""
    dz, dy = p1[0] - p0[0], p1[1] - p0[1]
    n = math.hypot(dz, dy)
    pz, py = -dy / n, dz / n
    return [(p0[0] + pz * w0, p0[1] + py * w0), (p1[0] + pz * w1, p1[1] + py * w1),
            (p1[0] - pz * w1, p1[1] - py * w1), (p0[0] - pz * w0, p0[1] - py * w0)]


def clevis(part, xc, piv, toward, r, length, inner, outer, bolt_r, mat=DARK):
    """Fork plates either side of a hinge along X at piv=(z, y): a round lobe of radius r
    round the pivot, running `length` toward the parent along `toward` (z, y). Plates span
    inner..outer from xc on each side; a bare axle bolt runs through both."""
    tz, ty = toward
    n = math.hypot(tz, ty)
    tz, ty = tz / n, ty / n
    phi = math.atan2(ty, tz)
    pts = []
    for k in range(5):                     # lobe: the half circle facing away from the parent, in 45 deg steps
        th = phi + math.pi / 2 + k * math.pi / 4
        pts.append((piv[0] + r * math.cos(th), piv[1] + r * math.sin(th)))
    pz, py = pts[-1][0] - piv[0], pts[-1][1] - piv[1]
    pts.append((piv[0] + pz + tz * length, piv[1] + py + ty * length))
    pts.append((piv[0] - pz + tz * length, piv[1] - py + ty * length))
    for sgn in (-1, 1):
        part.prism(pts, span=(xc + sgn * inner, xc + sgn * outer), mat=mat, smooth=True, smooth_angle=50, bevel=0)
    part.cylinder("x", (piv[1], piv[0]), bolt_r, (xc - outer - 0.014, xc + outer + 0.014), mat=BARE,
                  bevel=0)


def rod(part, p0, p1, r, mat, n=6):
    """A straight n-sided bar from p0 to p1 (Bevy world): cable runs and conduits."""
    d = [p1[k] - p0[k] for k in range(3)]
    ln = math.sqrt(sum(c * c for c in d))
    d = [c / ln for c in d]
    ref = (1.0, 0.0, 0.0) if abs(d[0]) < 0.9 else (0.0, 1.0, 0.0)
    u = (d[1] * ref[2] - d[2] * ref[1], d[2] * ref[0] - d[0] * ref[2], d[0] * ref[1] - d[1] * ref[0])
    un = math.sqrt(sum(c * c for c in u))
    u = [c / un for c in u]
    v = (d[1] * u[2] - d[2] * u[1], d[2] * u[0] - d[0] * u[2], d[0] * u[1] - d[1] * u[0])
    pts = []
    for p in (p0, p1):
        for k in range(n):
            th = 2 * math.pi * k / n
            pts.append(tuple(p[i] + r * (math.cos(th) * u[i] + math.sin(th) * v[i]) for i in range(3)))
    part.hull(pts, mat=mat, bevel=0)


def hexbolt(part, axis, center, span, r=0.02):
    part.cylinder(axis, center, r, span, segments=6, **BOLT)


# ---------------------------------------------------------------- pelvis
pelvis = a.part("pelvis", pivot=(0, HIP[0], HIP[1]), bevel=0.014, smooth_angle=SMOOTH)
pelvis.box((-0.15, 0.82, -0.13), (0.15, 1.04, 0.15), mat=DARK)
pelvis.box((-0.19, 1.02, -0.16), (0.19, 1.08, 0.17), mat=DARK)                 # waist bearing plate
pelvis.prism([(-0.18, 1.04), (0.18, 1.04), (0.12, 0.84), (-0.12, 0.84)], plane="xy",
             span=(-0.18, -0.12), mat=PAINT)                                    # front armour
pelvis.prism([(-0.15, 1.04), (0.15, 1.04), (0.10, 0.88), (-0.10, 0.88)], plane="xy",
             span=(0.14, 0.19), mat=PAINT)                                      # rear armour
pelvis.box((-0.09, 0.965, -0.188), (0.09, 0.985, -0.175), mat=LIGHT, bevel=0)  # status strip
pelvis.cylinder("x", (HIP[0], HIP[1]), 0.04, (-LX - 0.10, LX + 0.10), mat=BARE, bevel=0)  # hip axle
for _, s in SIDES:
    pelvis.cylinder("x", (HIP[0], HIP[1]), 0.065, sorted((s * (LX + 0.08), s * (LX + 0.10))), mat=DARK,
                    bevel=0)                                                    # axle end caps

# ---------------------------------------------------------------- torso
torso = a.part("torso", pelvis, pivot=WAIST, bevel=0.016, smooth_angle=SMOOTH)
torso.lathe([(0.14, 1.06), (0.14, 1.12), (0.17, 1.12), (0.17, 1.18)], axis="y", center=(0, 0.02),
            mats=[DARK, DARK, DARK], bevel=0)                                   # waist turntable
torso.box((-0.25, 1.14, -0.20), (0.25, 1.50, 0.14), mat=DARK)                  # core frame
CHEST = [(-0.22, 1.16), (-0.33, 1.36), (-0.36, 1.52), (-0.32, 1.62), (0.02, 1.62), (0.16, 1.50), (0.16, 1.16)]
torso.prism(CHEST, span=(-0.17, 0.17), mat=PAINT)                              # chest, low under the head
# the shoulder humps: high either side of the head, clear of the drum behind
HUMP = [(-0.28, 1.28), (-0.36, 1.48), (-0.38, 1.62), (-0.31, 1.80), (-0.19, 1.93), (-0.05, 1.95),
        (-0.02, 1.74), (0.05, 1.56), (0.16, 1.46), (0.16, 1.28)]
for side, s in SIDES:
    torso.prism(HUMP, span=sorted((s * 0.16, s * 0.37)), mat=PAINT)
    torso.prism([(-0.305, 1.81), (-0.19, 1.93), (-0.13, 1.94), (-0.25, 1.81)],
                span=sorted((s * 0.37, s * 0.385)), mat=ACCENT, bevel=0)        # bone stripe on the hump side
    torso.cylinder("x", SHOULDER, 0.10, sorted((s * 0.36, s * 0.415)), mat=DARK, bevel=0)  # shoulder hubs
torso.box((-0.10, 1.40, -0.37), (0.10, 1.47, -0.335), mat=ACCENT, bevel=0)     # chest plate stripe
# back: the backpack holding the winch's power cell (the weak spot), in a bone frame
torso.box((-0.21, 1.12, 0.10), (0.21, 1.50, 0.28), mat=DARK)
torso.frame((-0.175, CORE[1] - 0.175, 0.26), (0.175, CORE[1] + 0.175, 0.36), 0.04, "z", mat=ACCENT,
            bevel=0)                                                         # cell housing
ring = 8
torso.lathe([(CORE_R * math.sin(math.pi * k / ring), CORE[2] - CORE_R * math.cos(math.pi * k / ring))
             for k in range(1, ring)], axis="z", center=(CORE[0], CORE[1]),
            mats=["emissive_core"] * (ring - 2), bevel=0)                      # the cell (capped poles)
a.socket("SOCKET_weak", torso, at=CORE)
for _, s in SIDES:
    torso.box((min(s * 0.185, s * 0.20), 1.16, 0.28),
              (max(s * 0.185, s * 0.20), 1.44, 0.292), mat=LIGHT, bevel=0)     # backpack power strips
# winch cradle: fork plates either side of the drum, its motor on the left
clevis(torso, 0.0, (DRUM[1], DRUM[0]), (-0.7, -0.75), 0.10, 0.20, 0.245, 0.285, 0.035)
torso.lathe([(0.085, -0.285), (0.085, -0.35), (0.07, -0.365), (0.0, -0.365)], axis="x", center=DRUM,
            mats=[DARK, DARK, DARK], bevel=0)                                   # winch motor
torso.cylinder("x", DRUM, 0.088, (-0.335, -0.325), mat=LIGHT, bevel=0)          # motor power ring
torso.cylinder("x", DRUM, 0.05, (0.285, 0.30), mat=BARE, bevel=0)               # axle cap, right
rod(torso, (-0.12, 1.48, 0.30), (-0.32, 1.70, 0.22), 0.022, DARK)               # cell -> motor conduit
rod(torso, (0.575, 1.875, 0.11), (0.14, 1.985, 0.20), CABLE_R, BARE)            # cable: shoulder -> drum

# ---------------------------------------------------------------- winch (spins about X)
winch = a.part("winch", torso, pivot=(0, DRUM[0], DRUM[1]), bevel=0.012, smooth_angle=SMOOTH)
winch.lathe([(0, -0.235), (DRUM_R, -0.235), (DRUM_R, -0.20), (0.20, -0.20), (0.20, 0.20),
             (DRUM_R, 0.20), (DRUM_R, 0.235), (0, 0.235)], axis="x", center=DRUM,
            mats=[ACCENT, ACCENT, ACCENT, BARE, ACCENT, ACCENT, ACCENT])         # flanges, wound cable
winch.cylinder("x", DRUM, 0.07, (-0.245, 0.245), mat=DARK, bevel=0)              # hub
for _, s in SIDES:
    for k in range(3):
        th = 2 * math.pi * k / 3 + math.pi / 2
        c = (DRUM[0] + 0.165 * math.sin(th), DRUM[1] + 0.165 * math.cos(th))
        winch.cylinder("x", c, 0.022, sorted((s * 0.232, s * 0.247)), mat=LIGHT, bevel=0)  # winch lights

# ---------------------------------------------------------------- head
head = a.part("head", torso, pivot=NECK, bevel=0.014, smooth_angle=SMOOTH)
head.cylinder("y", (0, -0.27), 0.065, (1.56, 1.68), mat=DARK, bevel=0)          # neck
head.prism([(-0.47, 1.64), (-0.50, 1.67), (-0.50, 1.775), (-0.47, 1.80), (-0.21, 1.81), (-0.16, 1.77),
            (-0.16, 1.66), (-0.20, 1.64)], span=(-0.135, 0.135), mat=PAINT)   # sensor housing
head.box((-0.10, 1.615, -0.47), (0.10, 1.65, -0.25), mat=DARK, bevel=0.01)      # jaw
head.prism([(-0.54, 1.795), (-0.52, 1.825), (-0.35, 1.835), (-0.35, 1.795)], span=(-0.12, 0.12),
           mat=ACCENT, bevel=0.01)                                              # brow hood over the eye
head.lathe([(0.08, -0.48), (0.08, -0.505), (0.068, -0.512), (0, -0.512)], axis="z",
           center=(0, EYE_Y), mats=[DARK, DARK, DARK], bevel=0)                 # eye bezel
head.lathe([(0.058, -0.505), (0.058, -0.515), (0.045, EYE_FRONT), (0, EYE_FRONT)], axis="z",
           center=(0, EYE_Y), mats=["emissive"] * 3, bevel=0)                   # lens
a.socket("SOCKET_eye", head, at=(0, EYE_Y, EYE_FRONT), forward=(0, 0, -1))

# ---------------------------------------------------------------- arms
uppers = {}
for side, s in SIDES:
    x = s * AX
    up = a.part(f"upper_arm_{side}", torso, pivot=(x, SHOULDER[0], SHOULDER[1]), bevel=0.014,
                smooth_angle=SMOOTH)
    up.cylinder("x", SHOULDER, 0.088, sorted((s * 0.415, s * 0.56)), mat=DARK, bevel=0)  # shoulder knuckle
    up.prism([(-0.20, 1.50), (-0.25, 1.64), (-0.20, 1.80), (-0.08, 1.88), (0.04, 1.88), (0.08, 1.80),
              (0.08, 1.50)],
             span=sorted((s * 0.39, s * 0.62)), mat=PAINT)                       # pauldron
    up.prism([(-0.245, 1.65), (-0.225, 1.73), (0.08, 1.73), (0.08, 1.65)],
             span=sorted((s * 0.62, s * 0.632)), mat=ACCENT, bevel=0)           # bone band
    up.box((x - 0.055, 1.32, -0.115), (x + 0.055, 1.56, -0.005), mat=DARK)
    up.box((x - 0.075, 1.36, -0.14), (x + 0.075, 1.52, 0.02), mat=PAINT)
    clevis(up, x, (ELBOW[1], ELBOW[0]), (0, 1), 0.07, 0.12, 0.058, 0.082, 0.03)
    uppers[side] = up
rod(uppers["r"], (AX + 0.085, 1.36, 0.07), (AX + 0.08, 1.88, 0.11), CABLE_R, BARE)  # cable up the arm

# left: a heavy forearm and a three-fingered claw
xl = -AX
claw = a.part("claw_l", uppers["l"], pivot=(xl, ELBOW[0], ELBOW[1]), bevel=0.014, smooth_angle=SMOOTH)
claw.cylinder("x", ELBOW, 0.062, (xl - 0.055, xl + 0.055), mat=DARK, bevel=0)
claw.prism(bar((-0.06, 1.24), (-0.20, 0.94), 0.06, 0.05), span=(xl - 0.045, xl + 0.045), mat=DARK)  # strut
claw.prism(bar((-0.08, 1.20), (-0.22, 0.96), 0.085, 0.075), span=(xl - 0.09, xl + 0.09), mat=PAINT)  # shell
claw.prism(bar((-0.075, 1.17), (-0.20, 0.99), 0.07, 0.06), span=(xl + 0.09, xl + 0.10), mat=ACCENT,
           bevel=0)                                                             # inner bone plate
claw.cylinder("y", (xl, -0.22), 0.07, (0.88, 0.96), mat=DARK, bevel=0)          # wrist rotator
claw.box((xl - 0.10, 0.74, -0.36), (xl + 0.10, 0.90, -0.10), mat=DARK)        # palm
claw.box((xl - 0.06, 0.90, -0.32), (xl + 0.06, 0.912, -0.24), mat=LIGHT, bevel=0)  # grip light
claw.cylinder("x", (0.79, -0.32), 0.04, (xl - 0.105, xl + 0.105), mat=BARE, bevel=0)  # finger pin
claw.cylinder("x", (0.78, -0.14), 0.04, (xl - 0.05, xl + 0.05), mat=BARE, bevel=0)  # thumb pin
FINGER = [(-0.24, 0.82), (-0.38, 0.85), (-0.49, 0.78), (-0.53, 0.64), (-0.48, 0.48), (-0.42, 0.49),
          (-0.45, 0.63), (-0.42, 0.72), (-0.33, 0.76), (-0.24, 0.75)]
for x0 in (xl - 0.10, xl + 0.03):
    claw.prism(FINGER, span=(x0, x0 + 0.07), mat=BARE, bevel=0)
claw.prism([(-0.20, 0.80), (-0.10, 0.79), (-0.04, 0.67), (-0.08, 0.50), (-0.14, 0.505), (-0.11, 0.66),
            (-0.15, 0.73), (-0.22, 0.74)], span=(xl - 0.04, xl + 0.04), mat=BARE, bevel=0)  # thumb

# right: the harpoon launcher, pointing forward from the elbow
xr = AX
BORE = 1.22
MUZZLE_Z = -0.80
hp = a.part("harpoon_r", uppers["r"], pivot=(xr, ELBOW[0], ELBOW[1]), bevel=0.014, smooth_angle=SMOOTH)
hp.cylinder("x", ELBOW, 0.062, (xr - 0.055, xr + 0.055), mat=DARK, bevel=0)
hp.box((xr - 0.05, 1.18, -0.18), (xr + 0.05, 1.32, -0.06), mat=DARK, bevel=0.01)  # elbow block
hp.prism([(-0.14, 1.10), (-0.14, 1.32), (-0.18, 1.36), (-0.42, 1.36), (-0.48, 1.31), (-0.48, 1.12),
          (-0.44, 1.08), (-0.18, 1.08)], span=(xr - 0.10, xr + 0.10), mat=PAINT)  # launcher housing
hp.prism([(-0.20, 1.36), (-0.21, 1.38), (-0.40, 1.38), (-0.41, 1.36)], span=(xr - 0.06, xr + 0.06),
         mat=ACCENT, bevel=0)                                                   # bone top plate
hp.lathe([(0.095, -0.48), (0.095, -0.72), (0.12, -0.72), (0.12, MUZZLE_Z), (0.065, MUZZLE_Z),
          (0.065, MUZZLE_Z + 0.025), (0, MUZZLE_Z + 0.025)], axis="z", center=(xr, BORE),
         mats=[DARK, DARK, ACCENT, DARK, DARK, DARK], bevel=0.01)               # barrel, muzzle collar
# the barbed hook, sat in the muzzle: a spike with three barbs swept back at the mouth
hp.lathe([(0.032, MUZZLE_Z + 0.025), (0.032, -0.845), (0.06, -0.845), (0.06, -0.87), (0, -0.95)], axis="z",
         center=(xr, BORE), mats=[BARE, BARE, BARE, BARE], bevel=0)
for k in range(3):
    th = math.pi / 2 + k * 2 * math.pi / 3
    cx, cy = math.cos(th), math.sin(th)
    tx, ty = -cy, cx
    pts = []
    for rr, z in ((0.025, -0.86), (0.025, -0.895), (0.14, -0.815)):
        for w in (-0.013, 0.013):
            pts.append((xr + rr * cx + w * tx, BORE + rr * cy + w * ty, z))
    hp.hull(pts, mat=BARE, bevel=0)
for y in (1.29, 1.25, 1.21):                                                    # power lights, outer face
    hp.box((xr + 0.098, y - 0.012, -0.44), (xr + 0.106, y + 0.012, -0.40), mat=LIGHT, bevel=0)
hp.box((xr + 0.07, 1.30, -0.24), (xr + 0.105, 1.37, -0.15), mat=DARK, bevel=0)  # cable fairlead
rod(hp, (xr + 0.085, 1.35, -0.20), (xr + 0.085, 1.36, 0.07), CABLE_R, BARE)    # cable out the back
a.socket("SOCKET_muzzle", hp, at=(xr, BORE, MUZZLE_Z), forward=(0, 0, -1))

# ---------------------------------------------------------------- legs
for side, s in SIDES:
    x = s * LX
    thigh = a.part(f"thigh_{side}", pelvis, pivot=(x, HIP[0], HIP[1]), bevel=0.014, smooth_angle=SMOOTH)
    thigh.cylinder("x", HIP, 0.10, (x - 0.075, x + 0.075), mat=DARK, bevel=0.01)  # hip knuckle
    thigh.box((x - 0.06, 0.60, -0.10), (x + 0.06, 0.92, 0.07), mat=DARK)
    thigh.prism([(-0.16, 0.90), (0.12, 0.90), (0.12, 0.72), (0.06, 0.62), (-0.16, 0.62), (-0.20, 0.77)],
                span=(x - 0.125, x + 0.125), mat=PAINT)                         # thigh armour
    clevis(thigh, x, (KNEE[1], KNEE[0]), (0.12, 0.40), 0.08, 0.13, 0.07, 0.095, 0.035)

    shin = a.part(f"shin_{side}", thigh, pivot=(x, KNEE[0], KNEE[1]), bevel=0.014, smooth_angle=SMOOTH)
    shin.cylinder("x", KNEE, 0.075, (x - 0.068, x + 0.068), mat=DARK, bevel=0)  # knee knuckle (ends hidden)
    shin.prism([(-0.15, 0.63), (-0.215, 0.59), (-0.23, 0.48), (-0.19, 0.42), (-0.13, 0.43), (-0.13, 0.61)],
               span=(x - 0.085, x + 0.085), mat=ACCENT, bevel=0.01)             # knee cap
    shin.box((x - 0.055, 0.20, -0.10), (x + 0.055, 0.48, 0.05), mat=DARK)
    shin.prism([(-0.17, 0.46), (-0.185, 0.32), (-0.13, 0.21), (0.08, 0.21), (0.12, 0.32), (0.10, 0.46)],
               span=(x - 0.11, x + 0.11), mat=PAINT)                            # shin armour
    clevis(shin, x, (ANKLE[1], ANKLE[0]), (0, 1), 0.065, 0.12, 0.065, 0.09, 0.028)

    foot = a.part(f"foot_{side}", shin, pivot=(x, ANKLE[0], ANKLE[1]), bevel=0.014, smooth_angle=SMOOTH)
    foot.cylinder("x", ANKLE, 0.06, (x - 0.063, x + 0.063), mat=DARK, bevel=0)  # ankle knuckle
    foot.box((x - 0.13, 0.0, -0.34), (x + 0.13, 0.022, 0.20), mat=BARE, bevel=0)  # mag plate
    foot.box((x - 0.14, 0.018, -0.345), (x + 0.14, 0.075, 0.205), mat=DARK, bevel=0.012)  # magnet block
    foot.box((x - 0.144, 0.032, -0.349), (x + 0.144, 0.046, 0.209), mat=LIGHT, bevel=0)  # mag-boot glow seam
    foot.prism([(0.18, 0.07), (-0.20, 0.07), (-0.20, 0.15), (-0.12, 0.15), (-0.08, 0.12), (0.14, 0.12),
                (0.18, 0.10)], span=(x - 0.115, x + 0.115), mat=PAINT)          # boot upper
    foot.prism([(-0.195, 0.07), (-0.33, 0.07), (-0.34, 0.10), (-0.30, 0.16), (-0.195, 0.16)],
               span=(x - 0.125, x + 0.125), mat=ACCENT)                         # toe cap
    foot.box((x - 0.09, 0.06, 0.10), (x + 0.09, 0.15, 0.20), mat=DARK, bevel=0.012)  # heel coil housing
    a.socket(f"SOCKET_foot_{side}", foot, at=(x, 0, -0.06), forward=(0, -1, 0), up=(0, 0, -1))

    a.collider(f"COL_thigh_{side}", around=[thigh], parent=thigh)
    a.collider(f"COL_shin_{side}", around=[shin, foot], parent=shin)

# ---------------------------------------------------------------- affixes
# Variations the game shows only on a robot that has them (it hides every affix_ part
# otherwise): affix_<tag>_<what>, each on the part it rides. Their lights are
# emissive_secondary; the game tints them per affix. None of them sits between the
# weak spot and a view from behind.

# Armoured: thick dark slabs bolted over the chest, head, thighs, shins and the claw arm.
plate = a.part("affix_armour_chest", torso, pivot=WAIST, bevel=0.016)
plate.prism([(-0.285, 1.30), (-0.355, 1.28), (-0.40, 1.58), (-0.32, 1.60)], span=(-0.22, 0.22), mat=DARK)
for x in (-0.18, 0.18):
    for y in (1.33, 1.55):
        zf = -0.355 - 0.045 * (y - 1.28) / 0.30
        hexbolt(plate, "z", (x, y), (zf + 0.005, zf - 0.016))
helmet = a.part("affix_armour_head", head, pivot=NECK, bevel=0.014)
helmet.prism([(-0.52, 1.82), (-0.50, 1.865), (-0.19, 1.865), (-0.16, 1.825)], span=(-0.15, 0.15), mat=DARK)
for _, s in SIDES:
    helmet.box((min(s * 0.135, s * 0.155), 1.65, -0.49),
               (max(s * 0.135, s * 0.155), 1.83, -0.25), mat=DARK, bevel=0.01)  # cheek guards
    hexbolt(helmet, "x", (1.75, -0.43), sorted((s * 0.15, s * 0.17)), r=0.016)
for side, s in SIDES:
    x = s * LX
    tp = a.part(f"affix_armour_thigh_{side}", f"thigh_{side}", pivot=(x, HIP[0], HIP[1]), bevel=0.014)
    tp.prism([(-0.15, 0.88), (0.09, 0.88), (0.09, 0.72), (0.04, 0.65), (-0.15, 0.65), (-0.185, 0.77)],
             span=sorted((x + s * 0.12, x + s * 0.15)), mat=DARK)               # outer slab
    tp.prism([(-0.165, 0.88), (-0.22, 0.88), (-0.24, 0.68), (-0.18, 0.68)],
             span=(x - 0.10, x + 0.10), mat=DARK)                               # front slab
    for z in (-0.08, 0.03):
        hexbolt(tp, "x", (0.82, z), sorted((x + s * 0.145, x + s * 0.165)))
    sp = a.part(f"affix_armour_shin_{side}", f"shin_{side}", pivot=(x, KNEE[0], KNEE[1]), bevel=0.014)
    sp.prism([(-0.165, 0.42), (-0.225, 0.42), (-0.24, 0.32), (-0.18, 0.20), (-0.135, 0.22), (-0.18, 0.32)],
             span=(x - 0.10, x + 0.10), mat=DARK)
    hexbolt(sp, "z", (x, 0.36), (-0.23, -0.25))
cp = a.part("affix_armour_claw", claw, pivot=(xl, ELBOW[0], ELBOW[1]), bevel=0.014)
cp.prism(bar((-0.06, 1.21), (-0.21, 0.96), 0.095, 0.085), span=(xl - 0.13, xl - 0.09), mat=DARK)
for t in (0.25, 0.75):
    z, y = -0.06 + t * (-0.15), 1.21 + t * (-0.25)
    hexbolt(cp, "x", (y, z), (xl - 0.145, xl - 0.125))

# Shielded: an emitter ring on a mast standing off the winch motor, beside the drum.
emit = a.part("affix_shield_emitter", torso, pivot=WAIST, bevel=0)
EM = (-0.34, 0.22)                                                              # (x, z)
emit.box((-0.375, 1.84, 0.18), (-0.30, 1.885, 0.26), mat=DARK)
emit.cylinder("y", EM, 0.018, (1.88, 1.97), segments=8, **BOLT)
emit.lathe([(0.02, 1.955), (0.08, 1.985), (0.088, 2.0), (0.04, 2.005), (0, 2.01)], axis="y", center=EM,
           mats=[DARK, DARK, DARK, DARK])
emit.lathe([(0.091, 1.984), (0.091, 1.999)], axis="y", center=EM, mat=LIGHT)

# Overclocked: a finned radiator on the right of the drum, glowing between its fins, and an
# exhaust stack on each shoulder hump, where it vents.
oc = a.part("affix_overclock_radiator", torso, pivot=WAIST, bevel=0)
oc.box((0.29, 1.58, 0.13), (0.35, 1.86, 0.31), mat=LIGHT)                       # the glow behind the fins
for k in range(6):
    y = 1.585 + k * 0.052
    oc.box((0.295, y, 0.10), (0.40, y + 0.014, 0.34), mat=BARE)
oc.box((0.285, 1.565, 0.09), (0.40, 1.585, 0.35), mat=DARK)
vent = a.part("affix_overclock_vent", torso, pivot=WAIST, bevel=0)
for side, s in SIDES:
    c = (s * 0.27, -0.10)
    vent.lathe([(0.045, 1.92), (0.045, 1.95), (0.036, 1.962), (0.036, 2.03), (0, 2.03)], axis="y", center=c,
               mats=[DARK, DARK, DARK, LIGHT])                                   # its mouth glows
    a.socket(f"SOCKET_vent_{side}", torso, at=(c[0], 2.03, c[1]), forward=(0, 1, 0), up=(0, 0, 1))

# Volatile: two fuel canisters strapped either side of the power cell, and a third slung
# across the back of the hips; warning bands on each.
vol = a.part("affix_volatile_canisters", torso, pivot=WAIST, bevel=0.012)
for _, s in SIDES:
    c = (s * 0.29, 0.20)
    vol.lathe([(0.045, 1.14), (0.065, 1.17), (0.065, 1.43), (0.045, 1.46)], axis="y", center=c,
              mats=[ACCENT, ACCENT, ACCENT], bevel=0)
    vol.cylinder("y", c, 0.069, (1.28, 1.32), mat=LIGHT, bevel=0)
    vol.box((min(s * 0.20, s * 0.24), 1.25, 0.17), (max(s * 0.20, s * 0.24), 1.35, 0.23), mat=DARK, bevel=0)
hipcan = a.part("affix_volatile_hip", pelvis, pivot=(0, HIP[0], HIP[1]), bevel=0.012)
hipcan.lathe([(0.04, -0.13), (0.06, -0.11), (0.06, 0.11), (0.04, 0.13)], axis="x", center=(0.96, 0.25),
             mats=[ACCENT, ACCENT, ACCENT], bevel=0)
hipcan.cylinder("x", (0.96, 0.25), 0.064, (-0.02, 0.02), mat=LIGHT, bevel=0)
for x in (-0.08, 0.08):
    hipcan.box((x - 0.012, 0.89, 0.17), (x + 0.012, 1.03, 0.315), mat=DARK, bevel=0)  # straps

# ---------------------------------------------------------------- colliders
# Torso hulls from outlines (the lathes' many near-coplanar points make Blender's hull
# wobble past the verifier's convexity tolerance).
a.collider("COL_torso", parent=torso, points=[(x, y, z) for z, y in HUMP for x in (-0.385, 0.385)]
           + [(x, y, z) for z, y in CHEST for x in (-0.17, 0.17)]
           + [(x, y, z) for x in (-0.21, 0.21) for y in (1.12, 1.50) for z in (0.28,)]
           + [(x, y, z) for x in (-0.175, 0.175) for y in (1.125, 1.475) for z in (0.36,)]
           + [(x, y, z) for x in (-0.12, 0.12) for y in (1.19, 1.41) for z in (0.52,)])
a.collider("COL_winch", parent=torso, points=[
    (x, DRUM[0] + DRUM_R * math.sin(math.pi * k / 4), DRUM[1] + DRUM_R * math.cos(math.pi * k / 4))
    for x in (-0.245, 0.245) for k in range(8)])
a.collider("COL_pelvis", parent=pelvis, points=[(x, y, z) for x in (-0.19, 0.19) for y in (0.82, 1.08)
                                                for z in (-0.18, 0.19)])
a.collider("COL_head", around=[head], parent=head)
a.export()
