"""Asset, Part and the node tree. See docs/assets.md for the API and the rules."""
import math
import os

import bpy

import verify_glb as V

from . import _state, geometry
from .space import frame, vec3

# The view the triangle rules are judged at (docs/art-style.md, "Spending triangles"):
# the game's vertical field of view (tuning.ron fov_deg) on a 1440-line screen. One pixel
# at distance d is d * PX_PER_M metres. Each kind's `near` (kinds.json) is the closest it is
# usually seen from, and its `detail_px` how many pixels there the smallest detail worth
# geometry spans (2 for things in the world, 1 for things held in front of the camera).
VIEW_FOV_DEG = 85.0
VIEW_LINES = 1440
PX_PER_M = 2.0 * math.tan(math.radians(VIEW_FOV_DEG) / 2.0) / VIEW_LINES


class Node:
    """A node of the exported tree: the root, a part, a socket or a collider.

    world_g is its transform in Bevy space. Everything is placed by WORLD position; hb
    works out the parent-local transforms, so nothing depends on parenting order.
    """

    def __init__(self, asset, name, kind, parent, world_g):
        self.asset, self.name, self.kind, self.parent, self.world_g = asset, name, kind, parent, world_g
        self.obj = None

    @property
    def location(self):
        """World position in Bevy space."""
        return tuple(self.world_g.translation)

    def __repr__(self):
        return f"<{self.kind} {self.name}>"


