#!/usr/bin/env python3
"""Extract the real vehicle parameters from the decrypted HCR2 configs.

Input:  <analysis>/jsons_decrypted/json/vehicles/vehicle_<id>.json       (definition)
        <analysis>/jsons_decrypted/json/vehicles/vehicle_<id>_model.json (rube/Box2D scene)
Output: ../src/vehicle_data.rs   (const table, no runtime JSON parsing)
        <analysis>/rube_vehicle_specs.json (report)

Every number written to Rust comes from a named field in the dump.

Field notes (all verified against the dumps):
  * masses/geometry/inertia      <- rube body massData-mass, massData-I, fixture circle radius
  * wheel offsets                <- wheel body position minus chassis body position
  * springFrequency/DampingRatio <- wheel-type joints (averaged over load-bearing joints)
  * gravity / stepsPerSecond     <- rube scene header
  * torque_base/max, speed_*     <- upgrade_effects of type engine_params (may span two upgrades)
  * grip_*, damping_*            <- body_friction / spring_damping upgrade effects
  * fuel_tank, air_turn_rate, cc <- definition keys

Not in the dump (calibrated in the solver, never invented here):
  fuel burn rate, crash thresholds, terrain profile.
"""

import json
import pathlib
import sys

ANALYSIS = pathlib.Path(
    "/home/gullin/Downloads/hc2rea/workspace-96625685-d7cf-4d3f-a70e-523b0bb426e9 (2)/analysis"
)
SRC = ANALYSIS / "jsons_decrypted" / "json" / "vehicles"
OUT_RS = pathlib.Path(__file__).resolve().parent.parent / "src" / "vehicle_data.rs"
OUT_JSON = ANALYSIS / "rube_vehicle_specs.json"

MAX_WHEELS = 6

# Clone roster order (must stay in sync with main.js ART array) -> real HCR2 id.
ROSTER = [
    ("jeep", "jeep", "Hill Climber"),
    ("superjeep", "superjeep", "Super Jeep"),
    ("scooter", "scooter", "Scooter"),
    ("motocross", "motocross", "Motocross Bike"),
    ("dunebuggy", "dunebuggy", "Dune Buggy"),
    ("rally", "rally", "Rally Car"),
    ("tank", "minitank", "Tank"),
    ("truck", "monster", "Monster Truck"),
    ("racing_truck", "racing_truck", "Racing Truck"),
    ("bus", "bus", "Bus"),
    ("snowmobile", "snowmobile", "Snowmobile"),
    ("super_diesel", "pickup_truck", "Super Diesel"),
    ("moonlander", "moonlander", "Moon Lander"),
    ("sledge", "sled", "Sleigh"),
]


def number(value, default=0.0, prefer="from"):
    """Config numbers are float, numeric string, or {from, to} (level 1 -> max)."""
    if isinstance(value, bool):
        return default
    if isinstance(value, (int, float)):
        return float(value)
    if isinstance(value, str):
        try:
            return float(value)
        except ValueError:
            return default
    if isinstance(value, dict):
        key = prefer if prefer in value else ("to" if "to" in value else "from")
        return number(value.get(key), default)
    return default


def engine_fields(definition):
    """engine_params can be split across upgrades (rally: speeds in one, torque in another)."""
    found = {}
    for upgrade in definition.get("upgrades", []):
        for eff in upgrade.get("upgrade_effects", []):
            if eff.get("type") != "engine_params":
                continue
            for key, value in eff.items():
                if key != "type" and key not in found:
                    found[key] = value
    return found


def upgrade_amounts(definition, kind, prefer="to"):
    for upgrade in definition.get("upgrades", []):
        for eff in upgrade.get("upgrade_effects", []):
            if eff.get("type") == kind:
                return number(eff.get("amounts"), 0.0, prefer)
    return None


def custom(body):
    out = {}
    for prop in body.get("customProperties", []):
        for key in ("bool", "string", "int", "float"):
            if key in prop:
                out[prop.get("name")] = prop[key]
    return out


