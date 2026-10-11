"""Swarmer: small, fast, fragile pack drone. Bevy space: +Y up, -Z forward, +X right.
Origin = the centre of the body. Insect read: a low segmented bone-white carapace (brow,
thorax, abdomen) over a black core, a cluster of cyan eyes under an angry V brow with
dark mandibles either side, four octagonal thruster pods on long jointed legs (each rises from
the flank to a knee above the body line and drops to its pod, so the outline is a spider's;
front pair low, rear pair high), a bone-white dorsal crest, and a stubby twin gun turret under
the belly. Lights, loud to quiet: the cyan eyes (emissive); cyan strips along the upper legs, in
the thorax seam and in the nozzles (emissive_secondary); and the weak spot, a magenta reactor
core behind a grille in a dark cup at the very back, between the rear pods (emissive_core,
SOCKET_weak at its centre), seen from behind and above-behind, hidden from the front.

Moving parts: thruster_* swivel on their trunnion (pivot = pod centre, on the strut knuckle's
axis); gun turns on its ring (pivot = the turret ring under the belly)."""
import hb

a = hb.Asset(__file__)
WHITE, DARK, BLACK, GLOW, BARE = "paint_primary", "metal_dark", "polymer", "emissive", "metal_bare"
LIGHT, CORE = "emissive_secondary", "emissive_core"


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


def beam(a_, b_, half_w, half_h, k=1.0, lift=0.0):
    """Hull points of a straight bar from a_ to b_: a rectangle across it at each end (half_h
    toward up, half_w toward z), the far one scaled by k. `lift` moves both ends along the
    bar's up side (for a strip lying on another bar's top face)."""
    d = [b_[i] - a_[i] for i in range(3)]
    n = sum(c * c for c in d) ** 0.5
    d = [c / n for c in d]
    e2 = [-d[2] * d[0], -d[2] * d[1], 1.0 - d[2] * d[2]]            # z, made square to d
    n2 = sum(c * c for c in e2) ** 0.5
    e2 = [c / n2 for c in e2]
    e1 = [d[1] * e2[2] - d[2] * e2[1], d[2] * e2[0] - d[0] * e2[2], d[0] * e2[1] - d[1] * e2[0]]
    if e1[1] < 0:
        e1 = [-c for c in e1]
    out = []
    for p, s in ((a_, 1.0), (b_, k)):
        for u in (-1, 1):
            for v in (-1, 1):
                out.append(tuple(p[i] + e1[i] * (lift + u * half_h * s) + e2[i] * v * half_w * s for i in range(3)))
    return out


# ---------------------------------------------------------------- body
body = a.part("body", bevel_segments=1)
# Black core: a spindle along z that the plates sit on; it shows in the seams and under the flanks.
body.lathe([(0.045, -0.190), (0.090, -0.165), (0.105, -0.110), (0.105, 0.100), (0.085, 0.160), (0, 0.160)],
           axis="z", center=(0, 0), mat=DARK, segments=12)

# Carapace: separate plates with dark seams between them, each one cross-section lofted along z.
# Peaked thorax, then a tapering abdomen; the reactor cup closes the back.
SHELL = [(-0.145, -0.010), (0.145, -0.010), (0.145, 0.060), (0.080, 0.125), (0, 0.140), (-0.080, 0.125),
         (-0.145, 0.060)]


def shell(k):
    """SHELL scaled by k about the middle of its belly line."""
    return [(k * x, -0.010 + k * (y + 0.010)) for x, y in SHELL]


body.hull(frustum(shell(0.88), -0.100, 0.020, 1.0 / 0.88, (0, 0.006)), mat=WHITE)      # thorax
body.hull(frustum(shell(0.95), 0.034, 0.140, 0.78 / 0.95, (0, -0.010)), mat=WHITE)     # abdomen

# Seam lights: a glowing strip down each flank in the dark seam between thorax and abdomen.
for s in (-1, 1):
    body.box((s * 0.118, -0.002, 0.022), (s * 0.150, 0.052, 0.032), mat=LIGHT, bevel=0.002)

# Dorsal crest: a bone-white blade along the spine, rising toward the rear; it gives the side
# and front outline a peak at range.
body.prism([(-0.075, 0.112), (-0.035, 0.160), (0.055, 0.218), (0.105, 0.212), (0.130, 0.092)], plane="zy",
           span=(-0.013, 0.013), mat=WHITE)

# Reactor (the weak spot): a dark octagonal cup at the back, open to the rear, the core glowing
# in it behind a grille. The thorax and crest hide it from the front.
RC = (0.0, 0.035)                    # reactor axis (x, y)
CORE_Z, CORE_R = 0.190, 0.080
body.lathe([(0.098, 0.140), (0.112, 0.175), (0.106, 0.256), (0.088, 0.256), (0.088, 0.175), (0, 0.175)],
           axis="z", center=RC, mat=DARK, segments=8)