class Part(Node):
    """A mesh node. Add primitives to it; all coordinates are in the asset's Bevy frame
    (the same frame as `pivot`), not relative to the pivot."""

    def __init__(self, asset, name, parent, world_g, bevel, smooth_angle, bevel_segments):
        super().__init__(asset, name, "part", parent, world_g)
        self.prims = []
        self.bevel = bevel
        self.smooth_angle = smooth_angle
        self.bevel_segments = bevel_segments

    def _mat(self, mat):
        roles = V.load_roles()
        if mat not in roles:
            raise ValueError(f"{self.name}: material {mat!r} is not a role in tools/assets/materials.json"
                             f" ({sorted(roles)})")
        return mat

    def _bevel(self, bevel):
        b = self.bevel if bevel is None else float(bevel)
        if b < 0:
            raise ValueError(f"{self.name}: bevel {b} is negative")
        if 0 < b < self.asset.detail:          # under the kind's detail size
            self.asset._dropped_bevels += 1
            return 0.0
        return b

    def _segments(self, segments, profile, mats):
        """Explicit segments, or the fewest that keep the widest radius round to within the
        detail size (half of it if it glows: a lit edge against a dark one shows its corners).
        Returns (segments, smooth_seams)."""
        if segments is not None:
            return int(segments), False
        try:
            r = max(float(p[0]) for p in profile)
        except (TypeError, ValueError, IndexError):
            raise ValueError(f"{self.name}: lathe profile points must be (radius, t), got {profile!r}") from None
        roles = V.load_roles()
        glows = any("emissive_strength" in roles[m] for m in mats)
        return geometry.auto_segments(r, self.asset.detail / (2 if glows else 1)), True

    def _smooth(self, smooth_angle):
        return math.radians(self.smooth_angle if smooth_angle is None else float(smooth_angle))

    def _add(self, prim):
        if self.asset._exported:
            raise RuntimeError(f"{self.name}: geometry added after export()")
        self.prims.append(prim)
        return self

    def prism(self, points, plane="zy", span=(-0.01, 0.01), mat=None, smooth=False, bevel=None, smooth_angle=None):
        """Extrude a closed 2D outline. plane names the two Bevy axes the points are in, in
        order ('zy': points are (z, y), a side view); span is (from, to) along the third axis.
        smooth=True makes the side faces one curved surface (for outlines that approximate
        a curve); otherwise every face is flat."""
        return self._add(geometry.prism(points, plane, span, self._mat(mat), smooth, self._bevel(bevel),
                                        self._smooth(smooth_angle)))

    def box(self, lo, hi, mat=None, bevel=None):
        """Axis-aligned box between two opposite corners."""
        return self._add(geometry.box(lo, hi, self._mat(mat), self._bevel(bevel)))

    def frame(self, lo, hi, wall, axis, mat=None, bevel=None):
        """Rectangular sleeve: outer box lo..hi, open along axis ('x', 'y' or 'z'), walls `wall` thick."""
        return self._add(geometry.frame(lo, hi, wall, axis, self._mat(mat), self._bevel(bevel)))

    def lathe(self, profile, axis="z", center=(0.0, 0.0), mat=None, mats=None, segments=None, bevel=None,
              smooth_angle=None):
        """Surface of revolution. profile: [(radius, t)], t along `axis`. center: the other two
        axes' coordinates of the axis line, in xyz order (axis 'z' -> (x, y)). One mat, or
        mats with one role per profile segment. Open ends are capped.
        segments: leave it out for a round surface (the count comes from the widest radius
        and the kind's `near`, and the facets always shade smooth); give it for a deliberate
        polygon (then facets are hard where their angle is at least smooth_angle)."""
        if (mat is None) == (mats is None):
            raise ValueError(f"{self.name}: lathe takes mat= or mats=, not both or neither")
        n = len(profile) - 1
        mats = [self._mat(mat)] * n if mats is None else [self._mat(m) for m in mats]
        segments, seams = self._segments(segments, profile, mats)
        return self._add(geometry.lathe(profile, axis, center, mats, segments, self._bevel(bevel),
                                        self._smooth(smooth_angle), smooth_seams=seams))

    def cylinder(self, axis, center, radius, span, mat=None, segments=None, bevel=None, smooth_angle=None):
        """Capped cylinder along axis. center as for lathe; span is (from, to) along the axis."""
        a, b = sorted(float(s) for s in span)
        return self.lathe([(radius, a), (radius, b)], axis, center, mat=mat, segments=segments, bevel=bevel,
                          smooth_angle=smooth_angle)

    def hull(self, points, mat=None, bevel=None):
        """Convex hull of `points` (Bevy world): every face flat, coplanar sides merged.
        For wedges and sloped plates; one hull over an L shape is mostly air, so use several."""
        return self._add(geometry.hull(points, self._matf(mat), self._bevel(bevel)))

    # ---- organic primitives (flora, fauna): see geometry.py and docs/assets.md

    def _matf(self, mat):
        """A role, or a function of a face centre (Bevy world) -> role, checked per face."""
        if callable(mat):
            return lambda c: self._mat(mat(c))
        return self._mat(mat)

    def _organic_segments(self, segments, radius, mat):
        """Explicit, or the fewest even (at least 6) that keep `radius` round within the
        detail size (half of it if `mat` is a glowing role)."""
        if segments is not None:
            return int(segments)
        roles = V.load_roles()
        # A role function never counts as glowing; give segments explicitly if its glow shows.
        glows = isinstance(mat, str) and "emissive_strength" in roles.get(mat, {})
        return geometry.auto_segments(radius, self.asset.detail / (2 if glows else 1), minimum=6)

    def loft(self, rings, mat=None, smooth=True, crease=None):
        """A closed organic shell through rings of points (Bevy world, same count each; the
        first and last may be one point, a pole). smooth=True: one smooth skin (hard only
        across edges of at least `crease` degrees, if given); False: flat triangles."""
        return self._add(geometry.loft(rings, self._matf(mat), smooth, crease))

    def blob(self, center, radii, mat=None, forward=None, up=None, noise=0.0, seed=0, frequency=1.5,
             segments=None, smooth=True, crease=None):
        """A (noisy) ellipsoid: radii (rx, ry, rz) in a local frame oriented by forward/up
        (as for part()), poles on its local Y. noise is a fraction of the radius (0.2 =
        +/-20%), seed picks the shape. segments: around; the rings are half that."""
        rad = max(float(r) for r in radii)
        seg = self._organic_segments(segments, rad, mat)
        return self._add(geometry.blob(center, radii, self._matf(mat), seg, forward, up, noise, seed, frequency,
                                       smooth, crease))

    def sweep(self, path, radius, mat=None, sides=None, up=None, smooth=True, crease=None):
        """A tube along a path of points (Bevy world). radius: one number, or a list of one per point,
        each r or (r_side, r_up); 0 at an end makes it pointed. For tendrils, fronds,
        siphons and stalks."""
        if isinstance(radius, (int, float)):
            rmax = float(radius)
        else:
            rmax = max(max(r) if isinstance(r, (list, tuple)) else float(r) for r in radius)
        n = self._organic_segments(sides, rmax, mat)
        return self._add(geometry.sweep(path, radius, self._matf(mat), n, up, smooth, crease))

    def lump(self, center, radii, mat=None, forward=None, up=None, noise=0.25, seed=0, frequency=1.3,
             subdivisions=1, floor=None, bevel=None):
        """A rock: the convex hull of a noisy ellipsoid's icosphere points (subdivisions 1:
        42 points, about 80 faces; 2: 162), as the game builds its asteroids. Flat faces.
        floor: points below y = floor are raised to it (a flat base on a surface)."""
        return self._add(geometry.lump(center, radii, self._matf(mat), self._bevel(bevel), forward, up, noise,
                                       seed, frequency, subdivisions, floor))

    def points(self):
        """Every vertex of this part's primitives (Bevy world), before bevelling."""
        return [p for pr in self.prims for p in pr.verts]


class Collider(Node):
    def __init__(self, asset, name, parent, world_g, points):
        super().__init__(asset, name, "collider", parent, world_g)
        self.points = points


