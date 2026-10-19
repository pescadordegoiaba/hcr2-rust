#!/usr/bin/env python3
"""Bake the game's own terrain into `src/level_data.rs`.

Every level definition in `analysis/jsons_decrypted/json/levels/*.json` carries
an `algorithm` block whose `expr` is the function Hill Climb Racing 2 evaluates
to build the ground:

    var y := 0.0; y += cos(0.1 + x/1.7)*0.6; ... y *= 0.35;
    y *= clamp(0.0, x/10, 1.0); y += 2.0 - 2.0*clamp(0.0, x/10, 1.0);

The expression language is a tiny imperative script: `var NAME := expr`,
`NAME += expr`, `NAME *= expr`, `;` separated statements, and the functions
cos, sin, abs, min, max, pow, ceil, floor, sqrt and clamp(min, value, max).
`x` may be reassigned, which several levels do to bend the profile.

This script implements that language (no `eval`/`exec` — tokenizer plus
recursive descent), samples each chosen level into an (x, y) polyline and writes
the table the Rust build consumes. Sampling here instead of at runtime keeps the
wasm small and makes the numbers reproducible: run it twice, get the same file.

Usage:  python3 tools/extract_levels.py
"""

from __future__ import annotations

import json
import math
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
# climb/ -> inpiracao/ -> hc2rea/ (where the decrypted dump lives)
DUMP = ROOT.parents[1] / "workspace-96625685-d7cf-4d3f-a70e-523b0bb426e9 (2)" / "analysis"
LEVELS_JSON = DUMP / "jsons_decrypted" / "json" / "levels"
OUT_RS = ROOT / "src" / "level_data.rs"
OUT_JSON = DUMP / "level_specs.json"

# Sampling resolution in metres. 0.5 m keeps every dumped frequency
# (the fastest term is x/0.5, a 3.1 m wavelength) well resolved.
STEP = 0.5
# How much track to bake per level. Long (endless) levels get this much.
ENDLESS_LENGTH = 3000.0

TOKEN = re.compile(r"\s*(?:(\d+\.?\d*(?:[eE][-+]?\d+)?)|([A-Za-z_][A-Za-z_0-9]*)|(:=|[-+*/]=|[-+*/(),;^%]))")

FUNCS = {
    "cos": math.cos, "sin": math.sin, "tan": math.tan,
    "abs": abs, "sqrt": math.sqrt, "ceil": math.ceil, "floor": math.floor,
    "min": min, "max": max, "pow": math.pow,
}


def clamp(lo, value, hi):
    """The game's clamp takes (min, value, max) in that order."""
    return max(lo, min(hi, value))


FUNCS["clamp"] = clamp


def tokenize(text: str) -> list[str]:
    tokens, pos = [], 0
    text = text.strip()
    while pos < len(text):
        match = TOKEN.match(text, pos)
        if not match:
            raise ValueError(f"cannot tokenize at {pos}: {text[pos:pos + 24]!r}")
        number, name, symbol = match.groups()
        tokens.append(number or name or symbol)
        pos = match.end()
    return tokens


class Parser:
    """Recursive descent over the terrain expression language."""

    def __init__(self, tokens: list[str]):
        self.tokens = tokens
        self.i = 0

    def peek(self) -> str | None:
        return self.tokens[self.i] if self.i < len(self.tokens) else None

    def take(self, expected: str | None = None) -> str:
        token = self.peek()
        if token is None:
            raise ValueError("unexpected end of expression")
        if expected is not None and token != expected:
            raise ValueError(f"expected {expected!r}, got {token!r}")
        self.i += 1
        return token

    def parse_program(self):
        statements = []
        while self.peek() is not None:
            if self.peek() == ";":
                self.take()
                continue
            statements.append(self.parse_statement())
            if self.peek() == ";":
                self.take()
        return statements

    def parse_statement(self):
        if self.peek() == "var":
            self.take()
            name = self.take()
            self.take(":=")
            return ("set", name, self.parse_expr())
        # NAME op= expr  (op is one of += -= *= /=) or a bare assignment
        if self.i + 1 < len(self.tokens) and self.tokens[self.i + 1] in ("+=", "-=", "*=", "/="):
            name = self.take()
            op = self.take()[0]
            return ("acc", name, op, self.parse_expr())
        if self.i + 1 < len(self.tokens) and self.tokens[self.i + 1] == ":=":
            name = self.take()
            self.take(":=")
            return ("set", name, self.parse_expr())
        return ("expr", self.parse_expr())

    def parse_expr(self):
        return self.parse_add()

    def parse_add(self):
        node = self.parse_mul()
        while self.peek() in ("+", "-"):
            op = self.take()
            node = ("bin", op, node, self.parse_mul())
        return node

    def parse_mul(self):
        node = self.parse_unary()
        while self.peek() in ("*", "/", "%"):
            op = self.take()
            node = ("bin", op, node, self.parse_unary())
        return node

    def parse_pow(self):
        # Right associative, binds tighter than * and /.
        node = self.parse_atom()
        if self.peek() == "^":
            self.take()
            node = ("bin", "^", node, self.parse_pow())
        return node

    def parse_unary(self):
        if self.peek() == "-":
            self.take()
            return ("neg", self.parse_unary())
        if self.peek() == "+":
            self.take()
            return self.parse_unary()
        return self.parse_pow()

    def parse_atom(self):
        token = self.peek()
        if token == "(":
            self.take()
            node = self.parse_expr()
            self.take(")")
            return node
        if token is None:
            raise ValueError("unexpected end of expression")
        if re.fullmatch(r"\d+\.?\d*(?:[eE][-+]?\d+)?", token):
            self.take()
            return ("num", float(token))
        self.take()
        if self.peek() == "(":
            self.take()
            args = []
            if self.peek() != ")":
                args.append(self.parse_expr())
                while self.peek() == ",":
                    self.take()
                    args.append(self.parse_expr())
            self.take(")")
            return ("call", token, args)
        return ("var", token)


