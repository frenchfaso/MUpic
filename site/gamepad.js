// Standard browser Gamepad layout -> the actual C64 control-port lines.
// Gamepad Y is positive down. No emulator state or browser polling lives here.
const IDLE = 0xff;
const ENGAGE = 0.25;
const RELEASE = 0.18;
const RUN_STOP = 63;
const ANALOG_DEADZONE = 0.15;
const ANALOG_MAX = 85;
const ANALOG_TAG = 0xa5;

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

function analogAxis(value) {
  if (!Number.isFinite(value) || Math.abs(value) <= ANALOG_DEADZONE) return 0;
  const travel = (Math.min(1, Math.abs(value)) - ANALOG_DEADZONE) / (1 - ANALOG_DEADZONE);
  // Match native Rust rounding: round the positive magnitude, then apply sign.
  const magnitude = Math.round(travel * Math.sqrt(travel) * ANALOG_MAX);
  return value < 0 ? -magnitude : magnitude;
}

function movementAxis(value, negative, positive) {
  if (negative && positive) return 0;
  if (negative) return -ANALOG_MAX;
  if (positive) return ANALOG_MAX;
  return analogAxis(value);
}

function samePaddles(left, right) {
  if (left === null || right === null) return left === right;
  return left[0][0] === right[0][0] && left[0][1] === right[0][1] &&
    left[1][0] === right[1][0] && left[1][1] === right[1][1];
}

export class GamepadInput {
  constructor() {
    this.last = [IDLE, IDLE];
    this.menu = false;
    this.paddles = null;
    this.wolf = false;
    this.slots = [null, null]; // First controller -> port 2; second -> port 1.
    this.axes = new Map();
  }

  // Returns only changes, ready for Worker.postMessage(). `focused` must be
  // false while the page is hidden/unfocused, loading or paused.
  update(gamepads, { wolf = false, focused = true } = {}) {
    const events = [];
    if (this.wolf !== Boolean(wolf)) {
      events.push(...this.reset());
      this.wolf = Boolean(wolf);
    }
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
    // Wolf reserves both physical ports for its primary controller.
    const candidates = [...connected.keys()].sort((a, b) => a - b);
    for (let slot = 0; slot < (this.wolf ? 1 : 2); slot++) {
      if (this.slots[slot] === null) {
        this.slots[slot] = candidates.find((index) => !this.slots.includes(index)) ?? null;
      }
    }

    const output = [IDLE, IDLE]; // Indexed by C64 port minus one.
    let menu = false;
    let paddles = null;
    if (focused) {
      for (let slot = 0; slot < (this.wolf ? 1 : 2); slot++) {
        const pad = connected.get(this.slots[slot]);
        if (!pad) continue;
        const previous = this.axes.get(pad.index) || [0, 0, 0];
        const state = [axis(pad, 0, previous[0]), axis(pad, 1, previous[1]), axis(pad, 2, previous[2])];
        this.axes.set(pad.index, state);
        output[slot === 0 ? 1 : 0] = lines(
          state[1] < 0 || button(pad, 12), state[1] > 0 || button(pad, 13),
          state[0] < 0 || button(pad, 14), state[0] > 0 || button(pad, 15), button(pad, this.wolf ? 1 : 0));
        if (this.wolf) {
          // Wolf64U's independent action ABI, shared with its native host:
          // East use/menu confirm, South/RT shoot, West/North weapon cycle.
          output[0] = lines(button(pad, 2), button(pad, 3),
            state[2] < 0, state[2] > 0, button(pad, 0) || button(pad, 7));
          menu = button(pad, 9);
          // Native Wolf64 signed-byte SID POT adapter. Digital directions stay
          // available for firmware/menu navigation; gameplay uses these exact
          // progressive magnitudes whenever the port-1 Y presence tag is set.
          paddles = [
            [analogAxis(pad.axes?.[2]) & 255, ANALOG_TAG],
            [movementAxis(pad.axes?.[0], button(pad, 14), button(pad, 15)) & 255,
              movementAxis(-pad.axes?.[1], button(pad, 13), button(pad, 12)) & 255],
          ];
        }
      }
    } else this.axes.clear();
    for (let index = 0; index < output.length; index++) {
      if (output[index] !== this.last[index]) {
        events.push({ type: "joystick", port: index + 1, lines: output[index] });
        this.last[index] = output[index];
      }
    }
    if (!samePaddles(paddles, this.paddles)) {
      events.push({ type: "paddles", values: paddles });
      this.paddles = paddles;
    }
    if (menu !== this.menu) {
      events.push({ type: "key", index: RUN_STOP, down: menu });
      this.menu = menu;
    }
    return events;
  }

  // Use before replacing a worker and on loss of focus. Explicit neutral
  // messages also recover from an interrupted polling frame.
  reset() {
    const events = [
      { type: "joystick", port: 1, lines: IDLE },
      { type: "joystick", port: 2, lines: IDLE },
      { type: "paddles", values: null },
    ];
    if (this.menu) events.push({ type: "key", index: RUN_STOP, down: false });
    this.last = [IDLE, IDLE];
    this.menu = false;
    this.paddles = null;
    this.slots = [null, null];
    this.axes.clear();
    return events;
  }
}
