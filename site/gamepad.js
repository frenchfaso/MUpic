// Standard browser Gamepad layout -> the actual C64 control-port lines.
// Gamepad Y is positive down. No emulator state or browser polling lives here.
const IDLE = 0xff;
const ENGAGE = 0.25;
const RELEASE = 0.18;
function axis(pad, index, previous) {
  const raw = pad.axes?.[index];
  const value = Number.isFinite(raw) ? Math.max(-1, Math.min(1, raw)) : 0;
  // A separate release threshold prevents a resting stick from chattering.
  if (value > ENGAGE) return 1;
  if (value < -ENGAGE) return -1;
  if (previous === 1 && value > RELEASE) return 1;
  if (previous === -1 && value < -RELEASE) return -1;
  return 0;
}

function button(pad, index) {
  const value = pad.buttons?.[index];
  return value?.pressed === true || (Number.isFinite(value?.value) && value.value > 0.5);
}

function lines(up, down, left, right, fire) {
  let result = IDLE;
  // Opposites cancel, including disagreements between stick and D-pad.
  if (up !== down) result &= ~(up ? 1 : 2);
  if (left !== right) result &= ~(left ? 4 : 8);
  if (fire) result &= ~16;
  return result;
}

export class GamepadInput {
  constructor() {
    this.last = [IDLE, IDLE];
    this.slots = [null, null]; // First controller -> port 2; second -> port 1.
    this.axes = new Map();
  }

  // Returns only changes, ready for Worker.postMessage(). `focused` must be
  // false while the page is hidden/unfocused, loading or paused.
  update(gamepads, { focused = true } = {}) {
    const events = [];
    const connected = new Map();
    for (const pad of gamepads || []) {
      // Unknown layouts have no portable button positions. The browser maps
      // supported Xbox/PlayStation/8BitDo-style controllers to "standard".
      if (!pad || pad.connected === false || pad.mapping !== "standard" ||
          !Number.isInteger(pad.index) || pad.index < 0 || connected.has(pad.index)) continue;
      connected.set(pad.index, pad);
    }
    for (const index of this.axes.keys()) {
      if (!connected.has(index)) this.axes.delete(index);
    }
    for (let slot = 0; slot < this.slots.length; slot++) {
      if (!connected.has(this.slots[slot])) this.slots[slot] = null;
    }
    // Keep surviving controllers on their ports when another disconnects.
    const candidates = [...connected.keys()].sort((a, b) => a - b);
    for (let slot = 0; slot < this.slots.length; slot++) {
      if (this.slots[slot] === null) {
        this.slots[slot] = candidates.find((index) => !this.slots.includes(index)) ?? null;
      }
    }

    const output = [IDLE, IDLE]; // Indexed by C64 port minus one.
    if (focused) {
      for (let slot = 0; slot < this.slots.length; slot++) {
        const pad = connected.get(this.slots[slot]);
        if (!pad) continue;
        const previous = this.axes.get(pad.index) || [0, 0];
        const state = [axis(pad, 0, previous[0]), axis(pad, 1, previous[1])];
        this.axes.set(pad.index, state);
        output[slot === 0 ? 1 : 0] = lines(
          state[1] < 0 || button(pad, 12), state[1] > 0 || button(pad, 13),
          state[0] < 0 || button(pad, 14), state[0] > 0 || button(pad, 15), button(pad, 0));

      }
    } else this.axes.clear();
    for (let index = 0; index < output.length; index++) {
      if (output[index] !== this.last[index]) {
        events.push({ type: "joystick", port: index + 1, lines: output[index] });
        this.last[index] = output[index];
      }
    }
    return events;
  }

  // Use before replacing a worker and on loss of focus. Explicit neutral
  // messages also recover from an interrupted polling frame.
  reset() {
    const events = [
      { type: "joystick", port: 1, lines: IDLE },
      { type: "joystick", port: 2, lines: IDLE },
    ];
    this.last = [IDLE, IDLE];
    this.slots = [null, null];
    this.axes.clear();
    return events;
  }
}
