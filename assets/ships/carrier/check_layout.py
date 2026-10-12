#!/usr/bin/env python3
"""Check carrier.glb against layout.json, the collision it is drawn in place of.

    python3 assets/ships/carrier/check_layout.py        (needs numpy; no Blender)

The game collides against layout.json's boxes and draws carrier.glb instead, so the model
has to be where the boxes are. Everything here is ray casting against the .glb's
triangles along a 10 cm lattice (rays run along the axes; lattice origins are offset by
odd amounts so rays don't graze edges):

1. Surfaces. Every face of every layout box is sampled (4 cm in from its edges). A sample
   is skipped where the face is covered by another box (its point 5 cm out, grown by
   4 cm, is inside a box): only faces a player can touch count. From a point a little
   way out along the face's normal (up to the next box), a ray comes back toward the
   face; the first thing it hits must be a front face, at an offset from the face of
     - inside, below 2.3 m above the deck under it:  -2 cm .. +3 cm   (STRICT)
   A miss, a back face (the ray started inside some detail) or a deeper or prouder
   surface fails. Elsewhere details may stand off as far as they like, so the face only
   has to be filled: the point 2 cm inside it must be inside the model (along the ray's
   line, more faces entered than left up to it):
     - inside, higher (overhead, high walls)                          (OVERHEAD)
     - outside the hull                                               (OUTSIDE)
2. Walkable air. Lattice points inside the hull, 4 cm or more from every box and below
   2.3 m above their deck, must be outside every model shell: a point is inside when, on
   two or more of the three axis lines through it, more faces have been entered than left.
3. Doorways. Each doorway (and the bridge's window slit) must be a gap in the layout,
   and the opening measured in the model (rays from its middle out to each jamb, the
   sill and the lintel) must match the gap to 2 cm.
4. Lamps. Above each of the game's ceiling lights there must be a lit
   (emissive_secondary) face looking down, within 20 cm.

Known deviations are listed in KNOWN and reported, not failed. Exits 1 on any failure.
"""
import json
import math
import os
import sys

import numpy as np

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(os.path.dirname(HERE)))
sys.path.insert(0, os.path.join(ROOT, "tools", "assets"))
import verify_glb as V  # noqa: E402

H = 0.1                                       # lattice spacing
OFF = np.array([0.0371, 0.0213, 0.0437])      # lattice origin per axis
TOL_IN, TOL_OUT = 0.02, 0.031                 # a surface may sit 2 cm inside its box, details 3 cm out
                                              # (+1 mm: float32 positions)
HEADROOM = 2.3                                # above this over a deck, details are free
INSET = 0.04                                  # samples start this far in from a face's edges
COVER = 0.04                                  # a face point within this of another box is covered
W, UP, TOP = 6.0, 3.3, 6.3
INTERIOR = (np.array([-W, 0.0, -30.3]), np.array([W, TOP, 30.3]))

# Ceiling lights the game places (the design's lights, in layout.json).
with open(os.path.join(os.path.dirname(os.path.abspath(__file__)), "layout.json")) as _f:
    LAMPS = [tuple(p) for p in json.load(_f)["lights"]]

# Doorways: (name, normal axis, wall slab along it, opening along the other horizontal axis, y range).
DOORS = [
    ("hangar door (stern)", "z", (30.0, 30.3), (-3.0, 3.0), (0.0, 2.8)),
    ("hangar -> lower deck", "z", (13.85, 14.15), (-1.0, 1.0), (0.0, 2.4)),
    ("hangar ramp -> upper deck", "z", (13.85, 14.15), (-5.8, -3.8), (3.3, 5.7)),
    ("bridge door", "z", (-22.15, -21.85), (-1.0, 1.0), (3.3, 5.5)),
    ("bow window slit", "z", (-30.3, -30.0), (-6.0, 6.0), (4.5, 5.3)),
]
for x in (-2.0, 2.0):
    side = "port" if x < 0 else "starboard"
    for z0, z1 in ((-7.0, -5.4), (7.0, 8.6)):
        DOORS.append((f"lower {side} z {z0:+.1f}", "x", (x - 0.1, x + 0.1), (z0, z1), (0.0, 2.2)))
    for z0, z1 in ((-13.0, -11.4), (3.0, 4.6)):
        DOORS.append((f"upper {side} z {z0:+.1f}", "x", (x - 0.1, x + 0.1), (z0, z1), (3.3, 5.5)))

