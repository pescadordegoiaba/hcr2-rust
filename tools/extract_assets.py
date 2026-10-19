#!/usr/bin/env python3
"""Copy the art the port actually draws out of the extracted XAPK.

Everything here comes from `xapk_re/base/assets` (the plain files inside
`com.fingersoft.hcr2.apk`). The JSON and .rube files in there are encrypted, but
the PNG atlases are not, so the art can be used directly:

  Common/textures/cars/<id>/<rarity>/<skin>/car_<id>-<part>.png
      chassis, wheel, chaintrack, gun, ... one file per part.
  HD/game.png + HD/game.plist
      the in-game HUD atlas: fuel and rpm meters, gear digits, pedals.
  HD/textures/terrain/<family>_terrain_*.png
      the ground surface each level is painted with.

Output lands in `climb/assets/` so the page is self contained and the wasm build
does not depend on the XAPK being present.

Usage:  python3 tools/extract_assets.py [path/to/game.xapk | extracted/assets]

With no argument it looks for the dump the project was developed against.
"""

from __future__ import annotations

import json
import plistlib
import re
import shutil
import struct
import sys
import tempfile
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent          # climb/
OUT = ROOT / "assets"


def _assets_root(path: Path) -> Path | None:
    """Find the folder that holds `HD/` inside an extracted package."""
    for cand in (path, path / "assets", path / "base" / "assets"):
        if (cand / "HD").is_dir():
            return cand
    return None


def _unzip(archive: Path, into: Path) -> Path:
    with zipfile.ZipFile(archive) as zf:
        zf.extractall(into)
    return into


def resolve_assets() -> Path:
    """Locate `assets/` from a path argument, an XAPK or the dev dump."""
    if len(sys.argv) < 2:
        return ROOT.parents[1] / "xapk_re" / "base" / "assets"   # climb/ -> inpiracao/ -> hc2rea/

    given = Path(sys.argv[1]).expanduser().resolve()
    if given.is_dir():
        found = _assets_root(given)
        if found:
            return found
        raise SystemExit(f"no HD/ folder under {given}")

    if not given.is_file():
        raise SystemExit(f"not found: {given}")

    # An XAPK is a zip of APKs: unpack it, then unpack the base APK inside.
    tmp = Path(tempfile.mkdtemp(prefix="hcr2-assets-"))
    _unzip(given, tmp)
    found = _assets_root(tmp)
    if found:
        return found
    apks = sorted(tmp.glob("*.apk"))
    if not apks:
        raise SystemExit(f"no .apk inside {given}")
    inner = Path(tempfile.mkdtemp(prefix="hcr2-base-"))
    _unzip(apks[0], inner)
    found = _assets_root(inner)
    if not found:
        raise SystemExit(f"no assets/HD inside {apks[0].name}")
    return found


MINI = resolve_assets()

# Level family -> ground texture stem, taken from HD/textures/terrain.
GROUND = {
    "countryside": "terrain-ground-forest",
    "forest": "terrain-ground-forest",
    "dark": "terrain-ground-forest",
    "winter": "terrain-ground-forest",
    "glacier": "terrain-ground_mountain",
    "mountain": "terrain-ground_mountain",
    "nationalpark": "terrain-ground_mountain",
    "moon": "terrain-ground_moon",
    "mine": "terrain-ground_mine",
    "workshop": "terrain-ground_mine",
    "trailerpark": "terrain-ground-bayou",
    "savanna": "terrain-ground_forest",
    "city": "terrain-ground-cliff-cuptown",
    "cuptown": "terrain-ground-cliff-cuptown",
    "underwater": "terrain-ground_moon",
}
DEFAULT_GROUND = "terrain-ground-forest"

# HUD frames the port draws, copied one by one out of the game atlas so the
# runtime can address them by the same names the original uses.
HUD_FRAMES = [
    "game/meter-fuel.png",
    "game/meter-rpm.png",
    "game/meter-rpm-start.png",
    "game/meter-boost.png",
    "game/gear_indicator.png",
    "game/1-gear.png", "game/2-gear.png", "game/3-gear.png", "game/R-gear.png",
    "game/pedal-gas-normal.png", "game/pedal-gas-pressed.png",
    "game/pedal-brake-normal.png", "game/pedal-brake-pressed.png",
    "game/bolt/meter-fuel.png",
    "game/bolt/meter-rpm.png",
    "game/bolt/fuel-arrow.png",
]


def roster_ids() -> list[str]:
    """The real vehicle ids, read straight from the generated Rust table."""
    source = (ROOT / "src" / "vehicle_data.rs").read_text()
    return re.findall(r'real_id: "([a-z0-9_]+)"', source)


def png_size(path: Path) -> tuple[int, int]:
    """Width and height straight out of the IHDR chunk."""
    header = path.read_bytes()[:33]
    return struct.unpack(">II", header[16:24])


