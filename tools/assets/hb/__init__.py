"""hb: the Hullbreakers asset library, for Blender build scripts. See docs/assets.md.

A build script (assets/<group>/<name>/build.py) is run by tools/assets/build, never
directly. All coordinates are BEVY space, metres: +Y up, -Z forward, +X right.

    import hb

    a = hb.Asset(__file__)                          # reads asset.json (palette names paint_primary)
    body = a.part("body", pivot=(0, 0, 0))          # parent defaults to the root
    body.box((-0.1, 0, -0.2), (0.1, 0.1, 0.2), mat="paint_primary")
    a.socket("SOCKET_top", body, at=(0, 0.1, 0))
    a.collider("COL_body", around=[body])
    a.export()                                      # finish, export, verify-ready
"""
import bpy

from .asset import Asset, Part, Node            # noqa: F401
from .export import ExportError                  # noqa: F401

if bpy.app.version[:2] != (5, 2):
    print(f"hb: WARNING: built against Blender 5.2, running {bpy.app.version_string};"
          " check docs/assets.md 'Gotchas' for API drift")
