"""Limpet: a rock mimic about 0.8 m across that clings to asteroids. Bevy space: +Y away from
the surface it clings to, -Z forward, +X right. Origin on that surface, centred under it.

Read at range: a low, lumpy, faceted grey-brown rock, like the game's asteroids. Closer: the
rock is cracked into overlapping plates that all follow one lumpy dome (so they read as one
stone split, not a pile of pebbles); a frill of glistening purple-red foot lobes shows round
its base; a fleshy siphon pokes out under its front brow, where a cluster of pale-green eye
pits glows; and a thin magenta crack shows where the three back plates meet. Lights, loud to
quiet: the eye pits (emissive) and the weak-spot sac under the back plates' junction
(emissive_core, SOCKET_weak at its centre).

Moving parts (all pivoted at their joints, local -Z pointing from the joint to the free edge
or tip, so a positive rotation about local X lifts or curls it up):
  plate_l / plate_r   hinge along their inner edge by the spine (local X along the hinge);
  plate_rear          hinges at its front edge;
  skirt_0..skirt_7    foot lobes round the rim, k*45 deg clockwise from the front seen from
                      above (k=2 is +X), pivoted at the rim's inner edge;
  snout               pitches about its root under the brow.

LIMPET_OPEN=<degrees> builds the plates lifted that far (to look at the exposed weak spot;
never commit a build made with it).
"""
import math
import os
import random

from mathutils import Matrix, Vector

import hb
from hb import geometry

a = hb.Asset(__file__)
ROCK, DARK, FLESH, GLOW, CORE = "hide_primary", "hide_secondary", "flesh", "emissive", "emissive_core"
OPEN = math.radians(float(os.environ.get("LIMPET_OPEN", "0")))

# ---------------------------------------------------------------- the dome
# The carapace's plan is an ellipse; its top follows one lumpy dome. Every rock piece takes
# its top from it, so the pieces line up like a stone cracked apart.
RX, RZ, ZC = 0.355, 0.405, 0.03
Y0, H = 0.05, 0.30


def edge(t):
    """The rim's wobble: its distance from the centre at plan angle t (0 ahead, clockwise
    from above), as a fraction of the ellipse's."""
    return 1.0 + 0.10 * geometry.fbm(Vector((2.0 * math.sin(t), 0.0, 2.0 * math.cos(t))), 9)


def rho(x, z):
    return math.hypot(x / RX, (z - ZC) / RZ) / edge(math.atan2(x, -(z - ZC)))


def dome(x, z):
    r = min(rho(x, z), 1.0)
    lumpy = 0.05 * geometry.fbm(Vector((x * 4.0, 0.0, z * 4.0)), 7) + 0.05 * geometry.fbm(Vector((x * 1.6, 0.0, z * 1.6)), 8)
    return Y0 + H * (1.0 - r * r) ** 0.62 + lumpy * (1.0 - 0.5 * r)


def clip(poly, a_, b_, c_):
    """Keep the part of a convex polygon [(x, z)] where a*x + b*z + c >= 0."""
    out = []
    for i in range(len(poly)):
        p, q = poly[i], poly[(i + 1) % len(poly)]
        fp, fq = a_ * p[0] + b_ * p[1] + c_, a_ * q[0] + b_ * q[1] + c_
        if fp >= 0:
            out.append(p)
        if (fp >= 0) != (fq >= 0):
            t = fp / (fp - fq)
            out.append((p[0] + t * (q[0] - p[0]), p[1] + t * (q[1] - p[1])))
    return out


def hull2d(pts):
    pts = sorted(set(pts))

    def cross(o, a_, b_):
        return (a_[0] - o[0]) * (b_[1] - o[1]) - (a_[1] - o[1]) * (b_[0] - o[0])

    lo, hi = [], []
    for p in pts:
        while len(lo) >= 2 and cross(lo[-2], lo[-1], p) <= 0:
            lo.pop()
        lo.append(p)
    for p in reversed(pts):
        while len(hi) >= 2 and cross(hi[-2], hi[-1], p) <= 0:
            hi.pop()
        hi.append(p)
    return lo[:-1] + hi[:-1]


GAP, ROUND = 0.012, 0.022       # crack width; corner rounding (opens a small hole where three meet)


