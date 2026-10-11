#!/usr/bin/env bash
# One-command rebuild: venv (if missing) -> model+export -> postprocess -> verify -> previews.
# Usage: ./build.sh            (env: SCRATCH=<dir for venv + intermediates>, UV=<uv binary>)
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
SCRATCH="${SCRATCH:-/tmp/claude-1000/-home-ben-Documents-programmingPlay-space-game/7c5c38f8-cffe-497d-bcfd-61a3437058b7/scratchpad/build123d}"
UV="${UV:-$(command -v uv || echo /home/ben/.config/Code/User/globalStorage/atopile.atopile/uv/uv)}"
PY="$SCRATCH/.venv/bin/python"
if [ ! -x "$PY" ]; then
  mkdir -p "$SCRATCH"
  "$UV" venv -p 3.12 "$SCRATCH/.venv"
  "$UV" pip install -p "$PY" -r "$HERE/requirements.txt"
fi
WORK="$SCRATCH/work"
rm -rf "$WORK"; mkdir -p "$WORK"
"$PY" "$HERE/build_smg.py" "$WORK"
"$PY" "$HERE/postprocess.py" "$WORK" "$HERE/smg.glb"
"$PY" "$HERE/verify.py" "$HERE/smg.glb"
rm -rf "$HERE/preview"
PYOPENGL_PLATFORM=egl "$PY" "$HERE/preview.py" "$HERE/smg.glb" "$HERE/preview"
