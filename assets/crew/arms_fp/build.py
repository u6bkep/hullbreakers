"""Crew suit arms, first person. Bevy space: +Y up, -Z forward, +X right.

Two independent arms, arm_r and arm_l, each built with ITS OWN grip point at the origin
(they overlap in the file; the game places each at the grip of whatever that hand holds).
Each is a gloved fist closed round a pistol grip raked like the SMG's (assets/weapons/
smg_frame: 30 mm wide, ~54 mm front to back, bottom further back), index finger through
the trigger guard onto the trigger, thumb along the far side; then the glove gauntlet,
the suit cuff with its wrist display, and the forearm running back ~0.35 m, dropping and
angled outward. The left arm is the right one mirrored in x.

Same suit as assets/crew/suit: bone shell (paint_primary), the player's identity colour
(paint_secondary, recoloured per player) as a band on the forearm shell, charcoal soft suit
and glove (polymer), rubber pads, lime lights (emissive_secondary).
"""
import math

import hb

a = hb.Asset(__file__)

SHELL, ID, SOFT, PAD, DARK = "paint_primary", "paint_secondary", "polymer", "rubber", "metal_dark"
LIGHT = "emissive_secondary"
RAKE = -0.13          # the grip's centre line: z = RAKE * y (SMG grip outline)
WRIST = (0.012, -0.006, 0.075)
ELBOW_END = (0.072, -0.078, 0.40)     # rear end of the sleeve (right arm)


def _sub(p, q):
    return tuple(p[i] - q[i] for i in range(3))


def _norm(v):
    n = math.sqrt(sum(c * c for c in v))
    return tuple(c / n for c in v)


def _cross(u, v):
    return (u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0])


def _dot(u, v):
    return sum(u[i] * v[i] for i in range(3))


# forearm frame (right arm): along it, outward and up across it
FD = _norm(_sub(ELBOW_END, WRIST))
FUP = _norm(tuple((0, 1, 0)[i] - _dot((0, 1, 0), FD) * FD[i] for i in range(3)))
FOUT = _cross(FUP, FD)


def F(t, ang, r):
    """A point on the forearm: t metres back from the wrist, at angle ang (deg, 0 = up,
    90 = outward) and radius r from its axis."""
    c, s = math.cos(math.radians(ang)), math.sin(math.radians(ang))
    return tuple(WRIST[i] + FD[i] * t + r * (c * FUP[i] + s * FOUT[i]) for i in range(3))


