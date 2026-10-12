#!/usr/bin/env python3
"""Verify a built asset against docs/art-style.md and its own asset.json.

    python3 -I tools/assets/verify_glb.py assets/weapons/smg_receiver

Plain Python 3 stdlib; never needs Blender. Reads <dir>/asset.json, <dir>/<name>.glb and
<dir>/<name>.intent.json (written by the build: which faces were modelled flat).
Prints the node tree, materials, triangles and bounds, then one line per check.
Exits 1 if any check FAILs; WARN lines don't fail.

The hb build library imports this module for its GLB reader and the standards loaders,
so the rules (names, materials, budgets) are defined once, here and in the two JSON files.
"""
import json
import math
import os
import re
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
MATERIALS_JSON = os.path.join(HERE, "materials.json")
KINDS_JSON = os.path.join(HERE, "kinds.json")

# ---------------------------------------------------------------- naming rules
SNAKE = r"[a-z][a-z0-9]*(?:_[a-z0-9]+)*"
PART_RE = re.compile(rf"^{SNAKE}$")
SOCKET_RE = re.compile(rf"^SOCKET_{SNAKE}$")
COL_RE = re.compile(rf"^COL_{SNAKE}$")

FLAT_DEG = 1.0          # a flat face's vertex normals must be within this of its plane normal
DIR_DEG = 5.0           # socket forward/up expectations
COLOR_TOL = 2e-3        # linear colour factors
CONVEX_TOL = 2e-5       # metres: float32 GLB positions
PLANE_TOL = 2e-5        # metres: a triangle lies on an intended-flat plane
WEAK_SPOT = "#ff40c8"   # emissive_core: the same hot magenta on every enemy (art-style.md)


# ---------------------------------------------------------------- standards (shared with hb)
def srgb_hex_to_linear(h):
    """'#rrggbb' (sRGB, as a colour picker shows it) -> linear [r, g, b]."""
    if not isinstance(h, str) or not re.fullmatch(r"#[0-9a-fA-F]{6}", h):
        raise ValueError(f"colour {h!r} is not '#rrggbb' sRGB hex")
    out = []
    for i in (1, 3, 5):
        c = int(h[i:i + 2], 16) / 255.0
        out.append(c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4)
    return out


def load_roles():
    with open(MATERIALS_JSON) as f:
        return json.load(f)["roles"]


def load_kinds():
    with open(KINDS_JSON) as f:
        return json.load(f)["kinds"]


def load_sidecar(asset_dir):
    """Read and validate asset.json. Raises ValueError with a readable message."""
    path = os.path.join(asset_dir, "asset.json")
    if not os.path.isfile(path):
        raise ValueError(f"{path} is missing")
    with open(path) as f:
        sc = json.load(f)
    roles, kinds = load_roles(), load_kinds()
    for key in ("name", "kind"):
        if key not in sc:
            raise ValueError(f"asset.json: '{key}' is required")
    if not PART_RE.match(sc["name"]):
        raise ValueError(f"asset.json: name {sc['name']!r} must be snake_case")
    dirname = os.path.basename(os.path.normpath(os.path.abspath(asset_dir)))
    if sc["name"] != dirname:
        raise ValueError(f"asset.json: name {sc['name']!r} must match its directory {dirname!r}")
    if sc["kind"] not in kinds:
        raise ValueError(f"asset.json: kind {sc['kind']!r} is not one of {sorted(kinds)} (tools/assets/kinds.json)")
    known = {"name", "kind", "brief", "palette", "expect"}
    extra = set(sc) - known - {k for k in sc if k.startswith("_")}
    if extra:
        raise ValueError(f"asset.json: unknown keys {sorted(extra)} (known: {sorted(known)})")
    for role, colour in sc.get("palette", {}).items():
        if role not in roles:
            raise ValueError(f"asset.json palette: {role!r} is not a material role ({sorted(roles)})")
        srgb_hex_to_linear(colour)
    exp = sc.get("expect", {})
    known_exp = {"size", "parts", "sockets", "colliders", "tolerance"}
    extra = set(exp) - known_exp - {k for k in exp if k.startswith("_")}
    if extra:
        raise ValueError(f"asset.json expect: unknown keys {sorted(extra)} (known: {sorted(known_exp)})")
    def nums(v, n):
        return isinstance(v, list) and len(v) == n and all(isinstance(c, (int, float)) for c in v)

    for ax, rng in exp.get("size", {}).items():
        if ax not in ("x", "y", "z"):
            raise ValueError(f"asset.json expect.size: axis {ax!r} must be x, y or z")
        if not nums(rng, 2) or rng[0] > rng[1]:
            raise ValueError(f"asset.json expect.size.{ax}: must be [min, max] metres, got {rng!r}")
    for s, want in exp.get("sockets", {}).items():
        if not SOCKET_RE.match(s):
            raise ValueError(f"asset.json expect.sockets: {s!r} is not a SOCKET_<snake_case> name")
        if not isinstance(want, dict):
            raise ValueError(f"asset.json expect.sockets.{s}: must be an object like {{\"at\": [x, y, z]}}")
        bad = set(want) - {"at", "forward", "up", "tol"}
        if bad:
            raise ValueError(f"asset.json expect.sockets.{s}: unknown keys {sorted(bad)} (known: at, forward, up, tol)")
        for key in ("at", "forward", "up"):
            if key in want and not nums(want[key], 3):
                raise ValueError(f"asset.json expect.sockets.{s}.{key}: must be [x, y, z], got {want[key]!r}")
    for key in ("parts", "colliders"):
        if not isinstance(exp.get(key, []), list):
            raise ValueError(f"asset.json expect.{key}: must be a list of names")
    return sc


