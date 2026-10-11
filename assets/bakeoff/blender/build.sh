#!/usr/bin/env bash
# Rebuild smg.glb + preview renders from scratch, then verify the .glb independently.
set -euo pipefail
cd "$(dirname "$0")"
rm -f smg.glb preview/*.png
blender --background --factory-startup --python build_smg.py -- --render
python3 -I verify_glb.py smg.glb