def evaluate(node, env: dict[str, float]) -> float:
    kind = node[0]
    if kind == "num":
        return node[1]
    if kind == "var":
        return env[node[1]]
    if kind == "neg":
        return -evaluate(node[1], env)
    if kind == "bin":
        left = evaluate(node[2], env)
        right = evaluate(node[3], env)
        op = node[1]
        # if/elif, not a dict: a dict literal would evaluate every operator,
        # so a stray `^` would overflow even on a plain addition.
        if op == "+":
            return left + right
        if op == "-":
            return left - right
        if op == "*":
            return left * right
        if op == "/":
            return left / right if right else 0.0
        if op == "%":
            return math.fmod(left, right) if right else 0.0
        # C powf returns inf/nan instead of raising; mirror that and let the
        # caller drop non-finite samples.
        try:
            value = math.pow(left, right)
        except (ValueError, OverflowError):
            return 0.0
        return value if math.isfinite(value) else 0.0
    if kind == "call":
        func = FUNCS.get(node[1])
        if func is None:
            raise ValueError(f"unknown function {node[1]!r}")
        return func(*[evaluate(arg, env) for arg in node[2]])
    raise ValueError(f"bad node {kind}")


def compile_expr(text: str):
    statements = Parser(tokenize(text)).parse_program()

    def run(x: float) -> float:
        env = {"x": x, "y": 0.0}
        for statement in statements:
            if statement[0] == "set":
                env[statement[1]] = evaluate(statement[2], env)
            elif statement[0] == "acc":
                _, name, op, expr = statement
                value = evaluate(expr, env)
                current = env.get(name, 0.0)
                env[name] = {"+": current + value, "-": current - value,
                             "*": current * value,
                             "/": current / value if value else 0.0}[op]
            else:
                env["y"] = evaluate(statement[1], env)
        return env.get("y", 0.0)

    return run


def sample(text: str, length: float) -> list[float]:
    run = compile_expr(text)
    points: list[float] = []
    x = 0.0
    while x <= length:
        points.extend((round(x, 3), round(run(x), 4)))
        x += STEP
    return points


def decimate(points: list[float], tolerance: float = 0.05) -> list[float]:
    """Douglas-Peucker on the (x, y) pairs, flattened back to [x0, y0, x1, ...]."""
    pairs = [(points[i], points[i + 1]) for i in range(0, len(points), 2)]

    def simplify(segment: list[tuple[float, float]]) -> list[tuple[float, float]]:
        (x0, y0), (x1, y1) = segment[0], segment[-1]
        dx, dy = x1 - x0, y1 - y0
        base = math.hypot(dx, dy) or 1e-9
        worst, worst_at = -1.0, 0
        for i, (px, py) in enumerate(segment[1:-1], start=1):
            distance = abs(dy * (px - x0) - dx * (py - y0)) / base
            if distance > worst:
                worst, worst_at = distance, i
        if worst <= tolerance:
            return [segment[0], segment[-1]]
        left = simplify(segment[:worst_at + 1])
        right = simplify(segment[worst_at:])
        return left[:-1] + right

    kept = simplify(pairs)
    flat: list[float] = []
    for x, y in kept:
        flat.extend((x, y))
    return flat