# Deliberate deviations: (name, layout box, sample region lo, hi, why).
KNOWN = []

AX = "xyz"


# ------------------------------------------------------------------ loading
def load_layout():
    with open(os.path.join(HERE, "layout.json")) as f:
        lay = json.load(f)
    boxes = []
    for b in lay["boxes"]:
        x, y, z, w = b["rotation"]
        R = np.array([[1 - 2 * (y * y + z * z), 2 * (x * y - z * w), 2 * (x * z + y * w)],
                      [2 * (x * y + z * w), 1 - 2 * (x * x + z * z), 2 * (y * z - x * w)],
                      [2 * (x * z - y * w), 2 * (y * z + x * w), 1 - 2 * (x * x + y * y)]])
        boxes.append((np.array(b["center"]), np.array(b["size"]) / 2, R, b["material"]))
    return lay, boxes


def load_tris():
    glb = V.Glb(os.path.join(HERE, "carrier.glb"))
    tris, mats, parts = [], [], []
    names = []
    for i, nm in enumerate(glb.names):
        if V.node_kind(nm) != "part" or i in glb.roots:
            continue
        M = np.array(glb.world[i])
        for p, P, _, idx in glb.primitives(i):
            mat = glb.json["materials"][p["material"]]["name"]
            P = np.array(P) @ M[:3, :3].T + M[:3, 3]
            T = P[np.array(idx).reshape(-1, 3)]
            tris.append(T)
            mats += [mat] * len(T)
            parts += [nm] * len(T)
        names.append(nm)
    return np.concatenate(tris), np.array(mats), np.array(parts)


def inside_any(boxes, p, margin, skip=None):
    """Mask of points p (N, 3) inside any box grown by margin (skip: a box index to ignore)."""
    out = np.zeros(len(p), bool)
    for k, (c, h, R, _) in enumerate(boxes):
        if k == skip:
            continue
        loc = (p - c) @ R
        out |= np.all(np.abs(loc) < h + margin, axis=1)
    return out


# ------------------------------------------------------------------ the ray index
class Lines:
    """Every triangle's crossings of the lattice lines along each axis, sorted per line."""

    def __init__(self, tris, mats):
        self.mat_names, mat_id = np.unique(mats, return_inverse=True)
        self.axes = {}
        for a in range(3):
            u, v = [k for k in range(3) if k != a]
            K, T, S, Mi = [], [], [], []
            n = np.cross(tris[:, 1] - tris[:, 0], tris[:, 2] - tris[:, 0])
            for t in range(len(tris)):
                na = n[t, a]
                if abs(na) < 1e-12 * (np.linalg.norm(n[t]) + 1e-30):
                    continue
                P = tris[t]
                iu0 = math.ceil((P[:, u].min() - OFF[u]) / H)
                iu1 = math.floor((P[:, u].max() - OFF[u]) / H)
                iv0 = math.ceil((P[:, v].min() - OFF[v]) / H)
                iv1 = math.floor((P[:, v].max() - OFF[v]) / H)
                if iu1 < iu0 or iv1 < iv0:
                    continue
                IU, IV = np.meshgrid(np.arange(iu0, iu1 + 1), np.arange(iv0, iv1 + 1), indexing="ij")
                IU, IV = IU.ravel(), IV.ravel()
                U, Vv = OFF[u] + IU * H, OFF[v] + IV * H
                (x0, y0), (x1, y1), (x2, y2) = P[0, [u, v]], P[1, [u, v]], P[2, [u, v]]
                d = (y1 - y2) * (x0 - x2) + (x2 - x1) * (y0 - y2)
                l0 = ((y1 - y2) * (U - x2) + (x2 - x1) * (Vv - y2)) / d
                l1 = ((y2 - y0) * (U - x2) + (x0 - x2) * (Vv - y2)) / d
                l2 = 1 - l0 - l1
                m = (l0 >= -1e-9) & (l1 >= -1e-9) & (l2 >= -1e-9)
                if not m.any():
                    continue
                tt = P[0, a] - (n[t, u] * (U[m] - P[0, u]) + n[t, v] * (Vv[m] - P[0, v])) / na
                K.append(self.key(IU[m], IV[m]))
                T.append(tt)
                S.append(np.full(m.sum(), 1 if na > 0 else -1, np.int8))
                Mi.append(np.full(m.sum(), mat_id[t], np.int16))
            K, T, S, Mi = (np.concatenate(x) for x in (K, T, S, Mi))
            o = np.lexsort((T, K))
            K, T, S, Mi = K[o], T[o], S[o], Mi[o]
            self.axes[a] = (K, T, S, Mi, K * 1000.0 + (T + 200.0))
            self.cum = getattr(self, "cum", {})
            self.cum[a] = np.cumsum(-S.astype(np.int32))      # entries (normal against +a) minus exits

    @staticmethod
    def key(iu, iv):
        return (np.asarray(iu, np.int64) + 2000) * 10000 + (np.asarray(iv, np.int64) + 2000)

    def first(self, a, iu, iv, t0, direction):
        """First crossing on the line (iu, iv) along axis a from t0 going +1/-1.
        Returns (found, t, sign of the triangle normal along a, material id)."""
        K, T, S, Mi, C = self.axes[a]
        k = self.key(iu, iv)
        q = k * 1000.0 + (np.asarray(t0) + 200.0)
        if direction > 0:
            j = np.searchsorted(C, q, "right")
        else:
            j = np.searchsorted(C, q, "left") - 1
        jj = np.clip(j, 0, len(K) - 1)
        found = (j >= 0) & (j < len(K)) & (K[jj] == k)
        return found, T[jj], S[jj], Mi[jj]

    def depth(self, a, iu, iv, t):
        """How many shells contain the point at t on the line (iu, iv) along axis a: faces
        entered minus faces left, coming from -infinity. Exact for closed shells, however
        they overlap."""
        K, T, S, Mi, C = self.axes[a]
        k = self.key(iu, iv)
        j = np.searchsorted(C, k * 1000.0 + (np.asarray(t) + 200.0), "left") - 1
        s0 = np.searchsorted(C, k * 1000.0, "left")
        base = np.where(s0 > 0, self.cum[a][np.maximum(s0 - 1, 0)], 0)
        upto = self.cum[a][np.clip(j, 0, len(K) - 1)]
        return np.where(j >= s0, upto - base, 0)


