# hcr2-rust

A browser hill-climb game whose **driving physics is reimplemented in Rust** and
compiled to WebAssembly: an articulated chassis with real sprung wheel bodies,
the same suspension constants, the same terrain curves and the same HUD atlas as
the 2D hill-climb game it is modelled on.

No engine, no wasm-bindgen, no JS dependencies. One `.wasm` file, a canvas and
about 1000 lines of front-end.

```
cargo test                 # 20 tests, all passing
cargo build --release --target wasm32-unknown-unknown
python3 tools/extract_assets.py /path/to/game.xapk
python3 -m http.server 8765 && xdg-open http://127.0.0.1:8765/index.html
```

## Why this exists

Most "hill climb" clones fake the car: one rigid body with wheels welded to it,
then a torque applied to make it look like it is driving. That is why they feel
wrong — no squat, no dive, no wheelies, wheels that never move relative to the
body.

This one uses the actual construction the original scene files describe: a
chassis body and **one dynamic body per wheel**, joined by a `wheel` joint — a
spring along the chassis' local `+Y`, a rigid constraint perpendicular to it,
and a motor on the wheel's spin. Every constant is a named field taken from the
game data, never a tuned magic number:

| Quantity | Source |
| --- | --- |
| Chassis mass, inertia, centre of mass | `massData-mass` / `massData-I` / `massData-center` |
| Wheel mass, radius, position | wheel body `massData-mass`, fixture `circle.radius` |
| Suspension spring rate | `k = m_w * (2*pi*springFrequency)^2` |
| Damper | `c = 2 * zeta * sqrt(k * m_w)` from `springDampingRatio` |
| Drive torque | `engine_params` torque curve |
| Motor ceiling | joint `maxMotorTorque` (a ceiling, it never binds) |
| Top speed | `speed` curve — sets the wheel-speed target |
| Grip | `grip` curve — Coulomb limit at the contact patch |
| Terrain | the level's own `algorithm.expr`, evaluated at build time |
| HUD layout, meter art, pedals | the game's 26-frame `game.plist` atlas |

Where a value genuinely is a solver tuning choice (`BAUMGARTE`, `ITER`,
restitution) it is a named constant with a comment saying so — never hidden
inside a formula that pretends to be data.

## Physics

Fixed step at the game's own 60 Hz, with 4 substeps of 1/240 s. Each substep:

1. **Gravity** applied to the chassis and to every wheel body.
2. **Suspension spring** as an explicit force along the joint axis, `-k*s - c*vs`,
   where `s` is the compression of the wheel towards the chassis. The reaction is
   applied to the chassis *at the joint anchor*, so it produces pitch torque.
3. **Motor**: engine torque drives the wheel spin towards `-vmax / r`. The
   reaction is a pure couple on the chassis — that is what lifts the nose under
   throttle and dives under braking, and it comes out of the gear ratio rather
   than from a scripted "wheelie" hack.
4. **Solver sweeps**: `ITER = 8` passes alternating
   *(a)* the perpendicular part of the wheel joint, and *(b)* the ground contact
   for every wheel. Impulses on a wheel reach the chassis through the joint on
   the next sweep, so load transfer is a consequence of the geometry.
5. **Contact friction** is Coulomb-limited at `mu * jn` and its effective mass is
   `1/m + r^2/I`. Because the contact point velocity includes the wheel's
   angular velocity, a spinning wheel drives the car forward and a rolling wheel
   keeps rolling — no separate "drive force" term anywhere.
6. **Integration**, angle normalisation to `[-pi, pi]`, flip counting.

The belly of the chassis also collides with the ground, but only once the body is
tilted past ~57 degrees: the dumped `half_h` is the bounding box of the whole
sprite (the bus is 1.5 m tall with wheels hanging 1.24 m below its origin), so
using it as a collision box at rest would hold the car up before the tyres
touched anything.

Static sag falls out of this: a 2.27 kg jeep on two 0.25 kg wheels with 3 Hz
springs settles ~13 cm. The car visibly squats on landing and rebounds through
the damper instead of snapping to the ground.

## Terrain

Level geometry is not a sine wave. Each level ships an expression program:

```
var y := 0.0;
y += 2.0*sin(x/9.0) + 1.4*sin(x/3.7);
clamp(-1.0, y, 4.0);
```

`tools/extract_levels.py` implements a tokenizer and a recursive-descent parser
for that little language (`:=`, `+=`, `*=`, `^`, `%`, `cos/sin/abs/min/max/pow/
ceil/floor`, `clamp(lo, value, hi)`), evaluates it per level, decimates the
result with Douglas-Peucker, and bakes it into `src/level_data.rs`. 30 levels,
~43k points, no expression VM in the wasm.

At runtime `terrain_sample(level, x)` does a binary search plus a linear
interpolation and returns *(height, slope)*, so the contact normal follows the
hill exactly where the original does.

## Vehicles

14 chassis are prepared from the generated table: jeep, super jeep, scooter,
motocross, dune buggy, rally car, mini tank, monster truck, racing truck, bus,
snowmobile, pickup, moonlander and sled. Wheel count is per vehicle (the mini
tank drives on all six), and low-gravity levels (moon, underwater) use the
level's own gravity vector.

Upgrades follow the dumped curves: torque, top speed, grip, fuel tank, air
control and downforce are interpolated between their base and max values over 20
levels, and the prepared vehicle is expressed entirely in SI units (kg, m, N*m,
m/s).

## Front-end

`main.js` boots the wasm, reads three blobs out of its memory (vehicle roster,
level list, vehicle geometry) and renders with plain canvas 2D. Art comes from
`assets/manifest.json`, which `tools/extract_assets.py` produces from the game
package: per-vehicle chassis and tire sprites, the HUD atlas, and ground
textures.

Controls: right half of the canvas (or `ArrowRight` / `D`) accelerates, left half
(or `ArrowLeft` / `A`) brakes and reverses. The pedals are drawn from the same
atlas as the meters.

## Building

Requires a Rust toolchain with the `wasm32-unknown-unknown` target:

```
rustup target add wasm32-unknown-unknown
cargo test --release
cargo build --release --target wasm32-unknown-unknown
cp target/wasm32-unknown-unknown/release/climb.wasm .
```

A prebuilt `climb.wasm` is committed so the page runs without a toolchain.

## Assets

Artwork is **not** in this repository — it belongs to Fingersoft. Generate it
locally from a package you own:

```
python3 tools/extract_assets.py ~/Downloads/game.xapk
```

This writes `assets/cars/<id>/`, `assets/hud/game.{png,json}`,
`assets/ground/` and `assets/manifest.json`. Then serve the folder over HTTP
(the wasm cannot be fetched from `file://`):

```
python3 -m http.server 8765
```

Without assets the simulation still runs; you just get untextured boxes.

## Layout

```
src/lib.rs           wasm ABI, world/state, fuel, coins, crash, levels
src/suspension.rs    articulated chassis + wheel joints (the physics)
src/vehicles.rs      roster, upgrade curves, SI preparation
src/vehicle_data.rs  generated: masses, springs, torques, wheel geometry
src/level_data.rs    generated: baked terrain polylines, 30 levels
src/tuning.rs        upgrade effects
src/economy.rs       payouts and upgrade costs
src/physics.rs       small shared helpers
main.js              renderer, input, boot
hud.js               HUD drawn from the atlas
index.html           page
tools/               extractors that regenerate the data tables
```

## License

MIT for the code (see `LICENSE`). Game assets, names and trademarks are not
covered — see `NOTICE.md`.