def pick_part(parts: dict[str, str], *needles: str) -> str | None:
    """First part whose name contains every needle, longest name first."""
    for needle in needles:
        hits = [name for part, name in parts.items() if needle in part]
        if hits:
            return sorted(hits, key=len)[0]
    return None


def best_skin(car_dir: Path) -> Path | None:
    """Prefer the stock skin, then whatever skin exists first."""
    stock = car_dir / "default" / "default"
    if stock.is_dir():
        return stock
    for rarity in sorted(car_dir.glob("*/*")):
        if rarity.is_dir() and any(rarity.glob("*.png")):
            return rarity
    return None


def copy_vehicles() -> list[dict]:
    cars_root = MINI / "HD" / "textures" / "cars"
    manifest = []
    for car_id in roster_ids():
        skin = best_skin(cars_root / car_id) if (cars_root / car_id).is_dir() else None
        if skin is None:
            print(f"  {car_id:<14} missing")
            continue
        target = OUT / "cars" / car_id
        target.mkdir(parents=True, exist_ok=True)
        parts = {}
        for png in sorted(skin.glob("*.png")):
            if png.name.startswith("icon"):
                continue  # garage/thumbnail art, not drawn in game
            part = png.stem.split("-", 1)[1] if "-" in png.stem else png.stem
            shutil.copy2(png, target / png.name)
            parts[part] = png.name
        manifest.append({"id": car_id, "skin": str(skin.relative_to(cars_root)), "parts": parts})
        chassis = pick_part(parts, "chassis", "body")
        tire = pick_part(parts, "wheel", "tire")
        entry = {"id": car_id, "skin": str(skin.relative_to(cars_root)), "parts": parts}
        if chassis:
            w, h = png_size(target / chassis)
            entry.update(chassis=chassis, chassis_px=[w, h])
        if tire:
            w, h = png_size(target / tire)
            entry.update(tire=tire, tire_px=[w, h])
        manifest[-1] = entry
        print(f"  {car_id:<14} {len(parts)} parts  chassis={chassis} tire={tire}")
    # One shared tire for every vehicle that ships its wheels inside an atlas.
    generic = cars_root / "jeep-tire.png"
    if generic.is_file():
        target = OUT / "cars"
        target.mkdir(parents=True, exist_ok=True)
        shutil.copy2(generic, target / "tire-default.png")
        print(f"  shared tire: {png_size(generic)}")
    return manifest


def copy_atlas() -> dict:
    """Slice the HUD atlas into individual PNGs plus a name -> rect table."""
    png = MINI / "HD" / "game.png"
    plist = MINI / "HD" / "game.plist"
    target = OUT / "hud"
    target.mkdir(parents=True, exist_ok=True)
    shutil.copy2(png, target / "game.png")

    frames = plistlib.loads(plist.read_bytes()).get("frames", {})
    rects = {}
    for name, entry in frames.items():
        frame = entry.get("frame") or entry.get("textureRect")
        if isinstance(frame, str):
            # Cocos writes "{{x,y},{w,h}}"; pull the four numbers out of it.
            values = [float(v) for v in re.findall(r"-?\d+(?:\.\d+)?", frame)]
            x, y, w, h = values[:4]
        else:
            x, y, w, h = frame
        source = entry.get("sourceSize") or [w, h]
        rotated = bool(entry.get("rotated"))
        rects[name] = {
            "x": x, "y": y, "w": w, "h": h,
            "ox": (source[0] - w) / 2.0, "oy": (source[1] - h) / 2.0,
            "sw": source[0], "sh": source[1], "rotated": rotated,
        }
    (target / "game.json").write_text(json.dumps(rects, indent=1))
    print(f"  HUD atlas: {len(rects)} frames, sheet {png.stat().st_size} bytes")
    return rects


def copy_ground() -> dict:
    source = MINI / "HD" / "textures" / "terrain"
    target = OUT / "ground"
    target.mkdir(parents=True, exist_ok=True)
    used = {}
    for family, stem in GROUND.items():
        png = source / f"{stem}.png"
        if not png.is_file():
            print(f"  ground {family:<12} {stem} missing")
            continue
        shutil.copy2(png, target / png.name)
        used[family] = png.name
    # Anything not mapped falls back to the default surface.
    if DEFAULT_GROUND:
        fallback = source / f"{DEFAULT_GROUND}.png"
        if fallback.is_file():
            shutil.copy2(fallback, target / fallback.name)
            used["_default"] = fallback.name
    print(f"  ground textures: {len(used)}")
    return used


def main() -> None:
    if not MINI.is_dir():
        raise SystemExit(f"extracted XAPK not found at {MINI}")
    OUT.mkdir(parents=True, exist_ok=True)
    print("vehicles:")
    cars = copy_vehicles()
    print("hud:")
    rects = copy_atlas()
    print("ground:")
    ground = copy_ground()
    (OUT / "manifest.json").write_text(json.dumps(
        {"cars": cars, "hud": rects, "ground": ground}, indent=1))
    print(f"\n-> {OUT}")


if __name__ == "__main__":
    main()