def resolve_material(role, palette, roles=None):
    """The exact glTF values a role must export as, given the asset's palette."""
    roles = roles or load_roles()
    if role not in roles:
        raise ValueError(f"material {role!r} is not a role in tools/assets/materials.json ({sorted(roles)})")
    r = roles[role]
    colour = srgb_hex_to_linear(palette.get(role, r["color"]))
    out = {
        "metallic": float(r["metallic"]),
        "roughness": float(r["roughness"]),
        "alpha": float(r.get("alpha", 1.0)),
        "emissive": [0.0, 0.0, 0.0],
        "strength": 0.0,
    }
    if "emissive_strength" in r:
        out["emissive"] = colour
        out["strength"] = float(r["emissive_strength"])
        k = float(r.get("base_from_emission", 0.1))
        out["base"] = [k * c for c in colour]
    else:
        out["base"] = colour
    return out


# ---------------------------------------------------------------- GLB reading
_COMP = {5120: "b", 5121: "B", 5122: "h", 5123: "H", 5125: "I", 5126: "f"}
_NCOMP = {"SCALAR": 1, "VEC2": 2, "VEC3": 3, "VEC4": 4, "MAT4": 16}
IDENTITY = [[1.0 if i == j else 0.0 for j in range(4)] for i in range(4)]


def mat_mul(a, b):
    return [[sum(a[i][k] * b[k][j] for k in range(4)) for j in range(4)] for i in range(4)]


def xform(m, p):
    return tuple(m[i][0] * p[0] + m[i][1] * p[1] + m[i][2] * p[2] + m[i][3] for i in range(3))


def xdir(m, v):
    return tuple(m[i][0] * v[0] + m[i][1] * v[1] + m[i][2] * v[2] for i in range(3))


def local_matrix(n):
    if "matrix" in n:
        m = n["matrix"]
        return [[m[c * 4 + r] for c in range(4)] for r in range(4)]
    t = n.get("translation", [0, 0, 0])
    x, y, z, w = n.get("rotation", [0, 0, 0, 1])
    s = n.get("scale", [1, 1, 1])
    r = [[1 - 2 * (y * y + z * z), 2 * (x * y - z * w), 2 * (x * z + y * w)],
         [2 * (x * y + z * w), 1 - 2 * (x * x + z * z), 2 * (y * z - x * w)],
         [2 * (x * z - y * w), 2 * (y * z + x * w), 1 - 2 * (x * x + y * y)]]
    return [[r[i][0] * s[0], r[i][1] * s[1], r[i][2] * s[2], t[i]] for i in range(3)] + [[0, 0, 0, 1]]