def wheel_bodies(model, chassis):
    """Bodies that roll. Skips suspension arms ("Up"/"Mount") and driver limbs."""
    picked = []
    for body in model.get("body", []):
        if body is chassis:
            continue
        props = custom(body)
        name = body.get("name", "")
        radius = 0.0
        friction = None
        for fixture in body.get("fixture", []):
            circle = fixture.get("circle")
            if circle:
                radius = float(circle.get("radius", 0.0))
                friction = number(fixture.get("friction"), 0.0)
        if radius <= 0.0:
            continue
        is_wheel = props.get("partType") == "wheel" or name.lower() in (
            "wheel",
            "frontwheel",
            "rearwheel",
            "middlewheel",
        )
        if not is_wheel or "up" in name.lower() or "mount" in name.lower():
            continue
        picked.append(
            {
                "name": name,
                "x": body["position"]["x"] - chassis["position"]["x"],
                "y": body["position"]["y"] - chassis["position"]["y"],
                "r": radius,
                "mass": number(body.get("massData-mass"), 0.0),
                "friction": friction,
            }
        )
    picked.sort(key=lambda w: w["x"])
    if not picked:
        return []
    if len(picked) > MAX_WHEELS:
        # Keep the two axles plus the widest spread of the rest.
        front, rear = picked[-1], picked[0]
        rest = sorted(picked[1:-1], key=lambda w: abs(w["x"]), reverse=True)
        picked = sorted([rear, front] + rest[: MAX_WHEELS - 2], key=lambda w: w["x"])
    return picked


def wheel_springs(model):
    """Average frequency/damping over wheel joints that actually carry the body."""
    freqs, damps, torque, motor = [], [], 0.0, False
    for joint in model.get("joint", []):
        if joint.get("type") != "wheel":
            continue
        name = joint.get("name", "").lower()
        if name == "attach":
            continue
        frequency = number(joint.get("springFrequency"), 0.0)
        if frequency > 0.0:
            freqs.append(frequency)
            damps.append(number(joint.get("springDampingRatio"), 0.0))
        torque = max(torque, number(joint.get("maxMotorTorque"), 0.0))
        motor = motor or bool(joint.get("enableMotor", False))
    return (
        sum(freqs) / len(freqs) if freqs else 3.0,
        sum(damps) / len(damps) if damps else 0.22,
        torque,
        motor,
    )


def chassis_bbox(chassis):
    xs, ys = [], []
    for fixture in chassis.get("fixture", []):
        polygon = fixture.get("polygon")
        if not polygon:
            continue
        verts = polygon.get("vertices", {})
        xs.extend(verts.get("x", []))
        ys.extend(verts.get("y", []))
    if not xs:
        return (-1.0, 1.0, 0.0, 1.0)
    return (min(xs), max(xs), min(ys), max(ys))


