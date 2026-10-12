#!/usr/bin/env python3
"""Summaries for tools/stress.

  stress-summary.py table <run.json>...       one row per run
  stress-summary.py trace <trace.json> [N]    top N spans inside the measured window

A trace is the Chrome trace a `profile-trace` build writes to $TRACE_CHROME
(Bevy's tracing spans). Only what falls between the game's
`hb_profile_window` markers counts. Times are per frame (frames are the
"main app" spans in the window), CPU time on whatever thread ran it.
"""

import json
import re
import sys
from collections import defaultdict


def table(paths):
    runs = [json.load(open(p)) for p in paths]
    cols = [
        ("run", lambda d: d["label"], "<"),
        ("build", lambda d: d["build"] + ("+trace" if d.get("trace") else ""), "<"),
        ("present", lambda d: d.get("hullbreakers_present") or "vsync", "<"),
        ("off (A/B)", lambda d: d.get("hullbreakers_profile_ab") or "-", "<"),
        ("fps", lambda d: f'{d["fps"]:.0f}', ">"),
        ("frame mean", lambda d: f'{d["frame_ms"]["mean"]:.2f}', ">"),
        ("p50", lambda d: f'{d["frame_ms"]["p50"]:.2f}', ">"),
        ("p95", lambda d: f'{d["frame_ms"]["p95"]:.2f}', ">"),
        ("p99", lambda d: f'{d["frame_ms"]["p99"]:.2f}', ">"),
        ("fixed/frame", lambda d: f'{d["fixed_ms_per_frame"]["mean"]:.2f}', ">"),
        ("fixed/tick", lambda d: f'{d["fixed_ms_per_tick"]:.2f}', ">"),
        ("gpu*", lambda d: f'{d["gpu_ms"]["mean"]:.2f}', ">"),
        ("gpu busy%", lambda d: f'{d["gpu_busy_percent"]:.0f}' if "gpu_busy_percent" in d else "-", ">"),
        ("gpu W", lambda d: f'{d["gpu_power_watts"]:.0f}' if "gpu_power_watts" in d else "-", ">"),
        ("cpu cores", lambda d: f'{d["cpu_cores"]:.2f}', ">"),
        ("entities", lambda d: f'{d["counts"]["entities"]["mean"]:.0f}', ">"),
        ("vis meshes", lambda d: f'{d["counts"]["visible_meshes"]["mean"]:.0f}', ">"),
        ("vis tris", lambda d: f'{d["counts"]["visible_triangles"]["mean"] / 1000:.0f}k', ">"),
        ("lights p/s", lambda d: f'{d["counts"]["point_lights"]["mean"]:.0f}/{d["counts"]["spot_lights"]["mean"]:.0f}', ">"),
        ("bodies/colliders", lambda d: f'{d["counts"]["rigid_bodies"]["mean"]:.0f}/{d["counts"]["colliders"]["mean"]:.0f}', ">"),
        ("robots", lambda d: f'{d["counts"]["robots"]["mean"]:.0f}', ">"),
        ("tracers", lambda d: f'{d["counts"]["tracers"]["mean"]:.0f}', ">"),
        ("sparks", lambda d: f'{d["counts"]["bullet_sparks"]["mean"] + d["counts"]["hull_sparks"]["mean"]:.0f}', ">"),
        ("sent kB/s", lambda d: "/".join(f'{c["kbytes_per_s"]:.1f}' for c in d.get("sent_per_client", [])) or "-", ">"),
    ]
    rows = [[c[0] for c in cols]] + [[c[1](d) for c in cols] for d in runs]
    widths = [max(len(r[i]) for r in rows) for i in range(len(cols))]
    for n, r in enumerate(rows):
        print("  ".join(f"{v:{cols[i][2]}{widths[i]}}" for i, v in enumerate(r)))
        if n == 0:
            print("  ".join("-" * w for w in widths))
    print("\nframe/fixed in ms; fixed = RunFixedMainLoop (lightyear + sim + physics) per frame;")
    print("gpu* = sum of Bevy's instrumented GPU passes (shadow passes are not instrumented);")
    print("gpu busy% = the GPU's own utilisation (whole card, includes anything else on it);")
    print("cpu cores = process CPU time / wall time; sent = an authority's payload bytes to each remote client.")


def name_of(raw):
    """Span name, normalised: `system: name="a::b"` -> `a::b`."""
    m = re.match(r'(system|system_commands|schedule|sub app): name="?([^"]*)"?', raw)
    if m:
        kind, name = m.groups()
        return {"system": "sys", "system_commands": "cmds", "schedule": "schedule", "sub app": "sub app"}[kind], name
    m = re.match(r"RenderContextState::apply: system=(.*)", raw)
    if m:
        return "render apply", m.group(1)
    return "span", raw.rstrip(": ")


def trace(path, top=25):
    window = []  # [start, end] timestamps (µs)
    # First pass: find the window markers (cheap: grep-like).
    with open(path) as f:
        for line in f:
            if "hb_profile_window" in line and '"ph":"B"' in line:
                window.append(json.loads(line.rstrip().rstrip(","))["ts"])
    if len(window) < 2:
        sys.exit(f"{path}: no hb_profile_window markers (did the run finish?)")
    start, end = window[0], window[1]
    stacks = defaultdict(list)
    totals = defaultdict(float)
    counts = defaultdict(int)
    frames = 0
    with open(path) as f:
        for line in f:
            if '"ph":"B"' not in line and '"ph":"E"' not in line:
                continue
            try:
                e = json.loads(line.rstrip().rstrip(","))
            except json.JSONDecodeError:
                continue
            tid = e["tid"]
            if e["ph"] == "B":
                stacks[tid].append((e["name"], e["ts"]))
                continue
            if not stacks[tid]:
                continue
            name, began = stacks[tid].pop()
            a, b = max(began, start), min(e["ts"], end)
            if b <= a:
                continue
            key = name_of(name)
            totals[key] += b - a
            counts[key] += 1
            if name.startswith("main app") and began >= start:
                frames += 1
    frames = max(frames, 1)
    seconds = (end - start) / 1e6
    print(f"window {seconds:.1f} s, {frames} frames ({frames / seconds:.0f} fps); ms per frame (CPU, any thread)")

    def show(title, kinds, n):
        rows = sorted(((v, k) for k, v in totals.items() if k[0] in kinds), reverse=True)[:n]
        print(f"\n{title}")
        for v, (kind, name) in rows:
            calls = counts[(kind, name)] / frames
            print(f"  {v / 1000 / frames:8.3f}  {calls:6.1f}/frame  [{kind}] {name}")

    show("Frame structure", {"span", "sub app"}, 12)
    show("Schedules", {"schedule"}, 15)
    show(f"Top {top} systems (incl. their command application, render graph nodes)", {"sys", "cmds", "render apply"}, top)


if __name__ == "__main__":
    if len(sys.argv) >= 3 and sys.argv[1] == "table":
        table(sys.argv[2:])
    elif len(sys.argv) >= 3 and sys.argv[1] == "trace":
        trace(sys.argv[2], int(sys.argv[3]) if len(sys.argv) > 3 else 25)
    else:
        sys.exit(__doc__)
