"""Primitive builders. Each primitive is a closed shell in Bevy world coordinates.

Every face is tagged flat or curved by the primitive that made it, never by a guess:
  - flat faces (box sides, prism caps and sides, lathe end caps and radial steps) get all
    their edges marked hard, so they always shade exactly flat;
  - curved faces (lathe bands, smooth prism sides) are smooth across edges sharper than
    nothing, and hard across edges sharper than the primitive's smooth_angle, except
    a lathe's seams made with smooth_seams, which are always smooth.
Primitives are never welded to each other, so their edges never mix.
"""
import math

from mathutils import Vector

from .space import axis_index, vec3


class Prim:
    __slots__ = ("kind", "verts", "faces", "bevel", "smooth_angle", "seams", "facets")
    # smooth_angle is in radians here (Part converts).

    def __init__(self, kind, bevel, smooth_angle):
        self.kind = kind
        self.verts = []        # Bevy world coordinates (Vector)
        self.faces = []        # (vertex indices, flat?, material role)
        self.bevel = bevel
        self.smooth_angle = smooth_angle
        self.seams = []        # vertex index pairs: edges that stay smooth whatever their angle
        self.facets = []       # vertex index pairs: a deliberate polygon's seams; hard by angle, never bevelled

    def v(self, p):
        self.verts.append(Vector(p))
        return len(self.verts) - 1

    def face(self, idx, flat, mat):
        self.faces.append((tuple(idx), bool(flat), mat))


def _others(a):
    return [i for i in range(3) if i != a]