def idx(coord, axis):
    """Nearest lattice index of a coordinate."""
    return np.rint((np.asarray(coord) - OFF[axis]) / H).astype(np.int64)


def at(i, axis):
    return OFF[axis] + np.asarray(i) * H


# ------------------------------------------------------------------ decks
RAMP = (15.0, 27.0)


def ramp_top(z):
    return np.clip(UP * (RAMP[1] - z) / (RAMP[1] - RAMP[0]), 0.0, UP)


def deck(p):
    """Height of the deck a point stands over: the hangar floor (or the ramp and landing
    along its port wall), the lower deck, or the upper deck."""
    x, y, z = p[:, 0], p[:, 1], p[:, 2]
    hangar = z > 14.0
    rt = np.where(z <= 15.2, UP, ramp_top(z))
    on_ramp = hangar & (x < -3.6) & (y >= rt - 0.05)
    return np.where(on_ramp, rt, np.where(~hangar & (y >= 3.15), UP, 0.0))


def interior(p):
    return np.all((p > INTERIOR[0]) & (p < INTERIOR[1]), axis=1)


# ------------------------------------------------------------------ 1. surfaces
def face_samples(boxes):
    """Lattice samples on every box face: (points, normals, ray axis, lattice iu, iv, box index)."""
    out = []
    for b, (c, h, R, _) in enumerate(boxes):
        for j in range(3):
            j1, j2 = [k for k in range(3) if k != j]
            for sg in (-1, 1):
                n = sg * R[:, j]
                fc = c + sg * h[j] * R[:, j]
                a = int(np.argmax(np.abs(n)))
                u, v = [k for k in range(3) if k != a]
                corners = np.array([fc + s1 * h[j1] * R[:, j1] + s2 * h[j2] * R[:, j2]
                                    for s1 in (-1, 1) for s2 in (-1, 1)])
                iu = np.arange(math.ceil((corners[:, u].min() - OFF[u]) / H),
                               math.floor((corners[:, u].max() - OFF[u]) / H) + 1)
                iv = np.arange(math.ceil((corners[:, v].min() - OFF[v]) / H),
                               math.floor((corners[:, v].max() - OFF[v]) / H) + 1)
                if not len(iu) or not len(iv):
                    continue
                IU, IV = np.meshgrid(iu, iv, indexing="ij")
                IU, IV = IU.ravel(), IV.ravel()
                p = np.zeros((len(IU), 3))
                p[:, u], p[:, v] = at(IU, u), at(IV, v)
                p[:, a] = fc[a] - (n[u] * (p[:, u] - fc[u]) + n[v] * (p[:, v] - fc[v])) / n[a]
                s1, s2 = (p - fc) @ R[:, j1], (p - fc) @ R[:, j2]
                m = (np.abs(s1) <= h[j1] - INSET) & (np.abs(s2) <= h[j2] - INSET)
                if m.any():
                    out.append((p[m], np.tile(n, (m.sum(), 1)), np.full(m.sum(), a), IU[m], IV[m],
                                np.full(m.sum(), b)))
    return [np.concatenate(x) for x in zip(*out)]


