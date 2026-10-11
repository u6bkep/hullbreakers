"""Swarmer: small, fast, fragile pack drone. Bevy space: +Y up, -Z forward, +X right.
Origin = the centre of the body. Insect read: a low segmented bone-white carapace (brow,
thorax, abdomen, tail) over a black core, a cluster of cyan eyes under an angry V brow with
dark mandibles either side, four octagonal thruster pods on splayed black struts (front pair
low, rear pair high, so the front view is an X), and a stubby twin gun turret under the belly.

Moving parts: thruster_* swivel on their trunnion (pivot = pod centre, on the strut knuckle's
axis); gun turns on its ring (pivot = the turret ring under the belly)."""
import hb

a = hb.Asset(__file__)
WHITE, DARK, BLACK, GLOW, BARE = "paint_primary", "metal_dark", "polymer", "emissive", "metal_bare"


def sym(*pts):
    """Points plus their mirror images across x = 0 (points on x = 0 once)."""
    out = []
    for x, y, z in pts:
        out.append((x, y, z))
        if x:
            out.append((-x, y, z))
    return out


def frustum(ring, z0, z1, k, shift=(0.0, 0.0)):
    """Hull points of a plate along z: the convex outline `ring` (x, y) at z0, and the same
    outline scaled by k about its centre and moved by `shift` at z1. Similar outlines keep
    every side face planar, so the hull has clean corners for the bevel."""
    cx = sum(p[0] for p in ring) / len(ring)
    cy = sum(p[1] for p in ring) / len(ring)
    return ([(x, y, z0) for x, y in ring] +
            [(cx + k * (x - cx) + shift[0], cy + k * (y - cy) + shift[1], z1) for x, y in ring])


# ---------------------------------------------------------------- body
body = a.part("body", bevel_segments=1)
# Black core: a spindle along z that the plates sit on; it shows in the seams and under the flanks.
body.lathe([(0.045, -0.190), (0.090, -0.165), (0.105, -0.110), (0.105, 0.100), (0.075, 0.190), (0, 0.220)],
           axis="z", center=(0, 0), mat=DARK, segments=12)

# Carapace: separate plates with dark seams between them, each one cross-section lofted along z.
# Peaked thorax, then a tapering abdomen and tail.
SHELL = [(-0.145, -0.010), (0.145, -0.010), (0.145, 0.060), (0.080, 0.125), (0, 0.140), (-0.080, 0.125),
         (-0.145, 0.060)]


def shell(k):
    """SHELL scaled by k about the middle of its belly line."""
    return [(k * x, -0.010 + k * (y + 0.010)) for x, y in SHELL]


body.hull(frustum(shell(0.88), -0.100, 0.020, 1.0 / 0.88, (0, 0.006)), mat=WHITE)      # thorax
body.hull(frustum(shell(0.95), 0.034, 0.140, 0.78 / 0.95, (0, -0.010)), mat=WHITE)     # abdomen
body.hull(frustum(shell(0.74), 0.154, 0.250, 0.42 / 0.74, (0, -0.016)), mat=WHITE)     # tail

# Head: a brow with an angry V front edge, a cheek plate either side of the face, dark mandibles.
body.hull(frustum([(-0.118, 0.030), (0, 0.000), (0.118, 0.030), (0.075, 0.060), (-0.075, 0.060)],
                  -0.250, -0.110, 1.22, (0, 0.050)), mat=WHITE)
for s in (-1, 1):
    body.hull(frustum([(s * 0.093, -0.095), (s * 0.130, -0.080), (s * 0.130, 0.030), (s * 0.093, 0.040)],
                      -0.112, -0.235, 0.72, (-s * 0.008, -0.010)), mat=WHITE)
    body.hull(frustum([(s * 0.088, -0.075), (s * 0.132, -0.065), (s * 0.128, -0.118), (s * 0.094, -0.128)],
                      -0.170, -0.310, 0.45, (-s * 0.045, -0.004)), mat=DARK)
# Face plate, recessed between brow and cheeks.
body.box((-0.095, -0.108, -0.205), (0.095, 0.050, -0.110), mat=DARK)

# Eye cluster: one big eye and four small ones, each a dark rim with a glowing dome.
FACE = -0.205
EYES = [(0.0, -0.040, 0.030), (-0.058, -0.006, 0.016), (0.058, -0.006, 0.016), (-0.060, -0.074, 0.016),
        (0.060, -0.074, 0.016)]
for x, y, r in EYES:
    rim = r + 0.008
    body.lathe([(rim, FACE + 0.010), (rim, FACE - 0.014), (r, FACE - 0.014), (0.8 * r, FACE - 0.014 - 0.5 * r),
                (0, FACE - 0.014 - 0.75 * r)], axis="z", center=(x, y), mats=[DARK, DARK, GLOW, GLOW],
               segments=12, bevel=0.003)
a.socket("SOCKET_eye", body, at=(0, -0.040, FACE - 0.014 - 0.75 * 0.030))

