#!/usr/bin/env python3
"""Writes the part variant assets: assets/weapons/parts/<frame>_<slot>_<variant>/ with
its asset.json (from the table below: what it is, the palette roles it paints, where
its sockets are) and a build.py that hands over to gunparts.py. Run it after changing
the table, then `tools/assets/build weapons/parts/...`. The variants are the part
enums in src/sim/items.rs (Barrel, Magazine, Stock, Sight, Muzzle); a bare muzzle is
no part at all.

    python3 -I assets/weapons/parts/make_parts.py
"""
import json
import os

HERE = os.path.dirname(os.path.abspath(__file__))

PALETTE = {
    "smg": {"paint_primary": "#c46c2c", "paint_secondary": "#3d4650", "emissive": "#6cedff"},
    "rifle": {"paint_primary": "#d6cfba", "paint_secondary": "#3b4658", "emissive": "#c2ff4d"},
}

# (slot, variant): (brief, palette roles used, {frame: muzzle z} for barrels)
P, A, G = "paint_primary", "paint_secondary", "emissive"
TABLE = {
    "smg": {
        ("barrel", "standard"): ("The issue shroud: collar, finned jacket, crowned tip.", [], -0.14),
        ("barrel", "long"): ("A slim jacket run well out, vented along its top.", [P], -0.292),
        ("barrel", "short"): ("A stub barely past the receiver.", [], -0.06),
        ("barrel", "heavy"): ("Thick, with heat-sink fins all round it.", [P], -0.152),
        ("muzzle", "compensator"): ("A block with ports cut across its top.", [], None),
        ("muzzle", "booster"): ("A can with grip rings, flaring at its end.", [P], None),
        ("mag", "standard"): ("A straight box mag with a count window.", [G], None),
        ("mag", "extended"): ("Nearly twice as long, raked forward.", [G], None),
        ("mag", "drum"): ("A short neck into a fat drum.", [P, G], None),
        ("mag", "quick"): ("Two mags taped side by side, with a pull loop.", [A, G], None),
        ("stock", "standard"): ("A folding wire stock.", [], None),
        ("stock", "light"): ("An end cap with a sling loop: next to no stock.", [], None),
        ("stock", "heavy"): ("A solid stock with a cheek rest and a thick pad.", [P, A], None),
        ("sight", "iron"): ("A notched rear sight and a front post with ears.", [], None),
        ("sight", "reflex"): ("An open window with a glowing dot.", [P, G], None),
        ("sight", "scope"): ("A tube scope with a big objective bell.", [P, G], None),
    },
    "rifle": {
        ("barrel", "standard"): ("The issue barrel, crowned.", [], -0.118),
        ("barrel", "long"): ("Long and thin, with a gas block.", [P], -0.315),
        ("barrel", "short"): ("Hardly out of the handguard.", [], -0.048),
        ("barrel", "heavy"): ("A fluted bull barrel with a heat collar.", [P], -0.145),
        ("muzzle", "compensator"): ("A block with ports cut across its top.", [], None),
        ("muzzle", "booster"): ("A can with grip rings, flaring at its end.", [P], None),
        ("mag", "standard"): ("The issue curved 30-round mag.", [G], None),
        ("mag", "extended"): ("The curve carried on: a banana mag.", [G], None),
        ("mag", "drum"): ("A short neck into a fat drum.", [P, G], None),
        ("mag", "quick"): ("Two mags taped side by side, with a pull loop.", [A], None),
        ("stock", "standard"): ("The skeletal issue stock.", [A], None),
        ("stock", "light"): ("A bare telescoping tube and a small pad.", [], None),
        ("stock", "heavy"): ("Full and solid, with a raised cheek piece.", [P, A], None),
        ("sight", "iron"): ("A notched rear sight and a front post with ears.", [], None),
        ("sight", "reflex"): ("An open window with a glowing dot.", [P, G], None),
        ("sight", "scope"): ("A tube scope with a big objective bell.", [P, G], None),
    },
}

BUILD = '''"""{brief} (A part variant: the geometry is in ../gunparts.py.)"""
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
import gunparts  # noqa: E402

gunparts.build(__file__)
'''


def main():
    for frame, parts in TABLE.items():
        for (slot, variant), (brief, roles, muzzle) in parts.items():
            name = f"{frame}_{slot}_{variant}"
            d = os.path.join(HERE, name)
            os.makedirs(d, exist_ok=True)
            expect = {"parts": [slot], "colliders": ["COL_part"], "sockets": {}, "tolerance": 0.002}
            if muzzle is not None:
                expect["sockets"]["SOCKET_muzzle"] = {"at": [0, 0, muzzle], "forward": [0, 0, -1]}
            asset = {
                "name": name,
                "kind": "weapon_part",
                "brief": f"{frame.upper() if frame == 'smg' else frame.capitalize()} {slot}, {variant}: {brief}",
                "palette": {r: PALETTE[frame][r] for r in roles},
                "expect": expect,
            }
            with open(os.path.join(d, "asset.json"), "w") as f:
                f.write(json.dumps(asset, indent=2) + "\n")
            with open(os.path.join(d, "build.py"), "w") as f:
                f.write(BUILD.format(brief=brief))
    print(f"wrote {sum(len(p) for p in TABLE.values())} part assets")


if __name__ == "__main__":
    main()