def free_distance(boxes, p, n, dmax, step):
    """How far out along n from p before entering a box (capped at dmax)."""
    d = np.full(len(p), dmax)
    open_ = np.ones(len(p), bool)
    for s in np.arange(0.05, dmax + 1e-9, step):
        hit = open_ & inside_any(boxes, p + n * s, 0.01)
        d[hit] = max(s - 0.03, 0.04)
        open_ &= ~hit
    return d


def check_surfaces(lines, boxes):
    p, n, a, iu, iv, b = face_samples(boxes)
    q = p + n * 0.05
    exposed = ~np.zeros(len(p), bool)
    for k in range(len(boxes)):
        # covered by another box (grown by COVER): not a face anyone touches
        sel = b != k
        c, h, R, _ = boxes[k]
        loc = (q[sel] - c) @ R
        cov = np.all(np.abs(loc) < h + COVER, axis=1)
        tmp = exposed[sel]
        tmp &= ~cov
        exposed[sel] = tmp
    p, n, a, iu, iv, b, q = p[exposed], n[exposed], a[exposed], iu[exposed], iv[exposed], b[exposed], q[exposed]
    inside = interior(q)
    height = q[:, 1] - deck(q)
    cls = np.where(~inside, 2, np.where(height < HEADROOM, 0, 1))         # 0 strict, 1 overhead, 2 outside
    na = n[np.arange(len(n)), a]
    sgn = np.sign(na)
    ray = np.zeros_like(n)                                   # out along the ray's axis
    ray[np.arange(len(n)), a] = sgn
    D = np.empty(len(p))
    for c_, dm, st in ((0, 0.3, 0.02), (1, 0.8, 0.04), (2, 3.0, 0.1)):
        m = cls == c_
        D[m] = free_distance(boxes, p[m], ray[m], dm, st)
    t0 = p[np.arange(len(p)), a] + sgn * D
    found = np.zeros(len(p), bool)
    th = np.zeros(len(p))
    front = np.zeros(len(p), bool)
    for s in (-1, 1):
        m = sgn == s
        for ax in range(3):
            mm = m & (a == ax)
            if not mm.any():
                continue
            f, t, S, _ = lines.first(ax, iu[mm], iv[mm], t0[mm], -s)
            found[mm], th[mm], front[mm] = f, t, S == s
    off = (th - p[np.arange(len(p)), a]) * sgn * np.abs(na)              # along the normal
    ok = found & front & (off >= -TOL_IN) & (off <= TOL_OUT)
    # overhead and outside: the model fills the box to within TOL_IN of the face
    pa = p[np.arange(len(p)), a]
    filled = np.zeros(len(p), bool)
    for s in (-1, 1):
        for ax in range(3):
            mm = (sgn == s) & (a == ax) & (cls != 0)
            if not mm.any():
                continue
            filled[mm] = lines.depth(ax, iu[mm], iv[mm], pa[mm] - s * TOL_IN / np.abs(na[mm])) > 0
    ok = np.where(cls == 0, ok, filled)
    known = np.zeros(len(p), bool)
    for _, kb, lo, hi, _ in KNOWN:
        known |= (b == kb) & np.all((p >= lo) & (p <= hi), axis=1)
    return dict(p=p, n=n, b=b, cls=cls, off=np.where(found, off, np.nan), found=found, front=front, ok=ok,
                known=known)


# ------------------------------------------------------------------ 2. walkable air
def air_points(boxes, lo=INTERIOR[0], hi=INTERIOR[1], inset=0.0):
    rng = [np.arange(math.ceil((lo[k] + inset - OFF[k]) / H), math.floor((hi[k] - inset - OFF[k]) / H) + 1)
           for k in range(3)]
    I = np.stack(np.meshgrid(*rng, indexing="ij"), -1).reshape(-1, 3)
    p = OFF + I * H
    return p, I


