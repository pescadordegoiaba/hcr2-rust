// HUD drawn with the game's own atlas (assets/hud/game.png + game.json).
//
// The frames come straight out of HD/game.plist inside the APK, so the gauges,
// gear digits and pedals are the original art at the original proportions. The
// layout mirrors the in-game HUD: fuel gauge bottom-left, rpm + gear bottom
// centre, pedals bottom-right, distance top-left.

const HUD = {
  sheet: null,
  frames: null,
  scale: 1,
};

async function loadHud() {
  const meta = await fetch("./assets/hud/game.json").then((r) => r.json());
  const image = new Image();
  image.src = "./assets/hud/game.png";
  await image.decode();
  HUD.frames = meta;
  HUD.sheet = image;
  return HUD;
}

function hudFrame(name) {
  const f = HUD.frames && HUD.frames[name];
  if (!f || !HUD.sheet) return null;
  return f;
}

function drawFrame(ctx, name, x, y, w, h, rotation = 0) {
  const f = hudFrame(name);
  if (!f) return false;
  ctx.save();
  ctx.translate(x + w / 2, y + h / 2);
  if (rotation) ctx.rotate(rotation);
  ctx.drawImage(HUD.sheet, f.x, f.y, f.w, f.h, -w / 2, -h / 2, w, h);
  ctx.restore();
  return true;
}

/// Fuel gauge: the dial art plus a needle sweeping with the remaining fuel.
function drawFuelGauge(ctx, x, y, size, ratio) {
  if (!drawFrame(ctx, "game/meter-fuel.png", x, y, size, size)) return;
  const cx = x + size * 0.5;
  const cy = y + size * 0.55;
  // The dial's scale runs from lower-left to lower-right; sweep 240 degrees.
  const start = Math.PI * 0.78;
  const sweep = Math.PI * 1.44;
  const angle = start + sweep * Math.max(0, Math.min(1, ratio));
  ctx.save();
  ctx.strokeStyle = ratio < 0.18 ? "#d2402f" : "#f6f2e2";
  ctx.lineWidth = Math.max(2, size * 0.035);
  ctx.lineCap = "round";
  ctx.beginPath();
  ctx.moveTo(cx, cy);
  ctx.lineTo(cx + Math.cos(angle) * size * 0.34, cy + Math.sin(angle) * size * 0.34);
  ctx.stroke();
  ctx.restore();
}

/// Rpm dial plus the gear digit that the original shows inside it.
function drawRpmGauge(ctx, x, y, size, ratio, gear) {
  if (!drawFrame(ctx, "game/meter-rpm.png", x, y, size, size)) return;
  const cx = x + size * 0.5;
  const cy = y + size * 0.5;
  const start = Math.PI * 0.75;
  const sweep = Math.PI * 1.5;
  const angle = start + sweep * Math.max(0, Math.min(1, ratio));
  ctx.save();
  ctx.strokeStyle = ratio > 0.92 ? "#e2553a" : "#f6f2e2";
  ctx.lineWidth = Math.max(2, size * 0.028);
  ctx.lineCap = "round";
  ctx.beginPath();
  ctx.moveTo(cx, cy);
  ctx.lineTo(cx + Math.cos(angle) * size * 0.36, cy + Math.sin(angle) * size * 0.36);
  ctx.stroke();
  ctx.restore();

  const digit = gear <= 0 ? "game/R-gear.png" : `game/${Math.min(3, gear)}-gear.png`;
  const dw = size * 0.16;
  const dh = size * 0.20;
  drawFrame(ctx, "game/gear_indicator.png", cx - size * 0.16, cy - size * 0.30, size * 0.32, size * 0.44);
  drawFrame(ctx, digit, cx - dw / 2, cy - size * 0.20, dw, dh);
}

function drawPedals(ctx, w, h, gasDown, brakeDown) {
  const gasSize = Math.min(h * 0.30, w * 0.16);
  const gasX = w - gasSize * 1.05;
  const gasY = h - gasSize * 1.08;
  const brakeSize = gasSize * 0.92;
  const brakeX = gasX - brakeSize * 1.02;
  const brakeY = h - brakeSize * 1.04;
  drawFrame(ctx, gasDown ? "game/pedal-gas-pressed.png" : "game/pedal-gas-normal.png",
    gasX, gasY, gasSize, gasSize);
  drawFrame(ctx, brakeDown ? "game/pedal-brake-pressed.png" : "game/pedal-brake-normal.png",
    brakeX, brakeY, brakeSize, brakeSize);
}

/// Whole HUD: called every frame from draw().
function drawHud(ctx, w, h, state, inputs) {
  if (!HUD.sheet) return;
  const unit = Math.min(w, h);
  const gauge = Math.max(56, unit * 0.13);
  const fuelMax = state.fuelMax > 0 ? state.fuelMax : 1;
  drawFuelGauge(ctx, w * 0.02, h - gauge * 1.06, gauge, state.fuel / fuelMax);

  const rpmSize = gauge * 1.15;
  const rpmRatio = Math.min(1, state.speed / Math.max(8, state.topSpeed || 30));
  const gear = Math.max(0, Math.min(3, Math.round(rpmRatio * 3)));
  drawRpmGauge(ctx, w * 0.02 + gauge * 1.02, h - rpmSize * 1.06, rpmSize, rpmRatio, gear);

  drawPedals(ctx, w, h, inputs.gas > 0.5, inputs.brake > 0.5);

  ctx.save();
  ctx.font = `600 ${Math.max(14, unit * 0.028)}px "Segoe UI", sans-serif`;
  ctx.fillStyle = "rgba(12, 18, 12, 0.72)";
  ctx.textBaseline = "top";
  const label = `${Math.floor(state.distance)} m`;
  const metrics = ctx.measureText(label);
  ctx.fillRect(12, 10, metrics.width + 22, unit * 0.05);
  ctx.fillStyle = "#f6f2e2";
  ctx.fillText(label, 23, 12 + unit * 0.004);
  ctx.restore();
}
