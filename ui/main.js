"use strict";

// ---------------------------------------------------------------------------
// Backend bridge (falls back to an in-browser mock so the UI can be previewed
// by just opening index.html in a browser).
// ---------------------------------------------------------------------------
const tauri = window.__TAURI__;
const invoke = tauri ? tauri.core.invoke : mockInvoke;

const $ = (id) => document.getElementById(id);

const ICONS = {
  speaker:
    '<svg viewBox="0 0 24 24" fill="currentColor"><path d="M4 9h3l5-4v14l-5-4H4z"/><path d="M15.5 8.5a5 5 0 0 1 0 7" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"/><path d="M18 6a8.5 8.5 0 0 1 0 12" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"/></svg>',
  speakerMuted:
    '<svg viewBox="0 0 24 24" fill="currentColor"><path d="M4 9h3l5-4v14l-5-4H4z"/><path d="m16 9 6 6m0-6-6 6" stroke="currentColor" stroke-width="2" stroke-linecap="round"/></svg>',
  headphones:
    '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M4 15v-3a8 8 0 0 1 16 0v3"/><rect x="3" y="14" width="4" height="6" rx="1.5" fill="currentColor"/><rect x="17" y="14" width="4" height="6" rx="1.5" fill="currentColor"/></svg>',
  display:
    '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><rect x="3" y="4" width="18" height="12" rx="2"/><path d="M9 20h6M12 16v4"/></svg>',
  mic: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><rect x="9" y="3" width="6" height="11" rx="3" fill="currentColor"/><path d="M5 11a7 7 0 0 0 14 0M12 18v3"/></svg>',
  micMuted:
    '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><rect x="9" y="3" width="6" height="11" rx="3" fill="currentColor"/><path d="M5 11a7 7 0 0 0 14 0M12 18v3M4 4l16 16"/></svg>',
  check:
    '<svg class="check" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.6" stroke-linecap="round" stroke-linejoin="round"><path d="m5 12.5 4.5 4.5L19 7.5"/></svg>',
};

function deviceIcon(device) {
  const n = device.name.toLowerCase();
  if (device.flow === "input") return ICONS.mic;
  if (/head(phone|set)|airpods|buds|earphone|xm[345]|hands-free/.test(n)) return ICONS.headphones;
  if (/hdmi|displayport|monitor|nvidia|amd high definition|intel.*display|\btv\b/.test(n)) return ICONS.display;
  return ICONS.speaker;
}

// ---------------------------------------------------------------------------
// State + rendering
// ---------------------------------------------------------------------------
let devices = [];
let interacting = false; // true while a slider is being dragged
const busy = new Set();

function showError(message) {
  const el = $("error");
  el.textContent = message || "";
  el.hidden = !message;
  fitWindow();
}

async function call(cmd, args) {
  try {
    const result = await invoke(cmd, args);
    showError("");
    return result;
  } catch (e) {
    showError(String(e));
    throw e;
  }
}

async function refresh() {
  if (interacting) return;
  try {
    devices = await call("get_devices");
    render();
  } catch {
    /* error already shown */
  }
}

function setSliderFill(slider) {
  slider.style.setProperty("--value", `${slider.value}%`);
}

/** Throttled volume setter so dragging doesn't flood the backend. */
function volumeSetter() {
  let pending = null;
  let timer = null;
  return (id, value) => {
    pending = { id, volume: value / 100 };
    if (timer) return;
    timer = setTimeout(async () => {
      timer = null;
      const args = pending;
      pending = null;
      await call("set_volume", args).catch(() => {});
    }, 40);
  };
}

function bindSlider(slider, getId) {
  const setVolume = volumeSetter();
  slider.oninput = () => {
    interacting = true;
    setSliderFill(slider);
    const id = getId();
    if (id) setVolume(id, Number(slider.value));
  };
  slider.onchange = () => {
    interacting = false;
    const id = getId();
    const device = devices.find((d) => d.id === id);
    if (device) device.volume = Number(slider.value) / 100;
  };
}

function renderMaster(flow) {
  const device = devices.find((d) => d.flow === flow && d.isDefault);
  const slider = $(`${flow}-volume`);
  const mute = $(`${flow}-mute`);
  const isOut = flow === "output";

  slider.disabled = !device;
  mute.disabled = !device;
  if (!interacting) slider.value = device ? Math.round(device.volume * 100) : 0;
  setSliderFill(slider);

  const muted = device?.muted;
  mute.classList.toggle("muted", !!muted);
  mute.innerHTML = isOut ? (muted ? ICONS.speakerMuted : ICONS.speaker) : muted ? ICONS.micMuted : ICONS.mic;
  mute.title = muted ? "Unmute" : "Mute";
}