def inside_model(lines, p, I):
    """For each point, on how many of the three axis lines through it it is inside a shell."""
    votes = np.zeros(len(p), np.int8)
    for ax in range(3):
        u, v = [k for k in range(3) if k != ax]
        votes += (lines.depth(ax, I[:, u], I[:, v], p[:, ax]) > 0).astype(np.int8)
    return votes


def check_air(lines, boxes):
    p, I = air_points(boxes)
    keep = interior(p)
    keep &= ~inside_any(boxes, p, COVER)
    h = p[:, 1] - deck(p)
    keep &= (h > 0) & (h < HEADROOM)
    p, I = p[keep], I[keep]
    votes = inside_model(lines, p, I)
    return p, votes >= 2


# ------------------------------------------------------------------ 3. doorways
def check_doors(lines, boxes, air_p, air_bad):
    rows = []
    for name, nax, (w0, w1), (u0, u1), (y0, y1) in DOORS:
        a = AX.index(nax)
        u = 0 if a == 2 else 2                                    # the horizontal axis along the wall
        lo, hi = np.zeros(3), np.zeros(3)
        lo[a], hi[a], lo[u], hi[u], lo[1], hi[1] = w0, w1, u0, u1, y0, y1
        # the opening is a gap in the layout, closed on its sides and top by boxes
        pts, _ = air_points(boxes, lo, hi, inset=0.005)
        gap_clear = not inside_any(boxes, pts, -0.004).any()
        # measured opening, on a lattice plane through the wall
        ia = idx(w0 + 0.06, a)
        meas = {}
        errs = []
        mid_u, mid_y = (u0 + u1) / 2, (y0 + y1) / 2
        ys = np.linspace(y0 + 0.1, y1 - 0.1, 7)
        us = np.linspace(u0 + 0.1, u1 - 0.1, 7)
        # sides: rays along u from the middle, at several heights
        for d, want in ((-1, u0), (1, u1)):
            if a == 2:                                            # lines along x are keyed (y, z)
                f, t, S, _ = lines.first(0, idx(ys, 1), np.full(len(ys), ia), mid_u, d)
            else:                                                 # lines along z are keyed (x, y)
                f, t, S, _ = lines.first(2, np.full(len(ys), ia), idx(ys, 1), mid_u, d)
            got = np.where(f & (S == -d), t, np.nan)
            errs.append(np.abs(got - want))
            meas[d] = np.nanmedian(got) if np.isfinite(got).any() else np.nan
        # sill and lintel: rays along y from the middle, across the opening
        for d, want in ((-1, y0), (1, y1)):
            if a == 2:                                            # lines along y are keyed (x, z)
                f, t, S, _ = lines.first(1, idx(us, 0), np.full(len(us), ia), mid_y, d)
            else:
                f, t, S, _ = lines.first(1, np.full(len(us), ia), idx(us, 2), mid_y, d)
            got = np.where(f & (S == -d), t, np.nan)
            errs.append(np.abs(got - want))
            meas[2 * d] = np.nanmedian(got) if np.isfinite(got).any() else np.nan
        err = np.concatenate(errs)
        worst = np.nanmax(err) if np.isfinite(err).any() else np.inf
        missing = int(np.isnan(err).sum())
        inside = np.all((air_p >= lo - 1e-6) & (air_p <= hi + 1e-6), axis=1)
        blocked = int((inside & air_bad).sum())
        ok = gap_clear and missing == 0 and worst <= TOL_IN and blocked == 0
        rows.append((name, u1 - u0, y1 - y0, meas[1] - meas[-1], meas[2] - meas[-2], worst, missing,
                     int(inside.sum()), blocked, gap_clear, ok))
    return rows


# ------------------------------------------------------------------ 4. lamps
def check_lamps(lines):
    lit = list(lines.mat_names).index("emissive_secondary")
    rows = []
    for x, y, z in LAMPS:
        ix, iz = idx(x, 0), idx(z, 2)
        f, t, S, M = lines.first(1, np.array([ix]), np.array([iz]), np.array([y]), 1)
        f, t, S, M = f[0], t[0], S[0], M[0]
        ok = bool(f and S < 0 and M == lit and t - y <= 0.2)
        rows.append(((x, y, z), t - y if f else np.nan, lines.mat_names[M] if f else "-", ok))
    return rows


