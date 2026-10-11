"""Bevy space <-> Blender space.

Everything an asset script passes to hb is in BEVY space, in metres: +Y up, -Z forward,
+X right, the same axes the .glb has and the game uses. Blender is Z-up; the glTF
exporter (export_yup) maps Blender (x, y, z) -> glTF (x, z, -y). So a Bevy point
(x, y, z) is the Blender point (x, -z, y). Only this module knows that.
"""
from mathutils import Matrix, Vector

# Bevy -> Blender, a proper rotation (det +1), so face winding is preserved.
B_FROM_G = Matrix(((1.0, 0.0, 0.0), (0.0, 0.0, -1.0), (0.0, 1.0, 0.0)))
G_FROM_B = B_FROM_G.transposed()
B_FROM_G4 = B_FROM_G.to_4x4()
G_FROM_B4 = G_FROM_B.to_4x4()

AXES = "xyz"


def vec3(v, what="point"):
    try:
        x, y, z = (float(c) for c in v)
    except (TypeError, ValueError):
        raise ValueError(f"{what} must be three numbers (x, y, z) in metres, got {v!r}") from None
    return Vector((x, y, z))


def to_blender(p):
    """Bevy point/direction -> Blender."""
    return B_FROM_G @ Vector(p)


def to_bevy(p):
    return G_FROM_B @ Vector(p)


def matrix_to_blender(m_g):
    """A 4x4 transform in Bevy space -> the same transform in Blender space."""
    return B_FROM_G4 @ m_g @ G_FROM_B4


def frame(at, forward=None, up=None, inherit=None):
    """A Bevy-space 4x4 transform: origin `at`, local -Z along `forward`, local +Y toward `up`.

    With no forward/up the rotation is `inherit` (a 3x3, the parent's world rotation),
    or identity. With only one given, the other defaults to the WORLD axis (-Z / +Y), so a
    forward along +/-Y needs an explicit up. This is Bevy's Transform::looking_to convention.
    """
    at = vec3(at, "location")
    if forward is None and up is None:
        rot = inherit.copy() if inherit is not None else Matrix.Identity(3)
    else:
        f = vec3(forward if forward is not None else (0, 0, -1), "forward")
        u = vec3(up if up is not None else (0, 1, 0), "up")
        if f.length < 1e-9 or u.length < 1e-9:
            raise ValueError("forward and up must be non-zero")
        z = -f.normalized()
        x = u.cross(z)
        if x.length < 1e-6:
            raise ValueError(f"forward {tuple(f)} and up {tuple(u)} are parallel; pass an `up` that isn't")
        x.normalize()
        y = z.cross(x)
        rot = Matrix((x, y, z)).transposed()
    m = rot.to_4x4()
    m.translation = at
    return m


def axis_index(letter, what="axis"):
    if letter not in AXES:
        raise ValueError(f"{what} must be one of 'x', 'y', 'z', got {letter!r}")
    return AXES.index(letter)
