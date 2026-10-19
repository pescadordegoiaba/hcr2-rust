const ART = [
  ["../textures/cars/jeep/default/default/chassis.png", "../textures/cars/jeep-tire.png"],
  ["../textures/cars/superjeep/default/default/chassis.png", "../textures/cars/superjeep/super-jeep_tire.png"],
  ["../textures/cars/scooter/default/default/chassis.png", "../textures/cars/jeep-tire.png"],
  ["../textures/cars/motocross/default/default/chassis.png", "../textures/cars/jeep-tire.png"],
  ["../textures/cars/dunebuggy/default/default/chassis.png", "../textures/cars/dunebuggy/default/default/dunebuggytire_front.png"],
  ["../textures/cars/rally/default/default/chassis.png", "../textures/cars/jeep-tire.png"],
  ["../textures/cars/minitank/default/default/car_minitank-chassis.png", "../textures/cars/minitank/default/default/car_minitank-wheel.png"],
  ["../textures/cars/monster/default/default/chassis.png", "../textures/cars/monster/car_monster-tire-survivor.png"],
  ["../textures/cars/racing_truck/default/default/chassis.png", "../textures/cars/jeep-tire.png"],
  ["../textures/cars/bus/default/default/bus_chassis_top.png", "../textures/cars/jeep-tire.png"],
  ["../textures/cars/snowmobile/default/default/snowmobile_chassis.png", "../textures/cars/snowmobile/default/default/snowmobile_tire_medium.png"],
  ["../textures/cars/tractor/chassis.png", "../textures/cars/tractor/tire.png"],
  ["../textures/cars/moonlander/default/default/moonlander_chassis.png", "../textures/cars/jeep-tire.png"],
  ["../textures/cars/sled/default/sled.png", "../textures/cars/jeep-tire.png"],
];

// Art comes from assets/manifest.json (tools/extract_assets.py reads the APK)
// and the level list comes from the wasm table (tools/extract_levels.py bakes
// the game's own `algorithm.expr`). Nothing below is hand-authored scenery.
let MANIFEST = null;
let LEVELS = [];
let levelId = 0;

/// Sky, ground texture and music per level family. Families without their own
/// sky fall back to the countryside palette; the ground texture itself comes
/// from the manifest, so it is always the one the APK ships for that biome.
function paletteFor(family) {
  const sky = {
    countryside: "../textures/sky/sky-bg-countryside.png",
    forest: "../textures/sky/sky-bg-forest3.png",
    dark: "../textures/sky/darkforest-Sky.png",
    winter: "../textures/sky/sky-bg-countryside_snow.png",
    glacier: "../textures/sky/sky-bg-forest7.png",
    mountain: "../textures/sky/sky-bg-forest7.png",
    nationalpark: "../textures/sky/nationalpark_sky.png",
    moon: "../textures/sky/sky-bg-moon.png",
    mine: "../textures/sky/sky-bg-bayou.png",
    workshop: "../textures/sky/sky-bg-bayou.png",
    trailerpark: "../textures/sky/sky-bg-bayou.png",
    savanna: "../textures/sky/savanna_sky.png",
    city: "../textures/sky/sky-bg-city.png",
    cuptown: "../textures/sky/cuptown-sky.png",
    amusement: "../textures/sky/amusementpark_sky.png",
    amusementpark: "../textures/sky/amusementpark_sky.png",
    arena: "../textures/sky/sky-bg-arena.png",
    underwater: "../textures/sky/sky-bg_deepwater.png",
    route66: "../textures/sky/route_sky.png",
  }[family] || "../textures/sky/sky-bg-countryside.png";
  const groundFile = (MANIFEST && MANIFEST.ground && (MANIFEST.ground[family] || MANIFEST.ground._default))
    || "terrain-ground-forest.png";
  const tint = {
    moon: "#8d9098", underwater: "#1d5f6b", mine: "#4a3a2a", workshop: "#4a3a2a",
    city: "#6a6a72", cuptown: "#6a6a72", amusement: "#5a4a62", amusementpark: "#5a4a62",
    arena: "#7a6a52", glacier: "#c9d7e2", winter: "#d8e4ec",
  }[family] || "#5d3b22";
  return { sky, ground: `./assets/ground/${groundFile}`, tint, music: "../common-4/music/bgm_countryside-race.ogg" };
}