# ------------------------------------------------------------------ main
def main():
    lay, boxes = load_layout()
    tris, mats, parts = load_tris()
    print(f"carrier.glb: {len(tris)} triangles; layout.json: {len(boxes)} boxes")
    lines = Lines(tris, mats)
    fails = 0

    print("\n== 1. surfaces: model surface at every exposed layout face")
    r = check_surfaces(lines, boxes)
    names = {0: "STRICT   (inside, < 2.3 m over a deck: -2..+3 cm)",
             1: "OVERHEAD (inside, higher: >= -2 cm)", 2: "OUTSIDE  (outside the hull: >= -2 cm)"}
    for c in (0, 1, 2):
        m = r["cls"] == c
        bad = m & ~r["ok"] & ~r["known"]
        off = r["off"][m & r["found"] & r["front"]]
        rng = f"offsets {np.nanmin(off) * 100:+.1f} .. {np.nanmax(off) * 100:+.1f} cm" if len(off) and c == 0 else ""
        print(f"  {names[c]}: {m.sum():7d} samples, {bad.sum():5d} fail  {rng}")
        fails += int(bad.sum())
    for name, kb, lo, hi, why in KNOWN:
        m = r["known"] & ~r["ok"]
        print(f"  known: {name}: {m.sum()} samples off ({np.nanmin(r['off'][m]) * 100:+.0f} cm at worst): {why}"
              if m.any() else f"  known: {name}: no samples off")
    bad = ~r["ok"] & ~r["known"]
    if bad.any():
        print("  failures by layout box:")
        for k in np.unique(r["b"][bad]):
            m = bad & (r["b"] == k)
            c, h, _, mat = boxes[k]
            ex = np.flatnonzero(m)[:3]
            why = []
            for e in ex:
                if not r["found"][e]:
                    why.append(f"{tuple(np.round(r['p'][e], 2))} nothing hit")
                elif not r["front"][e]:
                    why.append(f"{tuple(np.round(r['p'][e], 2))} inside a detail")
                else:
                    why.append(f"{tuple(np.round(r['p'][e], 2))} off {r['off'][e] * 100:+.1f} cm")
            print(f"    box {k:2d} ({mat}, centre {tuple(np.round(c, 2))}): {m.sum()} of "
                  f"{(r['b'] == k).sum()} fail; e.g. " + "; ".join(why))

    print("\n== 2. walkable air: nothing of the model in the space players move through")
    p, bad = check_air(lines, boxes)
    print(f"  {len(p)} lattice points (10 cm) below 2.3 m over a deck, >= 4 cm from every box: "
          f"{bad.sum()} inside the model")
    if bad.any():
        q = np.round(p[bad], 1)
        u, cnt = np.unique(np.round(q / 0.5) * 0.5, axis=0, return_counts=True)
        for o in np.argsort(-cnt)[:12]:
            print(f"    near {tuple(u[o])}: {cnt[o]} points")
    fails += int(bad.sum())

    print("\n== 3. doorways: open exactly as the gaps between the boxes")
    print(f"  {'doorway':28s} {'layout w x h':>13s} {'model w x h':>13s} {'worst':>6s} {'air pts':>8s}")
    for name, w, hgt, mw, mh, worst, missing, npts, blocked, gap_clear, ok in check_doors(lines, boxes, p, bad):
        flag = "ok  " if ok else "FAIL"
        extra = ("" if gap_clear else " NOT A GAP IN THE LAYOUT") + (f" {missing} rays missed" if missing else "") \
            + (f" {blocked} blocked" if blocked else "")
        print(f"  {flag} {name:23s} {w:5.2f} x {hgt:4.2f}   {mw:5.2f} x {mh:4.2f}  {worst * 100:4.1f}cm {npts:8d}{extra}")
        fails += 0 if ok else 1

    print("\n== 4. lamps: a lit fixture over each of the game's ceiling lights")
    for pos, dist, mat, ok in check_lamps(lines):
        print(f"  {'ok  ' if ok else 'FAIL'} {str(pos):18s} first face above: {mat} at {dist * 100:.0f} cm")
        fails += 0 if ok else 1

    print(f"\n{'LAYOUT CHECK PASSED' if not fails else f'LAYOUT CHECK FAILED: {fails} failures'}")
    return 1 if fails else 0


if __name__ == "__main__":
    sys.exit(main())