def build_arm(side, s):
    arm = a.part(f"arm_{side}", pivot=(0, 0, 0), bevel=0.003, smooth_angle=40)

    def M(p):
        return (s * p[0], p[1], p[2])

    def H(pts, mat, bevel=None):
        arm.hull([M(p) for p in pts], mat=mat, bevel=bevel)

    def G(x, y, z):                    # grip-relative, raked with the grip
        return (x, y, z + RAKE * y)

    def bar(p0, p1, w, h, mat, bevel=0, rake=True):
        """Finger bar from p0 to p1 (grip-relative, raked unless rake=False), lying in xz: half
        width w across it, half height h; an octagon in section so the knuckles read round."""
        q0, q1 = (G(*p0), G(*p1)) if rake else (p0, p1)
        dx, dz = q1[0] - q0[0], q1[2] - q0[2]
        n = math.hypot(dx, dz)
        lx, lz = -dz / n, dx / n
        pts = []
        for q in (q0, q1):
            for k in range(8):
                t = 2 * math.pi * (k + 0.5) / 8
                cw, ch = math.cos(t) * w / math.cos(math.pi / 8), math.sin(t) * h / math.cos(math.pi / 8)
                pts.append((q[0] + cw * lx, q[1] + ch, q[2] + cw * lz))
        H(pts, mat, bevel)

    def tube(t0, t1, r0, r1, mat, n=10, angs=None, bevel=None):
        """Hull of rings round the forearm axis; angs limits it to an arc (a shell plate)."""
        angs = angs or [360.0 * (k + 0.5) / n for k in range(n)]
        H([F(t0, g, r0) for g in angs] + [F(t1, g, r1) for g in angs], mat, bevel)

    # ---- glove: palm and back of the hand, round the grip's right side and back
    front = ((0.043, -0.028), (0.046, 0.02), (-0.019, 0.02), (-0.019, 0.0), (0.0, -0.031))
    heel = ((0.046, 0.0), (0.043, 0.04), (0.034, 0.056), (-0.010, 0.056), (-0.019, 0.03), (-0.019, 0.0))
    H([G(x, y, z) for y in (0.045, -0.074) for x, z in front], SOFT)             # glove: fingers' root
    H([G(x, y, z) for y in (0.034, -0.068) for x, z in heel], SOFT)              # palm heel round the back
    plate = ((-0.024, 0.041), (0.03, 0.033), (0.03, -0.028), (-0.024, -0.054))
    H([G(0.044, y, z) for z, y in plate]
      + [G(0.054, y * 0.8 - 0.002, z * 0.8 + 0.001) for z, y in plate], SHELL, bevel=0.002)   # back-of-hand plate
    H([G(x, y, z) for x, z in ((0.036, -0.032), (0.048, -0.024), (0.046, -0.036), (0.038, -0.042))
       for y in (-0.064, 0.042)], PAD, bevel=0.002)                              # knuckle pad
    # ---- three fingers under the trigger guard: across the front, back along the far side
    for yc, h in ((-0.0115, 0.0092), (-0.0335, 0.0092), (-0.0555, 0.0085)):
        bar((0.038, yc, -0.032), (-0.008, yc, -0.040), 0.0095, h, SOFT)
        bar((-0.011, yc, -0.042), (-0.025, yc, -0.012), 0.0092, h * 0.97, SOFT)
        bar((-0.026, yc, -0.016), (-0.022, yc, 0.010), 0.0088, h * 0.94, SOFT)
    # ---- index finger: along the frame, through the guard, pad on the trigger (~ y 0.03, z -0.049)
    for p0, p1, w in (((0.036, 0.036, -0.026), (0.018, 0.032, -0.055), 0.0088),
                      ((0.021, 0.032, -0.058), (0.000, 0.030, -0.058), 0.0086),
                      ((0.003, 0.030, -0.059), (-0.012, 0.029, -0.051), 0.0082)):
        bar(p0, p1, w, 0.0088, SOFT, rake=False)
    # ---- thumb: over the web and along the far side, below the receiver
    bar((0.022, 0.034, 0.048), (-0.024, 0.036, 0.020), 0.011, 0.011, SOFT)
    bar((-0.026, 0.036, 0.024), (-0.027, 0.036, -0.008), 0.0095, 0.0098, SOFT)
    bar((-0.027, 0.036, -0.006), (-0.022, 0.035, -0.026), 0.0088, 0.009, SOFT)
    # ---- wrist, gauntlet, cuff and forearm
    tube(-0.05, 0.005, 0.034, 0.036, SOFT, n=8)                                  # glove wrist
    tube(0.0, 0.055, 0.035, 0.044, SOFT)                                          # glove gauntlet
    tube(0.042, 0.064, 0.048, 0.048, DARK, n=12)                                  # suit cuff ring
    tube(0.07, 0.33, 0.044, 0.05, SOFT, bevel=0)                                  # sleeve (rear end off screen)
    shell = [-80 + 20 * k for k in range(10)]                                     # top and outer side
    tube(0.066, 0.27, 0.051, 0.056, SHELL, angs=shell)                            # forearm shell
    tube(0.20, 0.235, 0.0575, 0.0585, ID, angs=[-76 + 19 * k for k in range(9)])  # identity band
    # wrist display on top, angled toward the eye
    base = [F(t, g, r) for t in (0.072, 0.13) for g in (-34, 14) for r in (0.05, 0.062)]
    H(base, DARK, bevel=0.002)
    for (t0, t1), (g0, g1) in (((0.084, 0.09), (-26, 6)), ((0.102, 0.108), (-26, -12))):  # status bar and dot
        H([F(t, g, r) for t in (t0, t1) for g in (g0, g1) for r in (0.0615, 0.0628)], LIGHT, bevel=0)
    a.socket(f"SOCKET_grip_{side}", arm, at=(0, 0, 0), forward=(0, 0, -1), up=(0, 1, 0))


build_arm("r", 1)
build_arm("l", -1)
a.export()