function currentLevel() {
  return LEVELS[levelId] || { id: "—", family: "countryside", distance: 0, gravity: -10 };
}

const STATS = [
  ["acceleration", "Aceleração"],
  ["top", "Velocidade"],
  ["fuel", "Tanque"],
  ["suspension", "Suspensão"],
  ["grip", "Aderência"],
];

let VEHICLES = ART.map(([chassis, tire], id) => ({ id, name: "…", chassis, tire }));

const BIOMES = [
  {
    id: 0,
    name: "Campo",
    sky: "../textures/sky/sky-bg-countryside.png",
    ground: "../textures/terrain/countryside-slice.png",
    music: "../common-4/music/bgm_countryside-race.ogg",
  },
  {
    id: 1,
    name: "Floresta",
    sky: "../textures/sky/sky-bg-forest.png",
    ground: "../textures/terrain/terrain-ground-forest.png",
    music: "../common-4/music/bgm_forest-race.ogg",
  },
  {
    id: 2,
    name: "Praia",
    sky: "../textures/sky/sky-bg-beach.png",
    ground: "../textures/terrain/terrain-ground-beach.png",
    music: "../common-4/music/bgm_countryside-distance.ogg",
  },
  {
    id: 3,
    name: "Lua",
    sky: "../textures/sky/sky-bg-moon.png",
    ground: "../textures/terrain/terrain-ground_moon.png",
    music: "../common-4/music/bgm_cuptown-revB.ogg",
  },
  {
    id: 4,
    name: "Savana",
    sky: "../textures/sky/savanna_sky.png",
    ground: "../textures/terrain/savanna_terrain.png",
    music: "../common-4/music/bgm_countryside-intro.ogg",
  },
  {
    id: 5,
    name: "Deserto",
    sky: "../textures/sky/sky-bg-beach-sunset.png",
    ground: "../textures/terrain/terrain-surface-desert.png",
    music: "../common-4/music/bgm_finish.ogg",
  },
];

const canvas = document.getElementById("view");
const ctx = canvas.getContext("2d");
const hud = {
  dist: document.getElementById("dist"),
  speed: document.getElementById("speed"),
  fuel: document.getElementById("fuel"),
  coins: document.getElementById("coins"),
  note: document.getElementById("note"),
};
const menu = document.getElementById("menu");
const end = document.getElementById("end");
const music = document.getElementById("music");

let api = null;
let vehicleId = 0;
let biomeId = 0;
let gas = 0;
let brake = 0;
let running = false;
let ended = false;
const images = new Map();

function loadImage(src) {
  if (images.has(src)) return images.get(src);
  const img = new Image();
  img.src = src;
  images.set(src, img);
  return img;
}

function choiceRow(host, items, getId, setId) {
  items.forEach((item) => {
    const button = document.createElement("button");
    button.type = "button";
    button.textContent = item.name;
    button.dataset.id = String(item.id);
    button.addEventListener("click", () => {
      setId(item.id);
      [...host.children].forEach((child) => child.classList.toggle("on", child === button));
    });
    host.appendChild(button);
  });
  host.children[0].classList.add("on");
}

// Level buttons are built in boot(), once the wasm level table is read.

const GARAGE_KEY = "trilha-garage";
function loadGarage() {
  const blank = { coins: 2000, levels: {}, grip: false, best: 0 };
  try {
    const raw = JSON.parse(localStorage.getItem(GARAGE_KEY) || "null");
    if (!raw || typeof raw.coins !== "number") return blank;
    raw.levels = raw.levels || {};
    raw.best = raw.best || 0;
    return raw;
  } catch (_error) {
    return blank;
  }
}
let garage = loadGarage();
function saveGarage() {
  localStorage.setItem(GARAGE_KEY, JSON.stringify(garage));
  paintCoins();
}
function paintCoins() {
  hud.coins.textContent = `${Math.floor(garage.coins)} moedas`;
}
function levelsFor(id) {
  const found = garage.levels[id] || {};
  return {
    acceleration: found.acceleration || 1,
    top: found.top || 1,
    fuel: found.fuel || 1,
    suspension: found.suspension || 1,
    grip: found.grip || 1,
  };
}
const MAX_LEVEL = 20;

