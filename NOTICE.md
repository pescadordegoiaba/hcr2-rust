# NOTICE

This repository contains an independent, clean-room-style reimplementation of
the *driving physics and browser front-end* of a 2D hill-climb game, written in
Rust and compiled to WebAssembly.

## What is NOT in this repository

Deliberately excluded, because it is not ours to redistribute:

- PNG/atlas artwork, sprites, icons and audio extracted from the game package.
- The original `.rube` / `.csb` scene files and decrypted game JSON.
- The `.xapk` / `.apk` package itself.
- Any decompiled or dumped library code.

The `assets/` directory is generated locally by `tools/extract_assets.py` from a
copy of the game package that **you** own. It is listed in `.gitignore` and is
never committed.

## Trademarks

"Hill Climb Racing" and "Hill Climb Racing 2" are trademarks of Fingersoft Ltd.
This project is not affiliated with, endorsed by, or sponsored by Fingersoft.
Do not use it to distribute their assets or to circumvent their services.

## Data provenance

Numeric constants in `src/vehicle_data.rs` and `src/level_data.rs` are
parameters (masses, spring frequencies, damping ratios, motor torques, terrain
profiles) derived from files the user supplied locally. They describe physical
behaviour, are necessary for the simulation to work, and are not executable
game code. The generated files are checked in so the crate builds without the
original package; `tools/extract_*.py` regenerate them from a local dump.

If you are a rights holder and want something removed, open an issue.