function renderList(flow) {
  const list = $(`${flow}-list`);
  const items = devices.filter((d) => d.flow === flow);
  list.replaceChildren();

  if (!items.length) {
    const li = document.createElement("li");
    li.className = "empty";
    li.textContent = flow === "output" ? "No output devices" : "No input devices";
    list.append(li);
    return;
  }

  for (const device of items) {
    const li = document.createElement("li");
    li.className = "device";
    li.classList.toggle("active", device.isDefault);
    li.classList.toggle("mirrored", device.mirrored);
    li.classList.toggle("busy", busy.has(device.id));
    li.title = device.isDefault ? `${device.name} (default)` : `Switch to ${device.name}`;

    const icon = document.createElement("span");
    icon.className = "dev-icon";
    icon.innerHTML = deviceIcon(device);

    const name = document.createElement("span");
    name.className = "name";
    name.textContent = device.name;

    const end = document.createElement("span");
    if (device.isDefault) {
      end.innerHTML = ICONS.check;
    } else if (flow === "output") {
      // "Also play here" switch: mirror the default output to this device.
      end.className = "toggle";
      end.title = device.mirrored ? "Stop playing on this device" : "Also play on this device";
      end.innerHTML = `<input type="checkbox" ${device.mirrored ? "checked" : ""} aria-label="Also play on ${device.name.replace(/"/g, "")}"><span class="switch"></span>`;
      end.onclick = (e) => {
        e.stopPropagation();
        setMirror(device, !device.mirrored);
      };
    }

    li.append(icon, name, end);

    if (device.mirrored) {
      const sub = document.createElement("div");
      sub.className = "sub";
      const slider = document.createElement("input");
      slider.type = "range";
      slider.className = "slider small";
      slider.min = 0;
      slider.max = 100;
      slider.value = Math.round(device.volume * 100);
      setSliderFill(slider);
      bindSlider(slider, () => device.id);
      slider.onclick = (e) => e.stopPropagation();
      sub.append("Also playing", slider);
      li.append(sub);
    }

    li.onclick = () => {
      if (!device.isDefault) setDefault(device);
    };
    list.append(li);
  }
}

function render() {
  renderMaster("output");
  renderMaster("input");
  renderList("output");
  renderList("input");

  const mirrors = devices.filter((d) => d.mirrored).length;
  const hint = $("mirror-hint");
  hint.hidden = mirrors === 0;
  hint.textContent = `${mirrors + 1} outputs`;
  fitWindow();
}

// ---------------------------------------------------------------------------
// Actions
// ---------------------------------------------------------------------------
async function withBusy(device, fn) {
  busy.add(device.id);
  render();
  try {
    await fn();
  } catch {
    /* shown */
  } finally {
    busy.delete(device.id);
    await refresh();
  }
}

const setDefault = (device) =>
  withBusy(device, () => call("set_default_device", { id: device.id, flow: device.flow }));

const setMirror = (device, enabled) => withBusy(device, () => call("set_mirror", { id: device.id, enabled }));

async function toggleMute(flow) {
  const device = devices.find((d) => d.flow === flow && d.isDefault);
  if (!device) return;
  device.muted = !device.muted;
  renderMaster(flow);
  await call("set_mute", { id: device.id, muted: device.muted }).catch(() => {});
  refresh();
}

let lastHeight = 0;
function fitWindow() {
  requestAnimationFrame(() => {
    const height = Math.ceil($("panel").getBoundingClientRect().height);
    if (height !== lastHeight) {
      lastHeight = height;
      invoke("resize_widget", { height }).catch(() => {});
    }
  });
}

// ---------------------------------------------------------------------------
// Wiring
// ---------------------------------------------------------------------------
const defaultId = (flow) => devices.find((d) => d.flow === flow && d.isDefault)?.id;
bindSlider($("output-volume"), () => defaultId("output"));
bindSlider($("input-volume"), () => defaultId("input"));
$("output-mute").onclick = () => toggleMute("output");
$("input-mute").onclick = () => toggleMute("input");
$("open-settings").onclick = () => invoke("open_sound_settings").catch(() => {});

document.addEventListener("keydown", (e) => {
  if (e.key === "Escape") invoke("hide_widget").catch(() => {});
});
document.addEventListener("contextmenu", (e) => e.preventDefault());
window.addEventListener("pointerup", () => (interacting = false));

const autostart = $("autostart");
invoke("plugin:autostart|is_enabled")
  .then((on) => (autostart.checked = on))
  .catch(() => {});
autostart.onchange = () =>
  invoke(autostart.checked ? "plugin:autostart|enable" : "plugin:autostart|disable").catch((e) => {
    autostart.checked = !autostart.checked;
    showError(String(e));
  });

if (tauri) tauri.event.listen("widget-shown", refresh);
window.addEventListener("focus", refresh);
setInterval(() => {
  if (document.hasFocus()) refresh();
}, 2500);
refresh();

// ---------------------------------------------------------------------------
// Browser-only mock backend
// ---------------------------------------------------------------------------
function mockInvoke(cmd, args = {}) {
  const db = (mockInvoke.db ??= [
    { id: "o1", name: "Speakers (Realtek(R) Audio)", flow: "output", isDefault: true, volume: 0.62, muted: false, mirrored: false },
    { id: "o2", name: "Headphones (WH-1000XM5)", flow: "output", isDefault: false, volume: 0.4, muted: false, mirrored: true },
    { id: "o3", name: "LG HDR 4K (NVIDIA High Definition Audio)", flow: "output", isDefault: false, volume: 0.8, muted: false, mirrored: false },
    { id: "i1", name: "Microphone Array (Realtek(R) Audio)", flow: "input", isDefault: true, volume: 0.75, muted: false, mirrored: false },
    { id: "i2", name: "Headset Microphone (WH-1000XM5)", flow: "input", isDefault: false, volume: 0.5, muted: false, mirrored: false },
  ]);
  const find = (id) => db.find((d) => d.id === id);
  switch (cmd) {
    case "get_devices":
      return Promise.resolve(structuredClone(db));
    case "set_default_device":
      db.filter((d) => d.flow === args.flow).forEach((d) => (d.isDefault = d.id === args.id));
      find(args.id).mirrored = false;
      return Promise.resolve();
    case "set_mirror":
      find(args.id).mirrored = args.enabled;
      return Promise.resolve();
    case "set_volume":
      find(args.id).volume = args.volume;
      return Promise.resolve();
    case "set_mute":
      find(args.id).muted = args.muted;
      return Promise.resolve();
    case "plugin:autostart|is_enabled":
      return Promise.resolve(false);
    default:
      return Promise.resolve();
  }
}