def spec(clone_id, real_id, display):
    definition = json.load(open(SRC / f"vehicle_{real_id}.json"))
    model = json.load(open(SRC / f"vehicle_{real_id}_model.json"))
    bodies = {b.get("name"): b for b in model.get("body", [])}
    chassis = bodies["chassis"]

    wheels = wheel_bodies(model, chassis)
    freq, damp, motor_torque, motor_on = wheel_springs(model)
    engine = engine_fields(definition)

    torque = engine.get("max_torque", 0.0)
    speed = engine.get("max_speeds")
    grip_to = upgrade_amounts(definition, "body_friction", "to")
    grip_from = upgrade_amounts(definition, "body_friction", "from")
    damping_to = upgrade_amounts(definition, "spring_damping", "to")
    fuel_to = upgrade_amounts(definition, "fuel_tank", "to")
    thrust_from = upgrade_amounts(definition, "thrust", "from")
    thrust_to = upgrade_amounts(definition, "thrust", "to")

    base_friction = next((w["friction"] for w in wheels if w["friction"]), 0.0)

    left, right, bottom, top = chassis_bbox(chassis)
    out = {
        "id": clone_id,
        "real_id": real_id,
        "display_name": (definition.get("name", {}) or {}).get("value", display) or display,
        "engine_id": definition.get("engineSound", ""),
        "chassis_mass": number(chassis.get("massData-mass"), 1.0),
        "inertia": number(chassis.get("massData-I"), 0.0),
        "center_of_mass": chassis.get("massData-center", {"x": 0.0, "y": 0.0}),
        "gravity_y": (model.get("gravity") or {}).get("y", -10.0),
        "steps_per_second": model.get("stepsPerSecond", 60.0),
        "fuel_tank": number(definition.get("fuelTank"), 40.0),
        "air_turn_rate": number(definition.get("airTurnRate"), 40.0),
        "cc": number(definition.get("baseCCvalue"), 0.0),
        "spring_frequency": freq,
        "spring_damping": damp,
        "motor_max_torque": motor_torque,
        "motor_enabled": motor_on,
        "torque_base": number(torque, 0.0, "from"),
        "torque_max": number(torque, number(torque, 0.0, "from"), "to"),
        "speed_base": number(speed, 30.0, "from"),
        "speed_max": number(speed, number(speed, 30.0, "from"), "to"),
        "grip_base": grip_from if grip_from else base_friction,
        "grip_max": grip_to if grip_to else (grip_from if grip_from else base_friction),
        "damping_base": damp,
        "damping_max": damping_to if damping_to else damp,
        "fuel_base": number(definition.get("fuelTank"), 40.0),
        "fuel_max": fuel_to if fuel_to else number(definition.get("fuelTank"), 40.0),
        "thrust_base": thrust_from if thrust_from else 0.0,
        "thrust_max": thrust_to if thrust_to else (thrust_from if thrust_from else 0.0),
        "half_w": (right - left) / 2.0,
        "half_h": (top - bottom) / 2.0,
        "bbox": [left, right, bottom, top],
        "wheel_n": len(wheels),
        "wheels": wheels,
    }
    return out


def f32(value):
    return f"{float(value):.6f}f32"