function paintUpgrades() {
  const host = document.getElementById("upgrades");
  host.replaceChildren();
  const levels = levelsFor(vehicleId);
  STATS.forEach(([key, label]) => {
    const level = levels[key];
    const button = document.createElement("button");
    button.type = "button";
    const cost = api ? api.upgrade_cost_at(level) : 0;
    button.textContent = level >= MAX_LEVEL ? `${label} ${level}` : `${label} ${level} · ${cost}`;
    button.disabled = !api || level >= MAX_LEVEL || garage.coins < cost;
    button.addEventListener("click", () => {
      const price = api.upgrade_cost_at(levelsFor(vehicleId)[key]);
      if (garage.coins < price) return;
      garage.coins -= price;
      const next = levelsFor(vehicleId);
      next[key] += 1;
      garage.levels[vehicleId] = next;
      saveGarage();
      paintUpgrades();
    });
    host.appendChild(button);
  });
}
function pushUpgrades() {
  if (!api) return;
  const levels = levelsFor(vehicleId);
  api.set_upgrades(levels.acceleration, levels.top, levels.fuel, levels.suspension, levels.grip);
  api.set_grip_part(document.getElementById("grip").checked ? 1 : 0);
}
document.getElementById("grip").checked = !!garage.grip;
document.getElementById("grip").addEventListener("change", (event) => {
  garage.grip = event.target.checked;
  saveGarage();
});
paintCoins();

function resize() {
  const rect = canvas.getBoundingClientRect();
  const dpr = Math.min(window.devicePixelRatio || 1, 2);
  canvas.width = Math.max(1, Math.floor(rect.width * dpr));
  canvas.height = Math.max(1, Math.floor(rect.height * dpr));
}
window.addEventListener("resize", resize);
resize();

function floats(ptr, count) {
  return new Float32Array(api.memory.buffer, ptr, count);
}

// Layout exported by src/lib.rs: scalars first, then five floats per wheel
// (x, y, spin, grounded, radius) starting at WHEEL_BLOCK, then omega.
const WHEEL_BLOCK = 19;
const MAX_WHEELS = 6;

function readState() {
  const view = floats(api.state_ptr(), api.state_len());
  const wheels = [];
  const count = Math.min(view[10] | 0, MAX_WHEELS);
  for (let i = 0; i < count; i += 1) {
    const base = WHEEL_BLOCK + i * 5;
    wheels.push({
      x: view[base], y: view[base + 1], spin: view[base + 2],
      grounded: view[base + 3] > 0.5, radius: view[base + 4],
    });
  }
  return {
    x: view[0], y: view[1], angle: view[2], vx: view[3], vy: view[4],
    fuel: view[5], distance: view[6], speed: view[7],
    crashed: view[8], finished: view[9], wheels,
    halfW: view[11], halfH: view[12], fuelMax: view[13],
    flips: view[14], reward: view[15],
    // The last two slots are the chassis angular velocity and the vehicle's
    // top speed (m/s), which scales the rpm dial.
    omega: view[view.length - 2],
    topSpeed: view[view.length - 1],
  };
}

function worldToScreen(x, y, cam, scale, h) {
  return [(x - cam) * scale, h - y * scale];
}

