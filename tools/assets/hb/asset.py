"""Asset, Part and the node tree. See docs/assets.md for the API and the rules."""
import math
import os

import bpy

import verify_glb as V

from . import _state, geometry
from .space import frame, vec3


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
        return b

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

    def lathe(self, profile, axis="z", center=(0.0, 0.0), mat=None, mats=None, segments=20, bevel=None,
              smooth_angle=None):
        """Surface of revolution. profile: [(radius, t)], t along `axis`. center: the other two
        axes' coordinates of the axis line, in xyz order (axis 'z' -> (x, y)). One mat, or
        mats with one role per profile segment. Open ends are capped."""
        if (mat is None) == (mats is None):
            raise ValueError(f"{self.name}: lathe takes mat= or mats=, not both or neither")
        n = len(profile) - 1
        mats = [self._mat(mat)] * n if mats is None else [self._mat(m) for m in mats]
        return self._add(geometry.lathe(profile, axis, center, mats, segments, self._bevel(bevel),
                                        self._smooth(smooth_angle)))

    def cylinder(self, axis, center, radius, span, mat=None, segments=20, bevel=None, smooth_angle=None):
        """Capped cylinder along axis. center as for lathe; span is (from, to) along the axis."""
        a, b = sorted(float(s) for s in span)
        return self.lathe([(radius, a), (radius, b)], axis, center, mat=mat, segments=segments, bevel=bevel,
                          smooth_angle=smooth_angle)

    def hull(self, points, mat=None, bevel=None):
        """Convex hull of `points` (Bevy world): every face flat, coplanar sides merged.
        For wedges and sloped plates; one hull over an L shape is mostly air, so use several."""
        return self._add(geometry.hull(points, self._mat(mat), self._bevel(bevel)))

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
             smooth_angle=30.0, bevel_segments=2):
        """A mesh node pivoted at `pivot` (Bevy world). Parent defaults to the root.
        Orientation: local -Z along forward, +Y toward up; default is the parent's.
        bevel: width in metres for this part's edges (default: the kind's default_bevel).
        smooth_angle: curved surfaces stay smooth across edges shallower than this (degrees)."""
        parent = self._parent(parent)
        if bevel is None:
            bevel = self.kind_spec["default_bevel"]
        lo, hi = self.kind_spec["bevel"]
        if bevel and not (lo <= bevel <= hi):
            print(f"hb: note: {name} bevel {bevel} m is outside the {self.kind} guide {lo}-{hi} m")
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