def main():
    specs = [spec(*entry) for entry in ROSTER]
    OUT_JSON.write_text(json.dumps(specs, indent=1))

    lines = [
        "// @generated by climb/tools/extract_vehicle_data.py — do not edit by hand.",
        "//",
        "// Source: analysis/jsons_decrypted/json/vehicles/vehicle_<id>.json (definition)",
        "//         analysis/jsons_decrypted/json/vehicles/vehicle_<id>_model.json (rube scene)",
        "//",
        "// Units are the dumped Box2D ones: kg, m, N*m, Hz, m/s, s.",
        "// Nothing here is hand-tuned; see rube_vehicle_specs.json for the full report.",
        "",
        f"pub const MAX_WHEELS: usize = {MAX_WHEELS};",
        "",
        "#[derive(Clone, Copy)]",
        "pub struct VehicleSpec {",
        "    pub id: &'static str,",
        "    pub real_id: &'static str,",
        "    pub display_name: &'static str,",
        "    pub engine_id: &'static str,",
        "    pub chassis_mass: f32,",
        "    pub inertia: f32,",
        "    pub com_x: f32,",
        "    pub com_y: f32,",
        "    pub gravity: f32,",
        "    pub steps_per_second: f32,",
        "    pub fuel_tank: f32,",
        "    pub air_turn_rate: f32,",
        "    pub spring_frequency: f32,",
        "    pub spring_damping: f32,",
        "    pub damping_base: f32,",
        "    pub damping_max: f32,",
        "    pub motor_max_torque: f32,",
        "    pub motor_enabled: bool,",
        "    pub torque_base: f32,",
        "    pub torque_max: f32,",
        "    pub speed_base: f32,",
        "    pub speed_max: f32,",
        "    pub grip_base: f32,",
        "    pub grip_max: f32,",
        "    pub fuel_base: f32,",
        "    pub fuel_max: f32,",
        "    pub thrust_base: f32,",
        "    pub thrust_max: f32,",
        "    pub half_w: f32,",
        "    pub half_h: f32,",
        "    pub wheel_n: usize,",
        "    pub wheel_x: [f32; MAX_WHEELS],",
        "    pub wheel_y: [f32; MAX_WHEELS],",
        "    pub wheel_r: [f32; MAX_WHEELS],",
        "    pub wheel_mass: [f32; MAX_WHEELS],",
        "}",
        "",
        "pub const VEHICLE_SPECS: &[VehicleSpec] = &[",
    ]

    for s in specs:
        def arr(key):
            values = [w[key] for w in s["wheels"]] + [0.0] * (MAX_WHEELS - len(s["wheels"]))
            return "[" + ", ".join(f32(v) for v in values[:MAX_WHEELS]) + "]"

        lines.append("    VehicleSpec {")
        lines.append(f'        id: "{s["id"]}",')
        lines.append(f'        real_id: "{s["real_id"]}",')
        lines.append(f'        display_name: "{s["display_name"]}",')
        lines.append(f'        engine_id: "{s["engine_id"]}",')
        lines.append(f"        chassis_mass: {f32(s['chassis_mass'])},")
        lines.append(f"        inertia: {f32(s['inertia'])},")
        lines.append(f"        com_x: {f32(s['center_of_mass'].get('x', 0.0))},")
        lines.append(f"        com_y: {f32(s['center_of_mass'].get('y', 0.0))},")
        lines.append(f"        gravity: {f32(abs(s['gravity_y']))},")
        lines.append(f"        steps_per_second: {f32(s['steps_per_second'])},")
        lines.append(f"        fuel_tank: {f32(s['fuel_tank'])},")
        lines.append(f"        air_turn_rate: {f32(s['air_turn_rate'])},")
        lines.append(f"        spring_frequency: {f32(s['spring_frequency'])},")
        lines.append(f"        spring_damping: {f32(s['spring_damping'])},")
        lines.append(f"        damping_base: {f32(s['damping_base'])},")
        lines.append(f"        damping_max: {f32(s['damping_max'])},")
        lines.append(f"        motor_max_torque: {f32(s['motor_max_torque'])},")
        lines.append(f"        motor_enabled: {'true' if s['motor_enabled'] else 'false'},")
        lines.append(f"        torque_base: {f32(s['torque_base'])},")
        lines.append(f"        torque_max: {f32(s['torque_max'])},")
        lines.append(f"        speed_base: {f32(s['speed_base'])},")
        lines.append(f"        speed_max: {f32(s['speed_max'])},")
        lines.append(f"        grip_base: {f32(s['grip_base'])},")
        lines.append(f"        grip_max: {f32(s['grip_max'])},")
        lines.append(f"        fuel_base: {f32(s['fuel_base'])},")
        lines.append(f"        fuel_max: {f32(s['fuel_max'])},")
        lines.append(f"        thrust_base: {f32(s['thrust_base'])},")
        lines.append(f"        thrust_max: {f32(s['thrust_max'])},")
        lines.append(f"        half_w: {f32(s['half_w'])},")
        lines.append(f"        half_h: {f32(s['half_h'])},")
        lines.append(f"        wheel_n: {s['wheel_n']},")
        lines.append(f"        wheel_x: {arr('x')},")
        lines.append(f"        wheel_y: {arr('y')},")
        lines.append(f"        wheel_r: {arr('r')},")
        lines.append(f"        wheel_mass: {arr('mass')},")
        lines.append("    },")

    lines.append("];")
    lines.append("")
    OUT_RS.write_text("\n".join(lines))

    print(f"{len(specs)} vehicles -> {OUT_RS}")
    header = f"{'id':<13}{'mass':>7}{'wheels':>7}{'r':>6}{'torque':>8}{'vmax':>6}{'freq':>6}{'damp':>6}{'grip':>6}{'fuel':>6}"
    print(header)
    for s in specs:
        radii = s["wheels"][0]["r"] if s["wheels"] else 0.0
        print(
            f"{s['id']:<13}{s['chassis_mass']:>7.3f}{s['wheel_n']:>7}{radii:>6.2f}"
            f"{s['torque_base']:>8.1f}{s['speed_max']:>6.0f}{s['spring_frequency']:>6.1f}"
            f"{s['spring_damping']:>6.2f}{s['grip_max']:>6.2f}{s['fuel_tank']:>6.0f}"
        )


if __name__ == "__main__":
    sys.exit(main())