def region(lines, scale=1.0):
    """The carapace's outline (shrunk to `scale`) cut by `lines` (a, b, c: keep
    a*x + b*z + c >= 0, (a, b) unit), each pulled back by half the crack, with rounded
    corners. A convex outline in plan."""
    poly = [((scale * RX * edge(t) - ROUND) * math.sin(t), ZC - (scale * RZ * edge(t) - ROUND) * math.cos(t))
            for t in (2 * math.pi * k / 72 for k in range(72))]
    for a_, b_, c_ in lines:
        poly = clip(poly, a_, b_, c_ - GAP / 2 - ROUND)
    ring = [(x + ROUND * math.cos(t), z + ROUND * math.sin(t)) for x, z in poly
            for t in (2 * math.pi * k / 10 for k in range(10))]
    return hull2d(ring)


def line(p, q):
    """The line through plan points p and q, keeping the side to the left walking p -> q."""
    dx, dz = q[0] - p[0], q[1] - p[1]
    n = math.hypot(dx, dz)
    a_, b_ = -dz / n, dx / n
    return (a_, b_, -(a_ * p[0] + b_ * p[1]))


def flip(ln):
    return (-ln[0], -ln[1], -ln[2])


def resample(outline, step):
    out = []
    for i in range(len(outline)):
        p, q = outline[i], outline[(i + 1) % len(outline)]
        n = max(1, int(math.dist(p, q) / step))
        out += [(p[0] + (q[0] - p[0]) * k / n, p[1] + (q[1] - p[1]) * k / n) for k in range(n)]
    return out


def inside(poly, x, z):
    return all((q[0] - p[0]) * (z - p[1]) - (q[1] - p[1]) * (x - p[0]) >= 0
               for p, q in zip(poly, poly[1:] + poly[:1]))


def slab(outline, seed, thick=0.045, base=None, step=0.062, lift=(0.0, 0.0, 0.0)):
    """Hull points for a rock piece over `outline`: its top on the dome (with a little noise of
    its own), its underside `thick` below at the cracks and down at `base(x, z)` (a height)
    where it reaches the carapace rim. lift (dy, dy/dx, dy/dz) sets the piece a little
    proud of or under its neighbours, so the plates overlap like shingles."""
    rnd = random.Random(seed)
    top, bottom = [], []
    rim = resample(outline, step * 0.8)
    xs = [p[0] for p in outline]
    zs = [p[1] for p in outline]
    interior = []
    x = min(xs) + step / 2
    while x < max(xs):
        z = min(zs) + step / 2
        while z < max(zs):
            jx, jz = x + rnd.uniform(-0.35, 0.35) * step, z + rnd.uniform(-0.35, 0.35) * step
            if inside(outline, jx, jz):
                interior.append((jx, jz))
            z += step
        x += step
    cx, cz = sum(xs) / len(xs), sum(zs) / len(zs)

    def toward_centre(x, z, d):
        n = math.hypot(cx - x, cz - z)
        return (x + (cx - x) * d / n, z + (cz - z) * d / n)

    def top_y(x, z):
        return dome(x, z) + lift[0] + lift[1] * x + lift[2] * z + rnd.uniform(-0.006, 0.01)

    # The edge is worn round: the rim sits a little low, a ring just inside it at full height,
    # and the underside is drawn in, so the cracks between pieces are V-shaped grooves.
    for x, z in rim:
        top.append((x, top_y(x, z) - 0.012, z))
    for x, z in resample(outline, step * 1.3):
        ix, iz = toward_centre(x, z, 0.02)
        top.append((ix, top_y(ix, iz), iz))
    for x, z in interior:
        if all(math.dist((x, z), q) > 0.025 for q in rim):
            top.append((x, top_y(x, z), z))
    for x0, z0 in rim:
        x, z = toward_centre(x0, z0, 0.012)
        y = dome(x0, z0) - thick
        if base is not None and rho(x, z) > 0.86:
            y = min(y, base(x, z))
        bottom.append((x, max(y, 0.01), z))
    return top + bottom


def tone(y_dark):
    """Light rock above y_dark, dark below: the low flanks of a stone are weathered darker."""
    return lambda c: ROCK if c[1] >= y_dark else DARK


# Cracks, in plan (x, z). Slightly askew, as a stone splits.
FRONT = line((0.4, 0.0), (-0.4, -0.10))                   # keeps the front side
SPINE = line((0.0, 0.205), (0.035, -0.1))                 # keeps the right side
REAR = line((-0.4, 0.25), (0.4, 0.17))                    # keeps the rear side
BROW_L = line((-0.15, -0.5), (-0.09, -0.05))               # keeps the left cheek
BROW_R = line((0.10, -0.0), (0.15, -0.5))                  # keeps the right cheek