function draw() {
  const w = canvas.width;
  const h = canvas.height;
  const level = currentLevel();
  const biome = paletteFor(level.family);
  const vehicle = VEHICLES[vehicleId] || VEHICLES[0];
  const sky = loadImage(biome.sky);
  const ground = loadImage(biome.ground);
  const chassis = loadImage(vehicle.chassis);
  const tire = loadImage(vehicle.tire);
  const coinImg = loadImage("../textures/collectibles/battery.png");

  ctx.clearRect(0, 0, w, h);
  if (sky.complete && sky.naturalWidth) {
    ctx.drawImage(sky, 0, 0, w, h);
  } else {
    ctx.fillStyle = "#8ec6e6";
    ctx.fillRect(0, 0, w, h);
  }

  if (!api || !running) {
    requestAnimationFrame(draw);
    return;
  }

  const state = readState();
  const scale = Math.max(42, Math.min(w, h) / 14);
  const cam = state.x - (w / scale) * 0.32;
  const terrain = floats(api.terrain_ptr(), api.terrain_len() * 2);
  const coins = floats(api.coins_ptr(), api.coins_len() * 3);

  ctx.beginPath();
  let started = false;
  for (let i = 0; i < terrain.length; i += 2) {
    const [sx, sy] = worldToScreen(terrain[i], terrain[i + 1], cam, scale, h);
    if (!started) { ctx.moveTo(sx, sy); started = true; }
    else ctx.lineTo(sx, sy);
  }
  ctx.lineTo(w + 40, h + 40);
  ctx.lineTo(-40, h + 40);
  ctx.closePath();
  // Ground tiles are often transparent above the dirt, so paint a solid hill first.
  ctx.fillStyle = biome.tint;
  ctx.fill();
  if (ground.complete && ground.naturalWidth) {
    const pattern = ctx.createPattern(ground, "repeat");
    ctx.save();
    ctx.translate(-((cam * scale) % ground.width), 0);
    ctx.fillStyle = pattern;
    ctx.fill();
    ctx.restore();
  }
  ctx.strokeStyle = "rgba(20,30,10,0.45)";
  ctx.lineWidth = 3;
  ctx.stroke();

  for (let i = 0; i < coins.length; i += 3) {
    if (coins[i + 2] < 0.5) continue;
    const [sx, sy] = worldToScreen(coins[i], coins[i + 1], cam, scale, h);
    const s = scale * 0.7;
    if (coinImg.complete && coinImg.naturalWidth) ctx.drawImage(coinImg, sx - s / 2, sy - s / 2, s, s);
    else {
      ctx.fillStyle = "#e2b15a";
      ctx.fillRect(sx - 6, sy - 6, 12, 12);
    }
  }

  function drawWheel(x, y, spin, radius) {
    const [sx, sy] = worldToScreen(x, y, cam, scale, h);
    const d = radius * 2 * scale;
    ctx.save();
    ctx.translate(sx, sy);
    ctx.rotate(-spin);
    if (tire.complete && tire.naturalWidth) ctx.drawImage(tire, -d / 2, -d / 2, d, d);
    else {
      ctx.fillStyle = "#222";
      ctx.beginPath();
      ctx.arc(0, 0, d / 2, 0, Math.PI * 2);
      ctx.fill();
    }
    ctx.restore();
  }

  for (const wheel of state.wheels) {
    drawWheel(wheel.x, wheel.y, wheel.spin, wheel.radius || 0.36);
  }

  const [cx, cy] = worldToScreen(state.x, state.y, cam, scale, h);
  ctx.save();
  ctx.translate(cx, cy);
  ctx.rotate(-state.angle);
  // Keep the sprite's own aspect ratio: its width is calibrated against the
  // chassis polygon from the dump, so the wheels line up with the body.
  const cw = state.halfW * 2 * scale;
  const ch = chassis.complete && chassis.naturalWidth
    ? cw * (chassis.naturalHeight / chassis.naturalWidth)
    : state.halfH * 2 * scale;
  if (chassis.complete && chassis.naturalWidth) ctx.drawImage(chassis, -cw / 2, -ch / 2, cw, ch);
  else {
    ctx.fillStyle = "#c4542c";
    ctx.fillRect(-cw / 2, -ch / 2, cw, ch);
  }
  ctx.restore();

  // The in-game HUD, drawn from the APK atlas: fuel gauge, rpm/gear, pedals.
  drawHud(ctx, w, h, state, { gas, brake });

  hud.dist.textContent = `${Math.floor(state.distance)} m`;
  hud.speed.textContent = `${Math.floor(state.speed * 3.6)} km/h`;
  hud.fuel.textContent = state.fuelMax > 0
    ? `combustível ${Math.max(0, Math.ceil(state.fuel))}/${Math.round(state.fuelMax)}`
    : "sem motor";

  if (!ended && (state.crashed > 0.5 || state.finished > 0.5 || state.fuel <= 0.05 && state.speed < 0.4 && state.distance > 8)) {
    ended = true;
    running = false;
    document.getElementById("end-title").textContent = state.finished > 0.5 ? "Chegou" : state.crashed > 0.5 ? "Capotou" : "Sem combustível";
    const gained = Math.floor(state.reward);
    garage.coins += gained;
    garage.best = Math.max(garage.best || 0, Math.floor(state.distance));
    saveGarage();
    paintUpgrades();
    const flips = state.flips > 0 ? ` ${state.flips} volta(s) no ar.` : "";
    document.getElementById("end-copy").textContent = `${Math.floor(state.distance)} metros com o ${vehicle.name} em ${currentLevel().id}.${flips} +${gained} moedas. Recorde ${garage.best} m.`;
    end.hidden = false;
    music.pause();
  }

  requestAnimationFrame(draw);
}