ring = [(CORE_R * (1 - (t / 3) ** 2) ** 0.5, CORE_Z + CORE_R * t / 3) for t in range(-3, 4)]
body.lathe([(0, CORE_Z - CORE_R)] + ring[1:-1] + [(0, CORE_Z + CORE_R)], axis="z", center=RC, mat=CORE,
           segments=12, bevel=0)
for dy in (-0.046, 0.0, 0.046):
    body.box((-0.098, RC[1] + dy - 0.008, 0.250), (0.098, RC[1] + dy + 0.008, 0.272), mat=BLACK, bevel=0.003)
body.box((-0.008, RC[1] - 0.090, 0.250), (0.008, RC[1] + 0.090, 0.266), mat=BLACK, bevel=0.003)
a.socket("SOCKET_weak", body, at=(RC[0], RC[1], CORE_Z))

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
# name, pod centre (pivot, on the strut knuckle's axis), shoulder on the body (x, y, z), knee (x, y, z)
PODS = [("fl", (-0.360, -0.070, -0.110), (-0.105, -0.020, -0.050), (-0.235, 0.120, -0.090)),
        ("fr", (0.360, -0.070, -0.110), (0.105, -0.020, -0.050), (0.235, 0.120, -0.090)),
        ("rl", (-0.350, 0.060, 0.170), (-0.105, 0.020, 0.090), (-0.225, 0.185, 0.130)),
        ("rr", (0.350, 0.060, 0.170), (0.105, 0.020, 0.090), (0.225, 0.185, 0.130))]
POD_R = 0.050
for tag, (px, py, pz), (sx, sy, sz), knee in PODS:
    s = 1 if px > 0 else -1
    ax = abs(px)
    knuckle = ax - POD_R - 0.030          # knuckle centre, |x|
    # Leg: a shoulder joint on the flank, an upper arm rising to a knee above the body line, a
    # forearm dropping to the knuckle the pod swivels in. A light strip runs along the upper arm.
    # The joints are bare octagonal pins: a bevel on something this small is under a pixel at
    # play distance and cost a third of the legs' triangles.
    body.cylinder("x", (sy, sz), 0.034, (s * 0.095, s * 0.150), mat=BLACK, segments=8, bevel=0)
    body.hull(beam((s * 0.120, sy, sz), knee, 0.020, 0.020, 0.8), mat=DARK)
    body.hull(beam((s * 0.150, sy, sz), knee, 0.006, 0.006, 1.0, lift=0.021), mat=LIGHT, bevel=0.002)
    body.cylinder("z", knee[:2], 0.024, (knee[2] - 0.022, knee[2] + 0.022), mat=BLACK, segments=8, bevel=0)
    body.hull(beam(knee, (s * knuckle, py, pz), 0.017, 0.015, 0.85), mat=DARK)
    body.cylinder("x", (py, pz), 0.030, (s * (knuckle - 0.022), s * (knuckle + 0.022)), mat=BLACK, segments=8,
                  bevel=0)

    # Unbevelled: the pods are cut octagons, their hard facets catching the light like the shell's.
    pod = a.part(f"thruster_{tag}", body, pivot=(px, py, pz), bevel=0)
    # Cowl: pointed white nose, dark rear housing; octagonal to match the faceted shell.
    pod.lathe([(0, pz - 0.088), (0.030, pz - 0.064), (POD_R, pz - 0.036), (POD_R, pz + 0.004), (POD_R, pz + 0.044),
               (0.042, pz + 0.054)], axis="z", center=(px, py), mats=[WHITE, WHITE, WHITE, DARK, DARK],
              segments=8)
    # Nozzle bell: dark flare with a dish, a glowing core at the bottom of the dish.
    t0, t1 = pz + 0.040, pz + 0.100
    pod.lathe([(0.034, t0), (0.047, t1), (0.038, t1), (0.029, t1 - 0.020), (0, t1 - 0.020)], axis="z",
              center=(px, py), mat=DARK, segments=8)
    pod.cylinder("z", (px, py), 0.030, (t1 - 0.028, t1 - 0.014), mat=LIGHT, segments=8)
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
# One hull for the shell, head, crest, reactor and gun; one slab through the legs and pods.
a.collider("COL_body", points=sym((0.15, -0.13, -0.31), (0.15, 0.12, -0.25), (0.15, -0.10, 0.14), (0.12, 0.15, 0.10),
                                  (0.11, -0.08, 0.275), (0.11, 0.15, 0.27), (0, 0.22, 0.07),
                                  (0.06, -0.20, -0.29), (0.06, -0.20, 0.03)))
a.collider("COL_pods", points=sym((0.41, -0.12, -0.20), (0.41, 0.0, -0.20), (0.40, 0.0, 0.27), (0.40, 0.11, 0.27),
                                  (0.25, 0.20, 0.13), (0.25, 0.14, -0.09), (0.41, -0.12, 0.02)))
a.export()