class Glb:
    """Minimal GLB reader: the JSON, accessors, node names, parents and world matrices."""

    def __init__(self, path):
        with open(path, "rb") as f:
            data = f.read()
        magic, version, length = struct.unpack_from("<III", data, 0)
        if magic != 0x46546C67 or version != 2 or length != len(data):
            raise ValueError(f"{path}: not a GLB v2 file")
        jlen, jtype = struct.unpack_from("<II", data, 12)
        if jtype != 0x4E4F534A:
            raise ValueError(f"{path}: first chunk is not JSON")
        self.json = json.loads(data[20:20 + jlen])
        off = 20 + jlen
        self.bin = b""
        self.chunks = 1
        if off < len(data):
            blen, btype = struct.unpack_from("<II", data, off)
            if btype != 0x004E4942:
                raise ValueError(f"{path}: second chunk is not BIN")
            self.bin = data[off + 8: off + 8 + blen]
            self.chunks = 2
        g = self.json
        self.nodes = g.get("nodes", [])
        self.names = [n.get("name", f"<node {i}>") for i, n in enumerate(self.nodes)]
        self.parent = {}
        for i, n in enumerate(self.nodes):
            for c in n.get("children", []):
                self.parent[c] = i
        scene = g.get("scenes", [{}])[g.get("scene", 0)]
        self.roots = scene.get("nodes", [])
        self.world = {}

        def walk(i, pm):
            self.world[i] = mat_mul(pm, local_matrix(self.nodes[i]))
            for c in self.nodes[i].get("children", []):
                walk(c, self.world[i])

        for r in self.roots:
            walk(r, IDENTITY)
        self.index = {nm: i for i, nm in enumerate(self.names)}

    def accessor(self, i):
        a = self.json["accessors"][i]
        bv = self.json["bufferViews"][a["bufferView"]]
        n, c = a["count"], _NCOMP[a["type"]]
        fmt = _COMP[a["componentType"]]
        size = struct.calcsize(fmt)
        stride = bv.get("byteStride", size * c)
        base = bv.get("byteOffset", 0) + a.get("byteOffset", 0)
        return [struct.unpack_from("<" + fmt * c, self.bin, base + k * stride) for k in range(n)]

    def primitives(self, node_i):
        """[(primitive json, positions, normals or None, flat index list)] of a node's mesh."""
        n = self.nodes[node_i]
        if "mesh" not in n:
            return []
        out = []
        for p in self.json["meshes"][n["mesh"]]["primitives"]:
            P = self.accessor(p["attributes"]["POSITION"])
            N = self.accessor(p["attributes"]["NORMAL"]) if "NORMAL" in p["attributes"] else None
            idx = [t[0] for t in self.accessor(p["indices"])] if "indices" in p else list(range(len(P)))
            out.append((p, P, N, idx))
        return out

    def triangles(self, node_i):
        return sum((len(idx) // 3) for _, _, _, idx in self.primitives(node_i))


# ---------------------------------------------------------------- small vector helpers
def sub(a, b):
    return (a[0] - b[0], a[1] - b[1], a[2] - b[2])


def dot(a, b):
    return a[0] * b[0] + a[1] * b[1] + a[2] * b[2]


def cross(a, b):
    return (a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0])


def norm(a):
    ln = math.sqrt(dot(a, a))
    return (a[0] / ln, a[1] / ln, a[2] / ln) if ln > 0 else (0.0, 0.0, 0.0)


def angle_deg(a, b):
    return math.degrees(math.acos(max(-1.0, min(1.0, dot(norm(a), norm(b))))))


def node_kind(name):
    if name.startswith("SOCKET_"):
        return "socket"
    if name.startswith("COL_"):
        return "collider"
    return "part"


def shells(prims):
    """Split triangles into connected shells, joining vertices by position (the exporter
    splits vertices along hard edges). Returns [(signed volume, triangles, edge counts)]."""
    tris = []
    for _, P, _, idx in prims:
        for t in range(0, len(idx), 3):
            tris.append(tuple(P[idx[t + k]] for k in range(3)))
    key = {}

    def k(v):
        return key.setdefault(tuple(round(c * 1e6) for c in v), len(key))

    keyed = [tuple(k(v) for v in tri) for tri in tris]
    parent = list(range(len(key)))

    def find(a):
        while parent[a] != a:
            parent[a] = parent[parent[a]]
            a = parent[a]
        return a

    for a, b, c in keyed:
        parent[find(b)] = find(a)
        parent[find(c)] = find(a)
    out = {}
    for tri, kt in zip(tris, keyed):
        sh = out.setdefault(find(kt[0]), [0.0, 0, {}])
        a, b, c = tri
        sh[0] += dot(a, cross(b, c)) / 6.0
        sh[1] += 1
        edges = sh[2]
        for e in ((kt[0], kt[1]), (kt[1], kt[2]), (kt[2], kt[0])):
            if e[0] != e[1]:
                e = tuple(sorted(e))
                edges[e] = edges.get(e, 0) + 1
    return [tuple(v) for v in out.values()]


# ---------------------------------------------------------------- the checks
class Report:
    def __init__(self):
        self.fails, self.warns = [], []

    def check(self, cond, msg):
        print(("  ok   " if cond else "  FAIL ") + msg)
        if not cond:
            self.fails.append(msg)
        return cond

    def warn(self, msg):
        print("  WARN " + msg)
        self.warns.append(msg)


def verify(asset_dir):
    rep = Report()
    try:
        sc = load_sidecar(asset_dir)
    except ValueError as e:
        rep.check(False, str(e))
        return rep
    name, kind = sc["name"], sc["kind"]
    roles, kinds = load_roles(), load_kinds()
    palette = sc.get("palette", {})
    exp = sc.get("expect", {})
    tol = float(exp.get("tolerance", 0.01))
    glb_path = os.path.join(asset_dir, f"{name}.glb")
    intent_path = os.path.join(asset_dir, f"{name}.intent.json")
    print(f"== {name} ({kind}: {kinds[kind]['doc']}) ==")
    if not rep.check(os.path.isfile(glb_path), f"{name}.glb exists"):
        return rep
    try:
        glb = Glb(glb_path)
    except (ValueError, struct.error) as e:
        rep.check(False, f"GLB parses: {e}")
        return rep
    g = glb.json

    # ---- container
    print("\n-- container")
    rep.check(glb.chunks == 2 and len(g.get("buffers", [])) == 1 and "uri" not in g["buffers"][0],
              "single embedded BIN buffer, no external URIs")
    rep.check(not g.get("images") and not g.get("textures") and not g.get("samplers"),
              "no images or textures (materials only, per art style)")
    rep.check(not g.get("cameras") and not g.get("extensions", {}).get("KHR_lights_punctual"),
              "no cameras or lights")
    if g.get("animations") or g.get("skins"):
        rep.warn("file has animations or skins; nothing here checks them yet")

    # ---- hierarchy
    print("\n-- hierarchy (glTF space: +Y up, -Z forward, +X right; world positions)")

    def show(i, depth):
        n = glb.nodes[i]
        m = glb.world[i]
        what = f"mesh, {glb.triangles(i)} tris" if "mesh" in n else "empty"
        rot = "" if n.get("rotation") in (None, [0, 0, 0, 1]) else f" rot={[round(v, 4) for v in n['rotation']]}"
        print(f"  {'  ' * depth}{glb.names[i]}  [{what}]  at ({m[0][3]:+.4f}, {m[1][3]:+.4f}, {m[2][3]:+.4f}){rot}")
        for c in n.get("children", []):
            show(c, depth + 1)

    for r in glb.roots:
        show(r, 0)
    print()
    rep.check(len(glb.roots) == 1 and glb.names[glb.roots[0]] == name,
              f"one scene root, named '{name}' (got {[glb.names[r] for r in glb.roots]})")
    if glb.roots:
        root = glb.nodes[glb.roots[0]]
        rep.check(not any(k in root for k in ("translation", "rotation", "scale", "matrix")),
                  "root transform is identity")
    scaled = [glb.names[i] for i, n in enumerate(glb.nodes)
              if "matrix" in n or any(abs(s - 1) > 1e-6 for s in n.get("scale", [1, 1, 1]))]
    rep.check(not scaled, f"no node carries scale or a raw matrix ({scaled or 'none'})")
    if glb.roots:
        rep.check("mesh" not in glb.nodes[glb.roots[0]], "the root has no mesh (geometry belongs on parts)")
    unreached = [glb.names[i] for i in range(len(glb.nodes)) if i not in glb.world]
    rep.check(not unreached, f"every node is in the scene ({unreached or 'all reached'})")

    # ---- names
    print("\n-- names")
    rep.check(len(set(glb.names)) == len(glb.names), "node names are unique")
    bad = []
    for i, nm in enumerate(glb.names):
        if i in glb.roots:
            continue
        k = node_kind(nm)
        rx = {"socket": SOCKET_RE, "collider": COL_RE, "part": PART_RE}[k]
        if not rx.match(nm):
            bad.append(nm)
    rep.check(not bad, f"names are snake_case parts, SOCKET_<name> or COL_<name> ({bad or 'all good'})")
    sockets = [i for i, nm in enumerate(glb.names) if nm.startswith("SOCKET_")]
    cols = [i for i, nm in enumerate(glb.names) if nm.startswith("COL_")]
    parts = [i for i, nm in enumerate(glb.names) if node_kind(nm) == "part" and i not in glb.roots]
    with_mesh = [glb.names[i] for i in sockets if "mesh" in glb.nodes[i]]
    rep.check(not with_mesh, f"sockets have no mesh ({with_mesh or 'none do'})")
    no_mesh = [glb.names[i] for i in cols + parts if "mesh" not in glb.nodes[i]]
    rep.check(not no_mesh, f"parts and COL_ nodes have a mesh ({no_mesh or 'all do'})")
    mismatch = [glb.names[i] for i in parts + cols if "mesh" in glb.nodes[i]
                and g["meshes"][glb.nodes[i]["mesh"]].get("name") != glb.names[i]]
    rep.check(not mismatch, f"each mesh is named after its node ({mismatch or 'all are'})")
    for want, label in ((exp.get("parts", []), "part"), (exp.get("colliders", []), "collider")):
        for nm in want:
            rep.check(nm in glb.index and "mesh" in glb.nodes[glb.index[nm]], f"expected {label} '{nm}' present")
    if not cols:
        rep.warn("no COL_ collider; anything that sits in the physics world needs one")

    # ---- materials
    print("\n-- materials")
    mats = g.get("materials", [])
    used_roles = set()
    for m in mats:
        pbr = m.get("pbrMetallicRoughness", {})
        st = m.get("extensions", {}).get("KHR_materials_emissive_strength", {}).get("emissiveStrength")
        print(f"  {m.get('name')}: base={[round(v, 4) for v in pbr.get('baseColorFactor', [1, 1, 1, 1])]}"
              f" metallic={pbr.get('metallicFactor', 1.0)} roughness={round(pbr.get('roughnessFactor', 1.0), 4)}"
              f" emissive={[round(v, 4) for v in m.get('emissiveFactor', [0, 0, 0])]} strength={st}"
              f" alpha={m.get('alphaMode', 'OPAQUE')} doubleSided={m.get('doubleSided', False)}")
    for m in mats:
        nm = m.get("name")
        if nm not in roles:
            rep.check(False, f"material '{nm}' is a library role (tools/assets/materials.json)")
            continue
        used_roles.add(nm)
        want = resolve_material(nm, palette, roles)
        pbr = m.get("pbrMetallicRoughness", {})
        base = pbr.get("baseColorFactor", [1, 1, 1, 1])
        em = m.get("emissiveFactor", [0, 0, 0])
        st = m.get("extensions", {}).get("KHR_materials_emissive_strength", {}).get("emissiveStrength", 1.0)
        if want["strength"] == 0:
            st = 0.0 if max(em) == 0 else st
        problems = []
        if any(abs(a - b) > COLOR_TOL for a, b in zip(base[:3], want["base"])):
            problems.append(f"base colour {[round(v, 4) for v in base[:3]]} != {[round(v, 4) for v in want['base']]}")
        if abs(base[3] - want["alpha"]) > COLOR_TOL:
            problems.append(f"alpha {base[3]} != {want['alpha']}")
        if abs(pbr.get("metallicFactor", 1.0) - want["metallic"]) > 1e-3:
            problems.append(f"metallic {pbr.get('metallicFactor', 1.0)} != {want['metallic']}")
        if abs(pbr.get("roughnessFactor", 1.0) - want["roughness"]) > 1e-3:
            problems.append(f"roughness {pbr.get('roughnessFactor', 1.0)} != {want['roughness']}")
        # Blender's exporter normalises the colour (brightest channel 1) and scales the
        # strength to match, so compare the product: the light actually emitted.
        got_e = [c * st for c in em]
        want_e = [c * want["strength"] for c in want["emissive"]]
        if any(abs(a - b) > COLOR_TOL * max(1.0, want["strength"]) for a, b in zip(got_e, want_e)):
            problems.append(f"emitted colour x strength {[round(v, 4) for v in got_e]} != {[round(v, 4) for v in want_e]}")
        want_alpha = "BLEND" if want["alpha"] < 1 else "OPAQUE"
        if m.get("alphaMode", "OPAQUE") != want_alpha:
            problems.append(f"alphaMode {m.get('alphaMode', 'OPAQUE')} != {want_alpha}")
        if m.get("doubleSided", False):
            problems.append("doubleSided (backface culling is off)")
        if any(k.endswith("Texture") for k in list(pbr) + list(m)):
            problems.append("has a texture")
        src = "palette" if nm in palette else "library default"
        rep.check(not problems, f"material '{nm}' matches the library ({src} colour)"
                  + (": " + "; ".join(problems) if problems else ""))
    for nm in sorted(used_roles):
        if roles[nm].get("palette") and nm not in palette:
            rep.check(False, f"'{nm}' is used, so asset.json palette must name its colour (from the brief)")
    if "emissive_core" in used_roles:
        rep.check(palette.get("emissive_core", "").lower() == WEAK_SPOT,
                  f"emissive_core is the weak-spot magenta {WEAK_SPOT} (got {palette.get('emissive_core')})")
        rep.check("SOCKET_weak" in glb.index, "a model with an emissive_core weak spot marks it with SOCKET_weak")
    unused_palette = sorted(set(palette) - used_roles)
    if unused_palette:
        rep.warn(f"palette names roles the model doesn't use: {unused_palette}")
    if any("KHR_materials_emissive_strength" in m.get("extensions", {}) for m in mats):
        rep.check("KHR_materials_emissive_strength" in g.get("extensionsUsed", []),
                  "KHR_materials_emissive_strength declared in extensionsUsed")
    referenced = {p["material"] for mesh in g.get("meshes", []) for p in mesh["primitives"] if "material" in p}
    rep.check(referenced == set(range(len(mats))), "every material is used by some mesh")

    # ---- meshes, triangles, normals
    print("\n-- triangles")
    total = 0
    render_lo, render_hi = [math.inf] * 3, [-math.inf] * 3
    nonunit = misses = bad_wind = 0
    no_mat = []
    for i in parts:
        tris = 0
        for p, P, N, idx in glb.primitives(i):
            if p.get("mode", 4) != 4:
                rep.check(False, f"{glb.names[i]}: primitive mode {p.get('mode')} is not TRIANGLES")
                continue
            tris += len(idx) // 3
            if "material" not in p:
                no_mat.append(glb.names[i])
            if N is None:
                misses += 1
                continue
            nonunit += sum(1 for nv in N if abs(math.sqrt(dot(nv, nv)) - 1) > 1e-3)
            for t in range(0, len(idx), 3):
                a, b, c = P[idx[t]], P[idx[t + 1]], P[idx[t + 2]]
                gn = cross(sub(b, a), sub(c, a))
                if dot(gn, gn) < 1e-20:
                    continue
                if sum(dot(N[idx[t + k]], gn) for k in range(3)) <= 0:
                    bad_wind += 1
            for v in P:
                w = xform(glb.world[i], v)
                for k in range(3):
                    render_lo[k], render_hi[k] = min(render_lo[k], w[k]), max(render_hi[k], w[k])
        total += tris
        print(f"  {glb.names[i]:20s} {tris:6d}")
    for i in cols:
        print(f"  {glb.names[i]:20s} {glb.triangles(i):6d}  (collider, not counted)")
    lo, hi = kinds[kind]["tris"]
    print(f"  {'TOTAL':20s} {total:6d}  (budget {lo}-{hi} for {kind})")
    rep.check(total <= hi, f"triangles {total} within the {kind} budget (max {hi})")
    if total < lo:
        rep.warn(f"triangles {total} under the {kind} range ({lo}-{hi}): is it too simple to read?")
    rep.check(not no_mat, f"every render primitive has a material ({no_mat or 'all do'})")
    rep.check(misses == 0, "every render primitive has NORMALs")
    rep.check(nonunit == 0, f"normals are unit length ({nonunit} are not)")
    rep.check(bad_wind == 0, f"triangle winding agrees with stored normals ({bad_wind} triangles disagree)")
    inside_out = []
    for i in parts:
        for vol, n, _ in shells(glb.primitives(i)):
            if vol <= 0:
                inside_out.append(f"{glb.names[i]} ({n} tris, volume {vol:.3g})")
    rep.check(not inside_out, f"every closed shell faces outward, positive volume ({inside_out or 'all do'})")

    # ---- flat faces shade flat (against the build's intent)
    print("\n-- flat shading (faces modelled flat must have every vertex normal = face normal)")
    if not os.path.isfile(intent_path):
        rep.check(False, f"{name}.intent.json exists (written by the build; rebuild with tools/assets/build)")
        intent = {"parts": {}}
    else:
        with open(intent_path) as f:
            intent = json.load(f)
        rep.check(intent.get("asset") == name, f"intent file is for '{name}' (says {intent.get('asset')!r})")
        listed, have = set(intent.get("parts", {})), {glb.names[i] for i in parts}
        rep.check(listed == have, f"intent file lists exactly the GLB's parts"
                  + ("" if listed == have else f" (only in intent: {sorted(listed - have)},"
                                               f" only in GLB: {sorted(have - listed)})"))
    for i in parts:
        nm = glb.names[i]
        planes = intent["parts"].get(nm, {}).get("flat_planes")
        if planes is None:
            rep.check(False, f"{nm}: intent file lists this part (stale {name}.intent.json?)")
            continue
        hit = [0] * len(planes)
        worst, worst_at = 0.0, None
        for _, P, N, idx in glb.primitives(i):
            if N is None:
                continue
            for t in range(0, len(idx), 3):
                a, b, c = P[idx[t]], P[idx[t + 1]], P[idx[t + 2]]
                gn = cross(sub(b, a), sub(c, a))
                if dot(gn, gn) < 1e-20:
                    continue
                gn = norm(gn)
                # A small triangle can lie on two modelled-flat planes a fraction of a degree
                # apart (neighbouring faces of a rock); it counts for each, and its normals are
                # judged against the closer one.
                best = None
                for k, (nx, ny, nz, d) in enumerate(planes):
                    align = dot(gn, (nx, ny, nz))
                    if align < 0.99999:
                        continue
                    if all(abs(dot(v, (nx, ny, nz)) - d) < PLANE_TOL for v in (a, b, c)):
                        hit[k] += 1
                        if best is None or align > best[0]:
                            best = (align, (nx, ny, nz))
                if best is not None:
                    for q in range(3):
                        dev = angle_deg(N[idx[t + q]], best[1])
                        if dev > worst:
                            worst, worst_at = dev, P[idx[t + q]]
        found = sum(1 for h in hit if h)
        where = "" if worst_at is None or worst < FLAT_DEG else f" at {tuple(round(v, 4) for v in worst_at)}"
        rep.check(worst < FLAT_DEG, f"{nm}: {found}/{len(planes)} flat planes, worst vertex normal {worst:.2f} deg off{where}")
        if planes and found == 0:
            rep.check(False, f"{nm}: none of its {len(planes)} modelled-flat planes is in the GLB"
                      " (inside out, moved, or a stale intent file)")
        elif found < len(planes):
            rep.warn(f"{nm}: {len(planes) - found} modelled-flat planes have no triangles left (bevel ate them?"
                     " a feature thinner than twice its bevel)")

    # ---- bounds and orientation
    print("\n-- bounds and orientation")
    size = [render_hi[k] - render_lo[k] for k in range(3)]
    print(f"  render bounds min ({render_lo[0]:+.4f}, {render_lo[1]:+.4f}, {render_lo[2]:+.4f})"
          f"  max ({render_hi[0]:+.4f}, {render_hi[1]:+.4f}, {render_hi[2]:+.4f})"
          f"  size x={size[0]:.4f} y={size[1]:.4f} z={size[2]:.4f}")
    for ax, (a, b) in exp.get("size", {}).items():
        v = size["xyz".index(ax)]
        rep.check(a <= v <= b, f"size along {ax} = {v:.4f} m, expected {a}-{b}")
    for sname, want in exp.get("sockets", {}).items():
        if not rep.check(sname in glb.index, f"expected socket '{sname}' present"):
            continue
        m = glb.world[glb.index[sname]]
        at = (m[0][3], m[1][3], m[2][3])
        if "at" in want:
            t = float(want.get("tol", tol))
            d = math.dist(at, want["at"])
            rep.check(d <= t, f"{sname} at {tuple(round(v, 4) for v in at)}, expected {want['at']} (off by {d:.4f}, tol {t})")
        for key, local in (("forward", (0, 0, -1)), ("up", (0, 1, 0))):
            if key in want:
                got = xdir(m, local)
                a = angle_deg(got, want[key])
                rep.check(a <= DIR_DEG, f"{sname} {key} {tuple(round(v, 3) for v in got)}, expected {want[key]} ({a:.1f} deg off)")

    # ---- colliders
    print("\n-- colliders")
    for i in cols:
        nm = glb.names[i]
        prims = glb.primitives(i)
        rep.check(all("material" not in p for p, _, _, _ in prims), f"{nm} has no material")
        rep.check(not glb.nodes[i].get("children"), f"{nm} has no children")
        tris = [tuple(P[idx[t + k]] for k in range(3)) for _, P, _, idx in prims for t in range(0, len(idx), 3)]
        pts = {v for tri in tris for v in tri}
        sh = shells(prims)
        closed = len(sh) == 1 and all(c == 2 for c in sh[0][2].values())
        rep.check(len(tris) >= 4 and closed and sh[0][0] > 1e-12,
                  f"{nm} is one closed hull with volume ({len(tris)} tris, {len(sh)} shell(s),"
                  f" volume {sum(v for v, _, _ in sh) * 1e6:.1f} cm3)")
        worst = 0.0
        for a, b, c in tris:
            gn = cross(sub(b, a), sub(c, a))
            if dot(gn, gn) < 1e-20:
                continue
            gn = norm(gn)
            worst = max([worst] + [dot(sub(v, a), gn) for v in pts])
        rep.check(worst <= CONVEX_TOL, f"{nm} is convex with outward faces ({glb.triangles(i)} tris,"
                  f" worst point {worst * 1000:.3f} mm outside a face)")

    return rep


def main(argv):
    if len(argv) != 2 or argv[1] in ("-h", "--help"):
        print(__doc__.strip())
        return 2
    target = argv[1]
    asset_dir = os.path.dirname(target) if target.endswith(".glb") else target
    rep = verify(asset_dir or ".")
    print()
    if rep.fails:
        print(f"VERIFY FAILED: {len(rep.fails)} check(s) failed, {len(rep.warns)} warning(s)")
        for f in rep.fails:
            print(f"  FAIL {f}")
        return 1
    print(f"VERIFY PASSED ({len(rep.warns)} warning(s))")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