def _check_2d(points, what):
    pts = []
    for p in points:
        try:
            u, v = (float(c) for c in p)
        except (TypeError, ValueError):
            raise ValueError(f"{what}: each point must be two numbers, got {p!r}") from None
        pts.append((u, v))
    if len(pts) < 3:
        raise ValueError(f"{what}: needs at least 3 points")
    for i in range(len(pts)):
        a, b = pts[i], pts[(i + 1) % len(pts)]
        if math.dist(a, b) < 1e-6:
            raise ValueError(f"{what}: points {i} and {(i + 1) % len(pts)} coincide ({a}); remove the duplicate"
                             " (don't repeat the first point at the end)")
    n = len(pts)

    def crosses(p1, p2, q1, q2):
        def side(a, b, c):
            return (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
        return (side(p1, p2, q1) * side(p1, p2, q2) < 0) and (side(q1, q2, p1) * side(q1, q2, p2) < 0)

    for i in range(n):
        for j in range(i + 2, n):
            if i == 0 and j == n - 1:
                continue
            if crosses(pts[i], pts[(i + 1) % n], pts[j], pts[(j + 1) % n]):
                raise ValueError(f"{what}: edges {i}-{i + 1} and {j}-{(j + 1) % n} cross; the outline must not"
                                 " self-intersect")
    area = 0.5 * sum(pts[i][0] * pts[(i + 1) % len(pts)][1] - pts[(i + 1) % len(pts)][0] * pts[i][1]
                     for i in range(len(pts)))
    if abs(area) < 1e-9:
        raise ValueError(f"{what}: the polygon has no area")
    return pts


def prism(points, plane, span, mat, smooth, bevel, smooth_angle):
    if not (isinstance(plane, str) and len(plane) == 2 and plane[0] != plane[1]):
        raise ValueError(f"prism plane must be two different axis letters like 'zy', got {plane!r}")
    u, v = axis_index(plane[0], "plane"), axis_index(plane[1], "plane")
    w = 3 - u - v
    pts = _check_2d(points, "prism points")
    lo, hi = sorted(float(s) for s in span)
    if hi - lo < 1e-6:
        raise ValueError(f"prism span {span} has no thickness")
    pr = Prim("prism", bevel, smooth_angle)
    rings = []
    for depth in (lo, hi):
        ring = []
        for a, b in pts:
            p = [0.0, 0.0, 0.0]
            p[u], p[v], p[w] = a, b, depth
            ring.append(pr.v(p))
        rings.append(ring)
    n = len(pts)
    pr.face(rings[0], True, mat)
    pr.face(rings[1], True, mat)
    for i in range(n):
        j = (i + 1) % n
        pr.face((rings[0][i], rings[0][j], rings[1][j], rings[1][i]), not smooth, mat)
    return pr


def box(lo, hi, mat, bevel):
    lo, hi = vec3(lo, "box corner"), vec3(hi, "box corner")
    a = [min(lo[k], hi[k]) for k in range(3)]
    b = [max(lo[k], hi[k]) for k in range(3)]
    if min(b[k] - a[k] for k in range(3)) < 1e-6:
        raise ValueError(f"box {tuple(lo)}..{tuple(hi)} is flat in some axis")
    return prism([(a[0], a[1]), (b[0], a[1]), (b[0], b[1]), (a[0], b[1])], "xy", (a[2], b[2]), mat,
                 False, bevel, 0.0)


def frame(lo, hi, wall, axis, mat, bevel):
    """Rectangular sleeve: outer box lo..hi, open along `axis`, walls `wall` thick."""
    a = axis_index(axis)
    o1, o2 = _others(a)
    lo, hi = vec3(lo, "frame corner"), vec3(hi, "frame corner")
    L = [min(lo[k], hi[k]) for k in range(3)]
    H = [max(lo[k], hi[k]) for k in range(3)]
    wall = float(wall)
    if min(H[o1] - L[o1], H[o2] - L[o2]) <= 2 * wall + 1e-6:
        raise ValueError(f"frame walls ({wall}) leave no opening in {tuple(L)}..{tuple(H)}")
    pr = Prim("frame", bevel, 0.0)

    def ring(t, inset):
        out = []
        for c1, c2 in ((L[o1] + inset, L[o2] + inset), (H[o1] - inset, L[o2] + inset),
                       (H[o1] - inset, H[o2] - inset), (L[o1] + inset, H[o2] - inset)):
            p = [0.0, 0.0, 0.0]
            p[a], p[o1], p[o2] = t, c1, c2
            out.append(pr.v(p))
        return out

    ob, ot, ib, it = ring(L[a], 0), ring(H[a], 0), ring(L[a], wall), ring(H[a], wall)
    for i in range(4):
        j = (i + 1) % 4
        pr.face((ob[i], ob[j], ot[j], ot[i]), True, mat)   # outer wall
        pr.face((it[i], it[j], ib[j], ib[i]), True, mat)   # inner wall
        pr.face((ot[i], ot[j], it[j], it[i]), True, mat)   # lip at hi
        pr.face((ib[i], ib[j], ob[j], ob[i]), True, mat)   # lip at lo
    return pr


def auto_segments(radius, tol, minimum=8):
    """Fewest segments (even, at least `minimum`) for which a circle of `radius` is within `tol` of
    its polygon (the sagitta r(1 - cos(pi/n)) <= tol), so the silhouette stays round."""
    if radius <= tol:
        return minimum
    n = math.ceil(math.pi / math.acos(1.0 - tol / radius))
    return max(minimum, n + (n % 2))


def lathe(profile, axis, center, mats, segments, bevel, smooth_angle, smooth_seams=False):
    """Surface of revolution. profile: [(radius, t)] with t the coordinate along `axis`.

    Open ends are capped flat. A radius-0 end at the same t as its neighbour is a flat
    cap; at a different t it is a cone tip (curved). Segments with constant t (radial
    steps) are flat; the rest (cylinders, cones) are curved.
    smooth_seams: the edges running along the profile between neighbouring segments of a
    curved band stay smooth whatever their angle (a round surface); otherwise they are
    hard where the facet angle 360/segments reaches smooth_angle (a faceted one), and
    those facet seams are never bevelled: the hard edge itself is the low-poly look, and
    a bevel along every facet would cost more than the rest of the lathe.
    """
    a = axis_index(axis)
    o1, o2 = _others(a)
    try:
        c1, c2 = (float(c) for c in center)
    except (TypeError, ValueError):
        raise ValueError(f"lathe center must be two numbers (the other two axes, in xyz order), got {center!r}") from None
    prof = []
    for p in profile:
        try:
            r, t = (float(c) for c in p)
        except (TypeError, ValueError):
            raise ValueError(f"lathe profile points must be (radius, t), got {p!r}") from None
        if r < 0:
            raise ValueError(f"lathe radius {r} is negative")
        prof.append((r, t))
    if len(prof) < 2:
        raise ValueError("lathe needs at least 2 profile points")
    if len(mats) != len(prof) - 1:
        raise ValueError(f"lathe mats: need one per profile segment ({len(prof) - 1}), got {len(mats)}")
    mats = list(mats)
    for i in range(len(prof) - 1):
        if math.dist(prof[i], prof[i + 1]) < 1e-6:
            raise ValueError(f"lathe profile points {i} and {i + 1} coincide")
    if any(r == 0 for r, _ in prof[1:-1]):
        raise ValueError("lathe radius 0 is only allowed at the ends of the profile")
    if int(segments) < 3:
        raise ValueError("lathe needs at least 3 segments")
    segments = int(segments)
    # A pole at the same t as its neighbour is just a flat cap: use one n-gon for it.
    cap_start = cap_end = None
    if prof[0][0] == 0 and abs(prof[0][1] - prof[1][1]) < 1e-9:
        prof, cap_start = prof[1:], mats.pop(0)
    elif prof[0][0] > 0:
        cap_start = mats[0]
    if prof[-1][0] == 0 and abs(prof[-1][1] - prof[-2][1]) < 1e-9:
        prof, cap_end = prof[:-1], mats.pop()
    elif prof[-1][0] > 0:
        cap_end = mats[-1]
    if len(prof) < 2:
        raise ValueError("lathe profile is only a flat disc; use cylinder() or add a second radius")

    pr = Prim("lathe", bevel, smooth_angle)
    rings = []
    for r, t in prof:
        if r == 0:
            p = [0.0, 0.0, 0.0]
            p[a], p[o1], p[o2] = t, c1, c2
            rings.append([pr.v(p)])
            continue
        ring = []
        for k in range(segments):
            th = 2 * math.pi * k / segments
            p = [0.0, 0.0, 0.0]
            p[a], p[o1], p[o2] = t, c1 + r * math.cos(th), c2 + r * math.sin(th)
            ring.append(pr.v(p))
        rings.append(ring)
    for i in range(len(rings) - 1):
        ra, rb = rings[i], rings[i + 1]
        flat = abs(prof[i][1] - prof[i + 1][1]) < 1e-9
        for k in range(segments):
            kn = (k + 1) % segments
            if len(ra) == 1:
                pr.face((ra[0], rb[kn], rb[k]), flat, mats[i])
            elif len(rb) == 1:
                pr.face((ra[k], ra[kn], rb[0]), flat, mats[i])
            else:
                pr.face((ra[k], ra[kn], rb[kn], rb[k]), flat, mats[i])
            if not flat:
                edge = (ra[k] if len(ra) > 1 else ra[0], rb[k] if len(rb) > 1 else rb[0])
                (pr.seams if smooth_seams else pr.facets).append(edge)
    if cap_start is not None:
        pr.face(rings[0], True, cap_start)
    if cap_end is not None:
        pr.face(rings[-1], True, cap_end)
    return pr


def hull(points, mat, bevel):
    """Convex hull of `points` (Bevy world): a closed shell of flat faces. Coplanar hull
    triangles are merged into one face, so each side of the hull is one flat plane. For
    wedges, sloped plates and chamfered blocks that boxes and prisms can't tilt into."""
    import bmesh

    pts = [vec3(p, "hull point") for p in points]
    if len(pts) < 4:
        raise ValueError("hull needs at least 4 points")
    bm = bmesh.new()
    try:
        for p in pts:
            bm.verts.new(p)
        res = bmesh.ops.convex_hull(bm, input=list(bm.verts), use_existing_faces=False)
        drop = list({g for g in res["geom_interior"] + res["geom_unused"] if isinstance(g, bmesh.types.BMVert)})
        bmesh.ops.delete(bm, geom=drop, context="VERTS")
        bmesh.ops.delete(bm, geom=[v for v in bm.verts if not v.link_faces], context="VERTS")
        if len(bm.faces) < 4:
            raise ValueError(f"hull of {len(pts)} points is flat or collinear; it needs volume")
        bmesh.ops.dissolve_limit(bm, angle_limit=math.radians(0.05), verts=list(bm.verts), edges=list(bm.edges))
        pr = Prim("hull", bevel, 0.0)
        index = {}
        for v in bm.verts:
            index[v] = pr.v(v.co)
        for f in bm.faces:
            pr.face([index[v] for v in f.verts], True, _role(mat, f.calc_center_median()))
    finally:
        bm.free()
    return pr


# ---------------------------------------------------------------- organic primitives
# Flora and fauna have no straight lines. These build closed shells from rings of points
# (loft), tubes along a path (sweep), noisy ellipsoids (blob) and asteroid-style rocks
# (lump). Their shading follows the `smooth` flag, never an angle guess:
#   - smooth=True: the surface is one smooth skin. Every edge between its own band faces
#     stays smooth whatever the angle (a low-poly organic surface shows its facets in the
#     silhouette, not in the shading), unless `crease` (degrees) is given: then edges at
#     least that sharp go hard.
#   - smooth=False: faceted. Every band quad is split into two flat triangles, so the
#     surface shades exactly flat per triangle (the asteroid look).
# A material can be a role, or a function of a face's centre (Bevy world point) -> role,
# for markings and two-tone rocks.


def _role(mat, centre):
    return mat(tuple(centre)) if callable(mat) else mat


def _centre(pts):
    n = len(pts)
    return Vector((sum(p[0] for p in pts) / n, sum(p[1] for p in pts) / n, sum(p[2] for p in pts) / n))


def _planar(pts, tol=1e-6):
    """True when the closed ring `pts` lies on one plane (Newell normal)."""
    nrm = Vector((0.0, 0.0, 0.0))
    for i in range(len(pts)):
        a, b = pts[i], pts[(i + 1) % len(pts)]
        nrm.x += (a.y - b.y) * (a.z + b.z)
        nrm.y += (a.z - b.z) * (a.x + b.x)
        nrm.z += (a.x - b.x) * (a.y + b.y)
    if nrm.length < 1e-12:
        return False
    nrm.normalize()
    c = _centre(pts)
    return all(abs((p - c).dot(nrm)) < tol for p in pts)


def loft(rings, mat, smooth=True, crease=None):
    """A closed shell through `rings`: each ring is a closed loop of points (Bevy world),
    all with the same count; the first and last ring may be a single point (a pole: a
    rounded or pointed end). Neighbouring rings are joined by quads. An end ring with more
    than one point is capped: one flat face if it is planar, else a fan to its centre."""
    rs = []
    for i, r in enumerate(rings):
        pts = [vec3(p, "loft point") for p in r]
        if not pts:
            raise ValueError(f"loft ring {i} is empty")
        rs.append(pts)
    if len(rs) < 2:
        raise ValueError("loft needs at least 2 rings")
    n = max(len(r) for r in rs)
    if n < 3:
        raise ValueError("loft rings need at least 3 points")
    for i, r in enumerate(rs):
        if len(r) == 1:
            if 0 < i < len(rs) - 1:
                raise ValueError(f"loft ring {i}: a single-point ring (a pole) is only allowed at the ends")
        elif len(r) != n:
            raise ValueError(f"loft ring {i} has {len(r)} points, the others {n}")
    if len(rs) == 2 and len(rs[0]) == 1 and len(rs[1]) == 1:
        raise ValueError("loft: two poles and nothing between them")

    pr = Prim("loft", 0.0, math.radians(crease) if crease is not None else math.pi)
    idx = [[pr.v(p) for p in r] for r in rs]
    band_edges = []

    def quad(a, b, c, d):
        role = _role(mat, _centre([pr.verts[k] for k in (a, b, c, d)]))
        if smooth:
            pr.face((a, b, c, d), False, role)
        else:                                   # two flat triangles
            pr.face((a, b, c), True, role)
            pr.face((a, c, d), True, role)

    def tri(a, b, c):
        pr.face((a, b, c), not smooth, _role(mat, _centre([pr.verts[k] for k in (a, b, c)])))

    last = len(rs) - 1
    for i in range(last):
        ra, rb = idx[i], idx[i + 1]
        for k in range(n):
            kn = (k + 1) % n
            if len(ra) == 1:
                tri(ra[0], rb[kn], rb[k])
                band_edges.append((ra[0], rb[k]))
            elif len(rb) == 1:
                tri(ra[k], ra[kn], rb[0])
                band_edges.append((ra[k], rb[0]))
            else:
                quad(ra[k], ra[kn], rb[kn], rb[k])
                band_edges.append((ra[k], rb[k]))
            if len(rb) > 1 and i + 1 < last:    # a ring edge between two bands
                band_edges.append((rb[k], rb[kn]))
    for ring in (idx[0], idx[-1]):
        if len(ring) == 1:
            continue
        pts = [pr.verts[k] for k in ring]
        if _planar(pts):
            pr.face(ring, True, _role(mat, _centre(pts)))
        else:
            c = pr.v(_centre(pts))
            for k in range(n):
                tri(ring[k], ring[(k + 1) % n], c)
                band_edges.append((ring[k], c))
                band_edges.append((ring[k], ring[(k + 1) % n]))
    if smooth and crease is None:
        pr.seams = band_edges
    return pr


def _basis(forward, up):
    """Rotation (3x3) with local -Z along forward and +Y toward up, as space.frame does;
    identity when both are None."""
    from .space import frame
    return frame((0.0, 0.0, 0.0), forward, up).to_3x3()


def fbm(p, seed, octaves=3):
    """Fractal Perlin noise in about [-1, 1] at point p; `seed` picks a far-off region of the
    field, so the same seed always gives the same shape."""
    import random

    from mathutils import noise
    rnd = random.Random(seed)
    off = Vector((rnd.uniform(-500, 500), rnd.uniform(-500, 500), rnd.uniform(-500, 500)))
    total, amp, freq, norm = 0.0, 1.0, 1.0, 0.0
    for _ in range(octaves):
        total += amp * noise.noise(Vector(p) * freq + off, noise_basis="PERLIN_ORIGINAL")
        norm += amp
        amp *= 0.5
        freq *= 2.0
    return total / norm


def blob(center, radii, mat, segments, forward=None, up=None, noise=0.0, seed=0, frequency=1.5, smooth=True,
         crease=None):
    """A closed ellipsoid: a UV sphere with its poles on local Y, radii (rx, ry, rz) in its
    local frame, its radius displaced by fractal noise (a fraction: 0.2 = +/-20%)."""
    c = vec3(center, "blob centre")
    rad = vec3(radii, "blob radii")
    if min(rad) <= 0:
        raise ValueError(f"blob radii {tuple(rad)} must be positive")
    seg = int(segments)
    if seg < 4:
        raise ValueError("blob needs at least 4 segments")
    rot = _basis(forward, up)
    nrings = max(2, seg // 2)
    rings = []
    for i in range(nrings + 1):
        phi = math.pi * i / nrings                    # 0 at -Y (bottom), pi at +Y (top)
        y, s = -math.cos(phi), math.sin(phi)
        if i in (0, nrings):
            dirs = [Vector((0.0, y, 0.0))]
        else:
            dirs = [Vector((s * math.cos(2 * math.pi * k / seg), y, -s * math.sin(2 * math.pi * k / seg)))
                    for k in range(seg)]
        ring = []
        for d in dirs:
            f = 1.0 + (noise * fbm(d * frequency, seed) if noise else 0.0)
            ring.append(c + rot @ Vector((d.x * rad.x * f, d.y * rad.y * f, d.z * rad.z * f)))
        rings.append(ring)
    return loft(rings, mat, smooth=smooth, crease=crease)


def sweep(path, radius, mat, sides, up=None, smooth=True, crease=None):
    """A tube along `path` (Bevy world points). `radius` is one number, or a list with one
    entry per path point; an entry is r, or (r_side, r_up) for an oval section. A zero
    radius at an end makes it pointed. Sections are carried along the path by parallel
    transport, starting with r_up toward `up` (default world +Y)."""
    pts = [vec3(p, "sweep point") for p in path]
    if len(pts) < 2:
        raise ValueError("sweep needs at least 2 path points")
    for i in range(len(pts) - 1):
        if (pts[i + 1] - pts[i]).length < 1e-6:
            raise ValueError(f"sweep path points {i} and {i + 1} coincide")
    if isinstance(radius, (int, float)):
        rads = [radius] * len(pts)
    elif isinstance(radius, list):
        rads = list(radius)
    else:
        raise ValueError(f"sweep radius: a number, or a list with one entry per path point (an oval section"
                         f" everywhere is [(r_side, r_up)] * n), got {radius!r}")
        if len(rads) != len(pts):
            raise ValueError(f"sweep radius: one per path point ({len(pts)}), got {len(rads)}")
    out = []
    for r in rads:
        if isinstance(r, (int, float)):
            out.append((float(r), float(r)))
        else:
            a, b = (float(v) for v in r)
            out.append((a, b))
    if any(a < 0 or b < 0 for a, b in out):
        raise ValueError("sweep radii must not be negative")
    if any(a == 0 or b == 0 for a, b in out[1:-1]):
        raise ValueError("sweep radius 0 is only allowed at the ends")
    tang = []
    for i in range(len(pts)):
        t = pts[min(len(pts) - 1, i + 1)] - pts[max(0, i - 1)]
        if t.length < 1e-9:
            raise ValueError(f"sweep path doubles back on itself at point {i}")
        tang.append(t.normalized())
    ref = vec3(up, "sweep up") if up is not None else Vector((0.0, 1.0, 0.0))
    if ref.length < 1e-9:
        raise ValueError("sweep up must be non-zero")
    if abs(ref.normalized().dot(tang[0])) > 0.95:
        raise ValueError(f"sweep up {tuple(ref)} is along the path's start; pass another up=")
    nrm = (ref - tang[0] * ref.dot(tang[0])).normalized()
    rings = []
    for i, p in enumerate(pts):
        nrm = nrm - tang[i] * nrm.dot(tang[i])        # parallel transport
        if nrm.length < 1e-9:
            raise ValueError("sweep path turns back on itself")
        nrm.normalize()
        side = tang[i].cross(nrm).normalized()
        rs, ru = out[i]
        if rs == 0 or ru == 0:
            rings.append([p.copy()])
            continue
        rings.append([p + side * (rs * math.cos(2 * math.pi * k / sides)) + nrm * (ru * math.sin(2 * math.pi * k / sides))
                      for k in range(int(sides))])
    return loft(rings, mat, smooth=smooth, crease=crease)


def ico_points(subdivisions):
    """Unit icosphere vertex directions."""
    import bmesh
    bm = bmesh.new()
    try:
        bmesh.ops.create_icosphere(bm, subdivisions=int(subdivisions), radius=1.0)
        return [v.co.normalized() for v in bm.verts]
    finally:
        bm.free()


def lump(center, radii, mat, bevel, forward=None, up=None, noise=0.25, seed=0, frequency=1.3, subdivisions=1,
         floor=None):
    """A rock: the convex hull of a noisy ellipsoid's icosphere points, the way the game
    builds its asteroids (src/sim/asteroids.rs). Every face flat. floor: points below
    y = floor are raised to it (a flat base sitting on a surface)."""
    c = vec3(center, "lump centre")
    rad = vec3(radii, "lump radii")
    if min(rad) <= 0:
        raise ValueError(f"lump radii {tuple(rad)} must be positive")
    rot = _basis(forward, up)
    pts = []
    for d in ico_points(subdivisions):
        f = 1.0 + noise * fbm(d * frequency, seed)
        p = c + rot @ Vector((d.x * rad.x * f, d.y * rad.y * f, d.z * rad.z * f))
        if floor is not None and p.y < floor:
            p.y = float(floor)
        pts.append(p)
    pr = hull(pts, mat, bevel)
    pr.kind = "lump"
    return pr