function setHeld(nextGas, nextBrake) {
  gas = nextGas;
  brake = nextBrake;
  if (api && running) api.set_input(gas, brake);
}

const keys = new Set();
function syncKeys() {
  const nextGas = keys.has("arrowright") || keys.has("d") || keys.has("w") ? 1 : 0;
  const nextBrake = keys.has("arrowleft") || keys.has("a") || keys.has("s") ? 1 : 0;
  setHeld(Math.max(nextGas, gasButton), Math.max(nextBrake, brakeButton));
}
let gasButton = 0;
let brakeButton = 0;
window.addEventListener("keydown", (event) => {
  keys.add(event.key.toLowerCase());
  if (["arrowright", "arrowleft", " "].includes(event.key.toLowerCase())) event.preventDefault();
  syncKeys();
});
window.addEventListener("keyup", (event) => {
  keys.delete(event.key.toLowerCase());
  syncKeys();
});

function hold(button, which) {
  const down = (event) => {
    event.preventDefault();
    if (which === "gas") gasButton = 1;
    else brakeButton = 1;
    syncKeys();
  };
  const up = (event) => {
    event.preventDefault();
    if (which === "gas") gasButton = 0;
    else brakeButton = 0;
    syncKeys();
  };
  button.addEventListener("pointerdown", down);
  button.addEventListener("pointerup", up);
  button.addEventListener("pointerleave", up);
}
hold(document.getElementById("btn-gas"), "gas");
hold(document.getElementById("btn-brake"), "brake");

// The pedals are drawn on the canvas, so the canvas itself is the control:
// right half accelerates, left half brakes (same as the on-screen pedals).
function canvasHold(which, on) {
  if (which === "gas") gasButton = on ? 1 : 0;
  else brakeButton = on ? 1 : 0;
  syncKeys();
}
for (const [event, on] of [["pointerdown", true], ["pointerup", false], ["pointercancel", false]]) {
  canvas.addEventListener(event, (e) => {
    if (!running) return;
    e.preventDefault();
    const rect = canvas.getBoundingClientRect();
    const x = (e.clientX - rect.left) / rect.width;
    canvasHold(x < 0.5 ? "brake" : "gas", on);
  });
}

function startRun() {
  if (!api) return;
  pushUpgrades();
  api.start(vehicleId, levelId);
  api.set_input(0, 0);
  gas = 0;
  brake = 0;
  running = true;
  ended = false;
  menu.hidden = true;
  end.hidden = true;
  music.src = paletteFor(currentLevel().family).music;
  music.play().catch(() => {});
  let last = performance.now();
  function frame(now) {
    if (!running) return;
    const dt = Math.min(0.05, (now - last) / 1000);
    last = now;
    api.tick(dt);
    requestAnimationFrame(frame);
  }
  requestAnimationFrame(frame);
}

document.getElementById("go").addEventListener("click", startRun);
document.getElementById("again").addEventListener("click", () => {
  end.hidden = true;
  menu.hidden = false;
});