def pick_levels() -> list[dict]:
    """One representative level per family: prefer distance, then longest expr."""
    best: dict[str, dict] = {}
    for path in sorted(LEVELS_JSON.glob("*.json")):
        try:
            data = json.loads(path.read_text())
        except (json.JSONDecodeError, UnicodeDecodeError):
            continue
        expr = ((data.get("algorithm") or {}).get("expr") or "").strip()
        level_id = data.get("id") or ""
        family = level_id.split("_")[0].split("0")[0].strip("_") or level_id
        if len(expr) < 100:
            continue  # block-composed track: no analytic profile to bake
        gravity = data.get("gravity") or [0.0, -10.0]
        if not isinstance(gravity, list) or len(gravity) != 2:
            continue
        distance = data.get("levelDistance")
        if isinstance(distance, dict):
            distance = distance.get("value")
        endless = str(data.get("levelType")) == "distance"
        entry = {
            "id": level_id,
            "file": path.name,
            "family": family,
            "expr": expr,
            "gravity": -abs(float(gravity[1])),
            "distance": float(distance or 0.0),
            "endless": endless,
            "fuel_density": float(data.get("fuelDensity") or 3.0),
            "fuel_density_increase": float(data.get("fuelDensityIncrease") or 0.3),
            "stars": [float(v) for v in (data.get("starCriteria") or [0.0, 0.0, 0.0])],
            "coin_multiplier": float(data.get("coinMultiplier") or 1.0),
            "score": (2 if endless else 1, len(expr)),
        }
        current = best.get(family)
        if current is None or entry["score"] > current["score"]:
            best[family] = entry
    return sorted(best.values(), key=lambda e: (-e["score"][0], -e["score"][1]))


def main() -> None:
    levels = pick_levels()
    if not levels:
        raise SystemExit(f"no usable level definitions under {LEVELS_JSON}")

    baked = []
    for level in levels:
        length = ENDLESS_LENGTH if level["endless"] else max(level["distance"], 300.0)
        points = decimate(sample(level["expr"], length))
        level["length"] = length
        level["points"] = points
        baked.append(level)
        print(f"{level['id']:<28} family={level['family']:<12} "
              f"{len(points) // 2:>5} pts  {length:>6.0f} m  g={level['gravity']}")

    write_rust(baked)
    write_report(baked)
    total = sum(len(b["points"]) for b in baked)
    print(f"\n{len(baked)} levels, {total // 2} points -> {OUT_RS}")


def rust_f32(value: float) -> str:
    """A float literal Rust accepts: `0` and `-0` are not f32."""
    text = f"{value:g}"
    if "." not in text and "e" not in text and "E" not in text:
        text += ".0"
    return text


def write_rust(levels: list[dict]) -> None:
    lines = [
        "// @generated by climb/tools/extract_levels.py — do not edit by hand.",
        "//",
        "// Source: analysis/jsons_decrypted/json/levels/*.json",
        "//         field `algorithm.expr`, evaluated by the terrains expression",
        "//         language (cos/sin/clamp/... over x) and sampled every 0.5 m.",
        "//",
        "// Each track is a flat (x, y) array: pairs of f32, ascending in x.",
        "",
        "pub struct LevelSpec {",
        "    pub id: &'static str,",
        "    pub family: &'static str,",
        "    pub gravity: f32,",
        "    pub distance: f32,",
        "    pub track_length: f32,",
        "    pub endless: bool,",
        "    pub fuel_density: f32,",
        "    pub fuel_density_increase: f32,",
        "    pub stars: [f32; 3],",
        "    pub coin_multiplier: f32,",
        "    pub track: &'static [f32],",
        "}",
        "",
    ]
    for index, level in enumerate(levels):
        name = re.sub(r"[^A-Za-z0-9]", "_", level["id"]).upper()
        points = ", ".join(rust_f32(v) for v in level["points"])
        lines.append(f"static TRACK_{index}: [f32; {len(level['points'])}] = [{points}];")
        lines.append("")
    lines.append("pub const LEVELS: &[LevelSpec] = &[")
    for index, level in enumerate(levels):
        stars = ", ".join(rust_f32(v) for v in level["stars"])
        lines.append(
            "    LevelSpec {"
            f' id: "{level["id"]}",'
            f' family: "{level["family"]}",'
            f' gravity: {rust_f32(level["gravity"])},'
            f' distance: {rust_f32(level["distance"])},'
            f' track_length: {rust_f32(level["length"])},'
            f' endless: {str(level["endless"]).lower()},'
            f' fuel_density: {rust_f32(level["fuel_density"])},'
            f' fuel_density_increase: {rust_f32(level["fuel_density_increase"])},'
            f' stars: [{stars}],'
            f' coin_multiplier: {rust_f32(level["coin_multiplier"])},'
            f' track: &TRACK_{index} }},'
        )
    lines.append("];")
    lines.append("")
    OUT_RS.write_text("\n".join(lines))


def write_report(levels: list[dict]) -> None:
    report = []
    for level in levels:
        ys = level["points"][1::2]
        report.append({
            "id": level["id"],
            "file": level["file"],
            "family": level["family"],
            "grammar": level["expr"],
            "gravity": level["gravity"],
            "distance": level["distance"],
            "length": level["length"],
            "endless": level["endless"],
            "fuel_density": level["fuel_density"],
            "fuel_density_increase": level["fuel_density_increase"],
            "stars": level["stars"],
            "points": len(level["points"]) // 2,
            "height_range": [round(min(ys), 2), round(max(ys), 2)],
        })
    OUT_JSON.write_text(json.dumps(report, indent=1))


if __name__ == "__main__":
    main()
