//! Temporary probe: prints the solver state while driving level 0.
use climb::*;

fn main() {
    let level = std::env::args().nth(1).and_then(|a| a.parse::<i32>().ok()).unwrap_or(0);
    let vehicle = std::env::args().nth(2).and_then(|a| a.parse::<i32>().ok()).unwrap_or(0);
    let gas: f32 = std::env::args().nth(3).and_then(|a| a.parse::<f32>().ok()).unwrap_or(1.0);
    start(vehicle, level);
    let n = state_len();
    let mut t = 0.0f32;
    for frame in 0..240 {
        set_input(gas, 0.0);
        tick(1.0 / 60.0);
        t += 1.0 / 60.0;
        if frame % 20 == 0 {
            let s = unsafe { std::slice::from_raw_parts(state_ptr(), n) };
            println!(
                "t={:5.2} x={:8.3} y={:8.3} ang={:6.2} vx={:7.2} vy={:7.2} fuel={:5.1} crash={} g0={} w0=({:.2},{:.2})",
                t, s[0], s[1], s[2], s[3], s[4], s[5], s[8], s[19 + 3], s[19], s[19 + 1]
            );
        }
    }
}