async function boot() {
  const response = await fetch("./climb.wasm");
  const bytes = await response.arrayBuffer();
  const { instance } = await WebAssembly.instantiate(bytes, {});
  const e = instance.exports;
  api = {
    memory: e.memory,
    start: e.start,
    set_input: e.set_input,
    tick: e.tick,
    state_ptr: e.state_ptr,
    state_len: e.state_len,
    terrain_ptr: e.terrain_ptr,
    terrain_len: e.terrain_len,
    coins_ptr: e.coins_ptr,
    coins_len: e.coins_len,
    set_upgrades: e.set_upgrades,
    set_grip_part: e.set_grip_part,
    upgrade_cost_at: e.upgrade_cost_at,
    reward_for_distance: e.reward_for_distance,
    roster_ptr: e.roster_ptr,
    roster_bytes: e.roster_bytes,
    roster_len: e.roster_len,
    vehicle_info_ptr: e.vehicle_info_ptr,
    vehicle_info_bytes: e.vehicle_info_bytes,
    levels_ptr: e.levels_ptr,
    levels_bytes: e.levels_bytes,
    levels_len: e.levels_len,
  };
  // Call the exporters BEFORE touching memory.buffer: roster_ptr() builds the
  // string on first call, that allocation grows the wasm memory and detaches
  // the previous ArrayBuffer. Reading buffer first yields
  // "Cannot perform Construct on a detached ArrayBuffer".
  const rosterPtr = api.roster_ptr();
  const rosterBytes = api.roster_bytes();
  const rosterText = new TextDecoder().decode(
    new Uint8Array(api.memory.buffer, rosterPtr, rosterBytes),
  );
  // Same ordering rule for the other blobs: read every pointer and length
  // before touching memory.buffer, because the first call may grow memory.
  const infoPtr = api.vehicle_info_ptr();
  const infoBytes = api.vehicle_info_bytes();
  const levelPtr = api.levels_ptr();
  const levelBytes = api.levels_bytes();
  const infoText = new TextDecoder().decode(
    new Uint8Array(api.memory.buffer, infoPtr, infoBytes),
  );
  const levelText = new TextDecoder().decode(
    new Uint8Array(api.memory.buffer, levelPtr, levelBytes),
  );

  MANIFEST = await fetch("./assets/manifest.json").then((r) => r.json());
  const art = new Map(MANIFEST.cars.map((car) => [car.id, car]));

  VEHICLES = infoText.split("\n").map((line, id) => {
    const [realId, name, halfW] = line.split("|");
    const car = art.get(realId) || {};
    return {
      id,
      name,
      realId,
      halfW: Number(halfW) || 1.2,
      chassis: car.chassis
        ? `./assets/cars/${realId}/${car.chassis}`
        : (ART[id] || ART[0])[0],
      tire: car.tire
        ? `./assets/cars/${realId}/${car.tire}`
        : "./assets/cars/tire-default.png",
      chassisPx: car.chassis_px || null,
    };
  });

  LEVELS = levelText.split("\n").map((line, id) => {
    const [name, family, distance, gravity] = line.split("|");
    return {
      id,
      name,
      family,
      distance: Number(distance) || 0,
      gravity: Number(gravity) || -10,
    };
  });

  const vehicleHost = document.getElementById("vehicles");
  vehicleHost.replaceChildren();
  choiceRow(vehicleHost, VEHICLES, () => vehicleId, (id) => {
    vehicleId = id;
    paintUpgrades();
  });

  const levelHost = document.getElementById("biomes");
  levelHost.replaceChildren();
  choiceRow(levelHost, LEVELS, () => levelId, (id) => { levelId = id; });

  paintUpgrades();
  hud.note.textContent = `pronto — ${VEHICLES.length} veículos, ${LEVELS.length} níveis`;
  VEHICLES.forEach((v) => { loadImage(v.chassis); loadImage(v.tire); });
  LEVELS.forEach((l) => loadImage(paletteFor(l.family).sky));
  loadImage("../textures/collectibles/battery.png");
  await loadHud();
}
boot().catch((error) => {
  hud.note.textContent = `falha ao abrir o wasm: ${error && error.message ? error.message : error}`;
  console.error(error);
});
requestAnimationFrame(draw);