class Asset:
    """One asset: reads asset.json beside the build script and starts an empty scene.

        a = hb.Asset(__file__)
    """

    def __init__(self, build_file):
        if _state.current is not None:
            raise RuntimeError("one hb.Asset per build script")
        self.dir = os.path.dirname(os.path.abspath(build_file))
        try:
            self.sidecar = V.load_sidecar(self.dir)
        except ValueError as e:
            raise SystemExit(f"hb: {e}") from None
        self.name = self.sidecar["name"]
        self.kind = self.sidecar["kind"]
        self.kind_spec = V.load_kinds()[self.kind]
        self.palette = self.sidecar.get("palette", {})
        # The smallest detail worth geometry: detail_px pixels at the kind's near distance.
        # Bevels narrower than this are dropped; auto lathes keep their silhouette within it.
        self.detail = self.kind_spec["near"] * PX_PER_M * self.kind_spec["detail_px"]
        self._dropped_bevels = 0
        self._buried_edges = 0
        self.nodes = {}
        self._exported = False

        bpy.ops.wm.read_factory_settings(use_empty=True)
        scene = bpy.context.scene
        scene.unit_settings.system = "METRIC"
        scene.unit_settings.scale_length = 1.0
        scene.unit_settings.length_unit = "METERS"

        from mathutils import Matrix
        self.root = self._register(Node(self, self.name, "root", None, Matrix.Identity(4)))
        _state.current = self

    # ---- tree
    def _parent(self, parent):
        if parent is None:
            return self.root
        if isinstance(parent, str):
            if parent not in self.nodes:
                raise ValueError(f"parent {parent!r} doesn't exist yet (have {list(self.nodes)})")
            parent = self.nodes[parent]
        if not isinstance(parent, Node) or parent.asset is not self:
            raise TypeError(f"parent must be a node from this asset (part/socket/root), got {parent!r}")
        if parent.kind == "collider":
            raise ValueError(f"{parent.name}: colliders can't have children")
        return parent

    def _register(self, node):
        if self._exported:
            raise RuntimeError(f"{node.name}: added after export()")
        rx = {"root": V.PART_RE, "part": V.PART_RE, "socket": V.SOCKET_RE, "collider": V.COL_RE}[node.kind]
        if not rx.match(node.name):
            want = {"root": "snake_case", "part": "snake_case (lower case, digits, underscores)",
                    "socket": "SOCKET_<snake_case>", "collider": "COL_<snake_case>"}[node.kind]
            raise ValueError(f"{node.kind} name {node.name!r} must be {want}")
        if node.name in self.nodes:
            raise ValueError(f"name {node.name!r} is already used by {self.nodes[node.name]!r}")
        self.nodes[node.name] = node
        return node

    def _rot(self, parent):
        return parent.world_g.to_3x3().normalized()

    def part(self, name, parent=None, pivot=(0.0, 0.0, 0.0), forward=None, up=None, bevel=None,
             smooth_angle=30.0, bevel_segments=None):
        """A mesh node pivoted at `pivot` (Bevy world). Parent defaults to the root.
        Orientation: local -Z along forward, +Y toward up; default is the parent's.
        bevel: width in metres for this part's edges (default: the kind's default_bevel).
        smooth_angle: curved surfaces stay smooth across edges shallower than this (degrees).
        bevel_segments: default the kind's (kinds.json). 1 is a chamfer; 2 rounds the edge, at
        about 2.5x the bevel's triangles (worth it only for bevels several pixels wide)."""
        parent = self._parent(parent)
        if bevel is None:
            bevel = self.kind_spec["default_bevel"]
        lo, hi = self.kind_spec["bevel"]
        if bevel and not (lo <= bevel <= hi):
            print(f"hb: note: {name} bevel {bevel} m is outside the {self.kind} guide {lo}-{hi} m")
        if bevel_segments is None:
            bevel_segments = self.kind_spec["bevel_segments"]
        p = Part(self, name, parent, frame(pivot, forward, up, self._rot(parent)), float(bevel), float(smooth_angle),
                 int(bevel_segments))
        return self._register(p)

    def socket(self, name, parent=None, at=(0.0, 0.0, 0.0), forward=None, up=None):
        """An empty SOCKET_<name> at `at` (Bevy world). Orientation as for part()."""
        parent = self._parent(parent)
        return self._register(Node(self, name, "socket", parent, frame(at, forward, up, self._rot(parent))))

    def collider(self, name, points=None, around=None, parent=None):
        """A convex COL_<name>: the hull of `points` (Bevy world), or of the vertices of the
        part(s) in `around`. Keep to a few simple hulls; one hull over an L shape is mostly air."""
        parent = self._parent(parent)
        pts = []
        if points is not None:
            pts += [vec3(p, "collider point") for p in points]
        if around is not None:
            for p in (around if isinstance(around, (list, tuple)) else [around]):
                if not isinstance(p, Part):
                    raise TypeError(f"{name}: around= takes parts, got {p!r}")
                if not p.prims:
                    raise ValueError(f"{name}: part {p.name} has no geometry yet; add the collider after it")
                pts += p.points()
        if len(pts) < 4:
            raise ValueError(f"{name}: a collider needs points= or around= with at least 4 points")
        c = Collider(self, name, parent, frame(parent.location, None, None, self._rot(parent)), pts)
        return self._register(c)

    def export(self):
        """Finish every part (bevel, hard edges, weighted normals), export <name>.glb, write
        <name>.intent.json, check nothing was dropped, and render previews if asked."""
        from . import export
        return export.export(self)
