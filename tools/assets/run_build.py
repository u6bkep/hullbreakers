"""Blender entry point for one asset build. tools/assets/build runs this; by hand:

    blender --background --factory-startup --python-exit-code 1 \
        --python tools/assets/run_build.py -- assets/weapons/smg_receiver [--preview]

--python-exit-code 1 matters: without it Blender exits 0 after a Python exception.
"""
import os
import runpy
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.stdout.reconfigure(line_buffering=True)   # keep hb lines in order with tracebacks
sys.path.insert(0, HERE)        # hb, verify_glb (Blender ignores PYTHONPATH)

import hb  # noqa: E402
from hb import _state  # noqa: E402

args = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else []
dirs = [a for a in args if not a.startswith("--")]
if len(dirs) != 1:
    raise SystemExit("run_build.py: give exactly one asset directory after --")
_state.preview = "--preview" in args
script = os.path.join(os.path.abspath(dirs[0]), "build.py")
if not os.path.isfile(script):
    raise SystemExit(f"run_build.py: {script} not found")

runpy.run_path(script, run_name="__main__")

if _state.current is None:
    raise SystemExit(f"{script} never made an hb.Asset")
if _state.exported is None:
    raise SystemExit(f"{script} finished without calling export(); nothing was written")
print("hb: BUILD OK")