# ---------------------------------------------------------------- shell (the fixed body)
shell = a.part("shell")
# The foot (a flat flesh pad over the footprint, what clings) and the mantle (a flesh dome
# under the plates, seen in the cracks and when they lift).
shell.blob((0.0, 0.04, 0.02), (0.30, 0.04, 0.35), mat=FLESH, segments=12)
shell.blob((0.0, 0.12, 0.05), (0.25, 0.12, 0.24), mat=FLESH, noise=0.06, seed=3, segments=14)

# The brow: the front middle piece, its underside lifted over the snout.
brow = region([FRONT, flip(BROW_L), flip(BROW_R)])
shell.hull(slab(brow, 1, thick=0.06, base=lambda x, z: 0.17, lift=(0.004, 0.0, 0.0)), mat=tone(0.17))
# The cheeks: lumpier dark rocks either side of the snout, down to the surface.
for ln, seed, lift in ((BROW_L, 2, (-0.008, 0.03, 0.0)), (BROW_R, 3, (-0.012, -0.02, 0.02))):
    shell.hull(slab(region([FRONT, ln]), seed, thick=0.06, base=lambda x, z: 0.015, lift=lift), mat=tone(0.13))

# The rim: big lobes of rock round the flanks and the back, under the back plates' edges,
# so the outline is a lumpy stone and the plates overlap it like shingles. (angle from ahead,
# clockwise from above; radii along the rim, up, outward)
for deg, r, seed in ((68, (0.15, 0.12, 0.085), 14), (112, (0.16, 0.13, 0.09), 15), (155, (0.15, 0.115, 0.085), 16),
                     (200, (0.17, 0.12, 0.09), 17), (246, (0.15, 0.13, 0.085), 18), (290, (0.14, 0.12, 0.085), 19)):
    t = math.radians(deg)
    out = Vector((math.sin(t), 0.0, -math.cos(t)))
    k = 0.8 * edge(t)
    c = (k * RX * out.x, 0.07, ZC + k * RZ * out.z)
    shell.lump(c, r, mat=tone(0.1), noise=0.24, seed=seed, floor=0.008, subdivisions=2, forward=tuple(-out),
               up=(out.x * 0.35, 1, out.z * 0.35))

# The weak spot: a glowing sac in the mantle under the hole where the back plates meet.
WEAK = (0.0, 0.26, 0.2)
shell.blob(WEAK, (0.06, 0.035, 0.065), mat=CORE, noise=0.08, seed=5)
a.socket("SOCKET_weak", shell, at=WEAK)


def surface_z(part, x, y, start=-2.0):
    """Where a ray from (x, y, start) along +Z first meets the part's geometry so far."""
    o, d = Vector((x, y, start)), Vector((0.0, 0.0, 1.0))
    best = None
    for pr in part.prims:
        for idx, _, _ in pr.faces:
            v = [pr.verts[i] for i in idx]
            for k in range(1, len(v) - 1):
                e1, e2 = v[k] - v[0], v[k + 1] - v[0]
                h = d.cross(e2)
                det = e1.dot(h)
                if abs(det) < 1e-12:
                    continue
                s_ = o - v[0]
                u = s_.dot(h) / det
                q = s_.cross(e1)
                w = d.dot(q) / det
                if u < 0 or w < 0 or u + w > 1:
                    continue
                t = e2.dot(q) / det
                if t > 0 and (best is None or t < best):
                    best = t
    return None if best is None else start + best


# Eyes: pale-green pits in the brow's front face, each a dark rim round a glowing bead.
EYES = [(-0.032, 0.236, 0.015), (0.038, 0.228, 0.013), (-0.064, 0.205, 0.011), (0.066, 0.196, 0.011),
        (0.004, 0.198, 0.009)]
for x, y, r in EYES:
    z = surface_z(shell, x, y)
    shell.blob((x, y, z + 0.006), (r + 0.008, r + 0.008, 0.012), mat=DARK, segments=8)
    shell.blob((x, y, z - 0.001), (r, r, r * 0.6), mat=GLOW)
a.socket("SOCKET_eye", shell, at=(0.0, 0.22, -0.36))

# ---------------------------------------------------------------- the back plates
# Built in place, then turned about their hinges by LIMPET_OPEN.