# Gun mount ring under the belly.
GUN = (0.0, -0.100, -0.050)
body.lathe([(0.075, -0.080), (0.075, -0.102)], axis="y", center=(GUN[0], GUN[2]), mat=BLACK, segments=16)

# ---------------------------------------------------------------- thrusters
# name, pod centre (pivot, on the strut knuckle's axis), strut root on the body (x, z)
PODS = [("fl", (-0.310, -0.060, -0.080), (-0.090, -0.050)),
        ("fr", (0.310, -0.060, -0.080), (0.090, -0.050)),
        ("rl", (-0.300, 0.060, 0.130), (-0.090, 0.090)),
        ("rr", (0.300, 0.060, 0.130), (0.090, 0.090))]
POD_R = 0.050
for tag, (px, py, pz), (rx, rz) in PODS:
    s = 1 if px > 0 else -1
    ax = abs(px)
    knuckle = ax - POD_R - 0.030          # knuckle centre, |x|
    # Strut: a tapered arm (top view) from the core to the knuckle, a shoulder joint at the body.
    body.prism([(s * abs(rx), rz - 0.038), (s * abs(rx), rz + 0.038), (s * knuckle, pz + 0.020),
                (s * knuckle, pz - 0.020)], plane="xz", span=(py - 0.020, py + 0.020), mat=DARK)
    body.cylinder("x", (py, rz), 0.036, (s * 0.110, s * 0.165), mat=BLACK, segments=12)
    body.cylinder("x", (py, pz), 0.030, (s * (knuckle - 0.022), s * (knuckle + 0.022)), mat=BLACK, segments=12)

    pod = a.part(f"thruster_{tag}", body, pivot=(px, py, pz), bevel=0.005, bevel_segments=1)
    # Cowl: pointed white nose, dark rear housing; octagonal to match the faceted shell.
    pod.lathe([(0, pz - 0.088), (0.030, pz - 0.064), (POD_R, pz - 0.036), (POD_R, pz + 0.004), (POD_R, pz + 0.044),
               (0.042, pz + 0.054)], axis="z", center=(px, py), mats=[WHITE, WHITE, WHITE, DARK, DARK],
              segments=8)
    # Nozzle bell: dark flare with a dish, a glowing core at the bottom of the dish.
    t0, t1 = pz + 0.040, pz + 0.100
    pod.lathe([(0.034, t0), (0.047, t1), (0.038, t1), (0.029, t1 - 0.020), (0, t1 - 0.020)], axis="z",
              center=(px, py), mat=DARK, segments=8, bevel=0.002)
    pod.cylinder("z", (px, py), 0.030, (t1 - 0.028, t1 - 0.014), mat=GLOW, segments=8, bevel=0.001)
    # Trunnion stub into the knuckle.
    pod.cylinder("x", (py, pz), 0.018, (s * (knuckle + 0.010), s * (ax - POD_R + 0.010)), mat=DARK, segments=8)
    a.socket(f"SOCKET_thrust_{tag}", pod, at=(px, py, t1 - 0.014), forward=(0, 0, 1))

# ---------------------------------------------------------------- gun
gun = a.part("gun", body, pivot=GUN, bevel_segments=1)
gun.lathe([(0.062, -0.090), (0.062, -0.115), (0.050, -0.130)], axis="y", center=(GUN[0], GUN[2]), mat=BLACK,
          segments=16)
gun.hull(frustum([(-0.062, -0.105), (0.062, -0.105), (0.056, -0.192), (-0.056, -0.192)], 0.025, -0.160, 0.82,
                 (0, 0.004)), mat=DARK)
BY, BZ0, BZ1 = -0.152, -0.145, -0.290
for tag, bx in (("l", -0.034), ("r", 0.034)):
    gun.lathe([(0.023, BZ0 + 0.010), (0.023, BZ0 - 0.060), (0.018, BZ0 - 0.060), (0.018, BZ1 + 0.014),
               (0.022, BZ1 + 0.014), (0.022, BZ1)], axis="z", center=(bx, BY),
              mats=[BARE, BARE, DARK, DARK, DARK], segments=10, bevel=0.003)
    a.socket(f"SOCKET_muzzle_{tag}", gun, at=(bx, BY, BZ1))

# ---------------------------------------------------------------- colliders
# One hull for the shell, head and gun; one flat slab through the four pods and their struts.
a.collider("COL_body", points=sym((0.15, -0.13, -0.31), (0.15, 0.12, -0.25), (0.15, -0.10, 0.25), (0.12, 0.15, 0.10),
                                  (0.06, -0.20, -0.29), (0.06, -0.20, 0.03)))
a.collider("COL_pods", points=sym((0.36, -0.11, -0.17), (0.36, 0.0, -0.17), (0.35, 0.0, 0.23), (0.35, 0.11, 0.23),
                                  (0.35, 0.11, 0.04), (0.36, -0.11, 0.02)))
a.export()