def plate(name, hinge, outward, outline, seed, lift):
    p = a.part(name, shell, pivot=hinge, forward=outward, up=(0, 1, 0))
    axis = Vector((0.0, 1.0, 0.0)).cross(-Vector(outward)).normalized()     # local X, in world
    rot = Matrix.Rotation(OPEN, 3, axis)
    h = Vector(hinge)
    pts = [tuple(h + rot @ (Vector(q) - h)) for q in slab(outline, seed, thick=0.06, lift=lift)]
    p.hull(pts, mat=tone(0.11))
    return p


left = region([flip(FRONT), flip(SPINE), flip(REAR)], 0.8)
right = region([flip(FRONT), SPINE, flip(REAR)], 0.8)
rear = region([REAR], 0.8)
plate("plate_l", (-0.008, dome(-0.008, 0.08) - 0.02, 0.08), (-1, 0, 0), left, 31, (0.014, 0.0, -0.03))
plate("plate_r", (0.008, dome(0.008, 0.08) - 0.02, 0.08), (1, 0, 0), right, 32, (0.008, 0.02, -0.02))
plate("plate_rear", (0.0, dome(0.0, 0.215) - 0.02, 0.215), (0, 0, 1), rear, 33, (-0.006, 0.0, 0.0))

# ---------------------------------------------------------------- skirt: eight foot lobes
for k in range(8):
    th = math.radians(45 * k)
    d = Vector((math.sin(th), 0.0, -math.cos(th)))
    inner = Vector((0.25 * d.x, 0.025, 0.29 * d.z))
    sk = a.part(f"skirt_{k}", shell, pivot=tuple(inner), forward=tuple(d), up=(0, 1, 0))
    # A broad, thin tongue tucked under the rock, drooping to the surface at its wavy tip;
    # neighbours overlap into one frill.
    reach = 0.085 if k != 0 else 0.06
    c = inner + d * (reach * 0.8)
    c.y = 0.03
    sk.blob(tuple(c), (0.11, 0.021, reach), mat=FLESH, forward=(d.x, -0.12, d.z), up=(0, 1, 0), noise=0.22,
            seed=40 + k, segments=12)

# ---------------------------------------------------------------- snout
# A siphon from under the brow: flesh outside, flared lips, a dark throat.
ROOT = (0.0, 0.11, -0.29)
snout = a.part("snout", shell, pivot=ROOT, forward=(0, 0, -1), up=(0, 1, 0))
N = 10


def ring(z, y, r, lobes=0.0, seed=0):
    """A wobbly section of the siphon at z: radius r (a little flatter than wide), its edge
    pushed in and out by `lobes` (a fraction) in five soft lobes."""
    out = []
    for k in range(N):
        t = 2 * math.pi * k / N
        w = 1.0 + lobes * math.cos(5 * t + seed)
        out.append((r * w * math.cos(t), y + 0.88 * r * w * math.sin(t), z))
    return out


# Out from under the brow, dipping a little and lifting to the mouth: the lips flare in soft
# lobes round a dark throat.
snout.loft([ring(-0.24, 0.10, 0.06), ring(-0.30, 0.098, 0.054, 0.05, 1), ring(-0.365, 0.106, 0.042, 0.06, 2),
            ring(-0.402, 0.116, 0.046, 0.14, 3), ring(-0.427, 0.12, 0.056, 0.2, 3), ring(-0.433, 0.12, 0.034, 0.14, 3),
            ring(-0.41, 0.12, 0.02), [(0.0, 0.12, -0.375)]],
           mat=lambda c: FLESH if math.hypot(c[0], c[1] - 0.12) > 0.036 or c[2] < -0.429 else DARK, crease=70)
a.socket("SOCKET_muzzle", snout, at=(0.0, 0.12, -0.432), forward=(0, 0, -1))

# ---------------------------------------------------------------- collider
# One hull over the dome (without its lumps) and its footprint.
col = [(0.0, Y0 + H + 0.02, ZC)]
for k in range(16):
    t = 2 * math.pi * k / 16
    for r, y in ((1.0, 0.0), (1.0, Y0 + 0.02), (0.75, None), (0.45, None)):
        x, z = r * RX * math.sin(t), ZC - r * RZ * math.cos(t)
        col.append((x, Y0 + H * (1 - r * r) ** 0.62 + 0.02 if y is None else y, z))
a.collider("COL_body", points=col)
a.export()
