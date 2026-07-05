# `controller.ytx` schema reference

A working reference for the Yaeltex controller config format as used by this repo.

This document is intentionally split into:

- **Known**: confirmed from direct testing or from the Yaeltex UI
- **Observed**: present in `config/controller.ytx`, but not fully semantically validated
- **Inferred**: likely meaning based on behavior and current usage, but still needs confirmation

The goal is to make it obvious what we **know**, what we are **assuming**, and what still needs hardware/UI verification.

---

## Scope

This reference is based on:

- `config/controller.ytx`
- current project behavior
- direct UI enum observations collected during setup

It does **not** claim to be an official Yaeltex schema spec.

---

## Top-level structure

Observed top-level keys:

```json
{
  "hwconfig": { ... },
  "banks": { ... }
}
```

### `hwconfig`
Global hardware/config metadata.

### `banks`
Per-bank control definitions.
In the current config there is only:

- `banks["0"]`

---

## Confidence legend

- **Known** = directly confirmed from UI or hardware behavior
- **Observed** = present in file, shape verified
- **Inferred** = likely meaning, but not yet fully confirmed
- **Likely** = strong reverse-engineering / UI evidence, but not yet exhaustively tested in every context

---

# 1. `hwconfig`

`hwconfig` contains hardware inventory, identity/version info, color tables, and some global routing/settings.

## 1.1 Hardware inventory arrays

Observed keys:

- `digital1`
- `digital2`
- `encoders`
- `analog1`
- `analog2`
- `analog_exp1`
- `analog_exp2`
- `feedbacks`

Example:

```json
"encoders": ["ENC41H", "ENC41H", "ENC41H", "ENC41H", "ENC41H", "ENC41H", "ENC41H", "NONE"]
```

### Meaning
- **Observed**: these are arrays of hardware part identifiers.
- **Inferred**: they describe the physical bill of materials / controller layout for different rows or zones.
- **Unknown**: exact positional mapping of each array element to the physical panel.

### Common observed values
- `RB42`
- `RB82`
- `RB41`
- `ENC41H`
- `P41`
- `F41`
- `NONE`

These appear to be Yaeltex hardware module identifiers.

---

## 1.2 Identity / version / build info

Observed keys:

- `device_name`
- `usb_pid`
- `usb_serial`
- `fw_version_major`
- `fw_version_minor`
- `hw_version_major`
- `hw_version_minor`
- `config_version_major`
- `config_version_minor`
- `signature`
- `bootflag`
- `reserved1`

### Meaning
- **Observed**: these fields store device and config metadata.
- **Inferred**:
  - `fw_version_*` = firmware version
  - `hw_version_*` = hardware revision
  - `config_version_*` = config format/app version
  - `signature` = internal file/signature marker
  - `bootflag`, `reserved1` = low-level/internal controller fields

These should generally be treated as controller-owned metadata unless there is a good reason to edit them.

---

## 1.3 Global behavior / bank settings

Observed keys:

- `midiMergeOption`
- `banksNumber`
- `banksModes`
- `banksIds`
- `takeover`
- `rainbow`
- `remoteBanks`
- `factoryReset`
- `dumpStateOnStartup`
- `rememberState`
- `qtyMsg7bit`
- `qtyMsg14bit`

### Meaning

#### `banksNumber`
- **Observed**: current value is `1`
- **Inferred**: number of hardware banks configured in the controller

#### `banksModes`
- **Observed**: array of numeric values
- **Inferred**: per-hardware-bank mode setting
- **Unknown**: enum meanings not yet documented

#### `banksIds`
- **Observed**: array of numeric values, currently `65535`
- **Inferred**: bank identifiers or disabled/unassigned bank slots

#### `remoteBanks`
- **Observed**: numeric field at hwconfig level
- **Inferred**: enables/disables Yaeltex remote bank switching feature
- Current project avoids relying on whole-surface hardware bank switching

#### `takeover`
- **Inferred**: likely control pickup/takeover behavior for analog/encoder state

#### `rainbow`
- **Observed**: boolean
- **Unknown**: likely UI/device color behavior option

#### `factoryReset`
- **Inferred**: reset flag / internal utility field

#### `dumpStateOnStartup`
- **Inferred**: whether device dumps current state at startup

#### `rememberState`
- **Inferred**: whether last values are persisted/restored

#### `qtyMsg7bit` / `qtyMsg14bit`
- **Observed**: numeric counts
- **Inferred**: message allocation/count metadata used internally by Yaeltex config tooling/firmware

---

## 1.4 Color tables

Observed keys:

- `switchFeedbackColorList`
- `analogFeedbackColorList`
- `digitalFeedbackColorList`

Each is a map from color name to hex RGB string.

Example:

```json
"WHITE": "DDDDDD"
```

### Meaning
- **Observed**: built-in palette definitions used by the config/UI.
- **Known**: these are not the runtime MIDI values we send from Rust; they are part of the Yaeltex config representation.

---

## 1.5 `specialChannels`

Observed keys:

- `valueToColor`
- `valueToIntensity`
- `vuMeter`
- `splitMode`
- `remoteBanks`

### Meaning
- **Observed**: channel numbers for special feedback/control behaviors.
- **Known**: this repo uses the `valueToIntensity` channel behavior.
- **Observed**: current config has:
  - `valueToColor = 16`
  - `valueToIntensity = 15`
- **Known**: in our Rust code this corresponds to using special color/intensity channels on the wire according to the controller config system.
- **Observed in hardware testing**: the controller appears sensitive to large bursts of LED updates; sparse feedback updates are more reliable than dense animated ones.

### Important note on channel numbering
- **Known**: channels in the `.ytx` file are **zero-based**.
  - file `channel: 0` => MIDI channel 1
  - file `channel: 1` => MIDI channel 2

This matches what we observed while configuring the controller.

---

# 2. `banks`

Observed shape:

```json
"banks": {
  "0": {
    "encoders": { ... },
    "digitals": { ... },
    "analogs": { ... },
    "feedbacks": { ... },
    "button": "None",
    "midi_ch": null,
    "mode": "Toggle",
    "color": null
  }
}
```

## 2.1 Bank root fields

### `encoders`
Map of encoder definitions.
Current config contains 28 entries: `"0" .. "27"`.

### `digitals`
Map of digital button/pad definitions.
Current config contains 140 entries: `"0" .. "139"`.

### `analogs`
Map of analog control definitions.
Current config contains 12 entries: `"0" .. "11"`.

### `feedbacks`
- **Observed**: currently empty object `{}`
- **Unknown**: likely reserved for dedicated feedback elements or alternate config paths

### `button`
- **Observed**: currently `"None"`
- **Unknown**: likely bank button assignment

### `midi_ch`
- **Observed**: currently `null`
- **Unknown**: perhaps bank-level default MIDI channel

### `mode`
- **Observed**: current value `"Toggle"`
- **Unknown**: bank behavior mode string enum

### `color`
- **Observed**: currently `null`
- **Unknown**: likely bank UI color metadata

---

# 3. Digital entries: `banks.*.digitals.*`

Observed shape:

```json
{
  "action_config": { ... },
  "feedback_config": { ... },
  "value": 0
}
```

## 3.1 `action_config`

Observed keys:

- `toggle_momentary`
- `midi_port`
- `message`
- `channel`
- `parameter`
- `parameter_lsb`
- `parameter_msb`
- `parameter_min`
- `parameter_min_lsb`
- `parameter_min_msb`
- `parameter_max`
- `parameter_max_lsb`
- `parameter_max_msb`
- `key`
- `modifier`
- `comment`

### Field notes

#### `toggle_momentary`
- **Likely**: matches the UI's momentary/toggle selector

Likely enum mapping:

- `0` = MOMENTARY
- `1` = TOGGLE

#### `midi_port`
- **Observed**: numeric, current config uses `3`
- **Known**: this is **not** the MIDI channel
- **Unknown**: exact Yaeltex routing semantics
- **Important project rule**: treat this as an internal / UI-hidden Yaeltex field and preserve working values unless explicitly tested

Do not reinterpret `midi_port` as channel data. When changing mappings, the fields that should normally be edited are `channel`, `message`, and `parameter`.

#### `message`
- **Likely**: digital `action_config.message` uses the same enum as `switch_config.message`

Likely enum mapping:

- `0` = NOTE
- `1` = CC
- `2` = PC #
- `3` = PC -
- `4` = PC +
- `5` = NPRn
- `6` = RPN
- `7` = PITCH BEND
- `8` = KEY STROKE

#### `channel`
- **Known**: zero-based MIDI channel number

#### `parameter` / `parameter_lsb` / `parameter_msb`
- **Observed**: numeric message parameter fields
- **Inferred**:
  - for note-like messages this is the note number / main parameter
  - for CC-like messages this is the CC number
  - LSB/MSB fields support wider message formats

#### `parameter_min*` / `parameter_max*`
- **Observed**: min/max range fields
- **Inferred**: define control output range for the assigned message type

#### `key` / `modifier`
- **Inferred**: keyboard-emulation related fields when message type is keyboard/keystroke-like

#### `comment`
- **Observed**: UI label/comment field

---

## 3.2 `feedback_config`

Observed keys:

- `source`
- `message`
- `channel`
- `local_behaviour`
- `color_range_enable`
- `parameter`
- `parameter_lsb`
- `parameter_msb`
- `color`
- `low_intensity_off_mode`
- `value_to_intensity`

### Field notes

#### `source`
- **Likely**: source is a bitmask-style combination of USB / MIDI / LOCAL

Likely enum mapping:

- `1` = USB
- `2` = MIDI
- `3` = USB + MIDI
- `4` = LOCAL
- `5` = USB + LOCAL
- `6` = MIDI + LOCAL
- `7` = USB + MIDI + LOCAL

This appears consistent across encoder and digital feedback configs, and likely analog feedback as well.

#### `message`
- **Likely**: digital `feedback_config.message` uses the same enum as `switch_feedback.message`

Likely enum mapping:

- `0` = NOTE
- `1` = CC
- `2` = PC #
- `3` = PC -
- `4` = PC +
- `5` = NPRn
- `6` = RPN
- `7` = PITCH BEND

#### `channel`
- **Known**: zero-based MIDI channel number

#### `local_behaviour`
- **Likely**: only appears when source includes LOCAL

Likely enum mapping:

- `0` = ON WITH PRESS
- `1` = ALWAYS ON

#### `color_range_enable`
- **Known**: `1` is required for color-range capable digital feedback in this project

#### `parameter*`
- **Observed**: feedback message target parameter(s)

#### `color`
- **Observed**: RGB triplet array like `[255, 255, 145]`
- **Inferred**: UI/default color metadata used by Yaeltex

#### `low_intensity_off_mode`
- **Likely**: only available when `color_range_enable = 0`

Likely enum mapping:

- `0` = LED OFF
- `1` = LOW INTENSITY

#### `value_to_intensity`
- **Known**:
  - `0` = do not map incoming value to intensity
  - `1` = use incoming value as intensity
- **Observed in hardware testing**:
  - encoder `switch_feedback.value_to_intensity = 1` works for switch-label intensity
  - encoder `rotation_feedback.value_to_intensity = 1` appears unstable when enabled across larger parts of the fixed encoder area
  - current safe project default is to avoid relying on encoder ring intensity animation

---

## 3.3 `value`
- **Observed**: numeric current/stored value field
- **Inferred**: may represent remembered state / current UI value

---

# 4. Analog entries: `banks.*.analogs.*`

Observed shape:

```json
{
  "general_analog": { ... },
  "message_config": { ... },
  "feedback_config": { ... },
  "value": 0,
  "has_feedback": false
}
```

## 4.1 `general_analog`

Observed keys:

- `analog_type`

### `analog_type`
- **Observed**: numeric, current config uses `3`
- **Unknown**: this field does not appear to exist directly in the UI, so its meaning is currently unclear.
- **Inferred**: likely low-level hardware/control-type metadata rather than a normal user-facing parameter.

---

## 4.2 `message_config`

Observed keys:

- `midi_port`
- `dead_zone`
- `split_mode`
- `message`
- `channel`
- `parameter`
- `parameter_lsb`
- `parameter_msb`
- `parameter_min`
- `parameter_min_lsb`
- `parameter_min_msb`
- `parameter_max`
- `parameter_max_lsb`
- `parameter_max_msb`
- `key`
- `modifier`
- `comment`

### Field notes

#### `message`
- **Likely**: analog `message_config.message` uses the same enum family as `rotary_config.message`

Known rotary-style enum mapping:

- `0` = NONE
- `1` = NOTE
- `2` = CC
- `4` = VUMETER CC
- `5` = PROGRAM CHANGE #
- `6` = NRPN
- `7` = RPN
- `8` = PITCH BEND

#### `dead_zone`
- **Inferred**: analog dead-zone setting

#### `split_mode`
- **Inferred**: special analog split behavior
- See also `hwconfig.specialChannels.splitMode`

#### `channel`
- **Known**: zero-based MIDI channel number

#### `parameter*`, `parameter_min*`, `parameter_max*`
Same general interpretation as digital entries.

#### `comment`
- **Observed**: label/comment, sometimes null-padded in the stored JSON

---

## 4.3 `feedback_config`

Observed keys are the same general shape as digital feedback:

- `source`
- `message`
- `channel`
- `local_behaviour`
- `color_range_enable`
- `parameter`
- `parameter_lsb`
- `parameter_msb`
- `color`
- `low_intensity_off_mode`
- `value_to_intensity`

### Notes
- **Observed**: current analogs often have `has_feedback = false`
- **Observed**: current analog feedback config exists even when feedback is disabled
- **Unknown**: exact semantics when `has_feedback = false`

---

## 4.4 `has_feedback`
- **Observed**: boolean
- **Inferred**: enables/disables analog feedback behavior

---

# 5. Encoder entries: `banks.*.encoders.*`

Observed shape:

```json
{
  "encoder_mode": { ... },
  "rotary_config": { ... },
  "rotation_feedback": { ... },
  "switch_config": { ... },
  "switch_feedback": { ... },
  "value": 0
}
```

This is the most important section for the current project.

---

## 5.1 `encoder_mode`

Observed keys:

- `hw_mode`
- `speed_value`

### `hw_mode`
- **Likely** enum mapping:
  - `0` = ABSOLUTE
  - `1` = BINARY OFFSET
  - `2` = 2 COMPONENT
  - `3` = SIGNED BIT
  - `4` = SIGNED BIT 2
  - `5` = SINGLE VALUE

### `speed_value`
- **Likely** enum mapping:
  - `0` = ACCEL 1
  - `1` = ACCEL 2
  - `2` = ACCEL 3
  - `3` = FIXED 1
  - `4` = FIXED 2
  - `5` = FIXED 3

---

## 5.2 `rotary_config`

Observed keys:

- `message`
- `channel`
- `midi_port`
- `parameter`
- `parameter_lsb`
- `parameter_msb`
- `parameter_min`
- `parameter_min_lsb`
- `parameter_min_msb`
- `parameter_max`
- `parameter_max_lsb`
- `parameter_max_msb`
- `key_left`
- `key_right`
- `modifier_left`
- `modifier_right`
- `comment`

### `rotary_config.message` enum
**Known from UI observation:**

- `0` = NONE
- `1` = NOTE
- `2` = CC
- `4` = VUMETER CC
- `5` = PROGRAM CHANGE #
- `6` = NRPN
- `7` = RPN
- `8` = PITCH BEND

### Notes
- **Known**: rotary message enum is **different** from switch message enum
- **Known**: this project uses:
  - `rotary_config.message = 2` for CC

Other fields follow the same general pattern:
- `channel` is zero-based MIDI channel
- `midi_port` is an internal/UI-hidden Yaeltex routing field, not MIDI channel
- `parameter*`, `parameter_min*`, `parameter_max*` define message identity and range
- `key_left/right`, `modifier_left/right` are for keyboard-style modes

---

## 5.3 `switch_config`

Observed keys:

- `type`
- `double_click`
- `mode`
- `message`
- `channel`
- `midi_port`
- `parameter`
- `parameter_lsb`
- `parameter_msb`
- `parameter_min`
- `parameter_min_lsb`
- `parameter_min_msb`
- `parameter_max`
- `parameter_max_lsb`
- `parameter_max_msb`
- `key`
- `key_left`
- `key_right`
- `modifier`
- `modifier_left`
- `modifier_right`
- `qstb_bank`
- `qstbn_note`
- `comment`

### `switch_config.message` enum
**Known from UI observation:**

- `0` = NOTE
- `1` = CC
- `2` = PC #
- `3` = PC -
- `4` = PC +
- `5` = NPRn
- `6` = RPN
- `7` = PITCH BEND
- `8` = KEY STROKE

### Notes
- **Known**: this enum does **not** match `rotary_config.message`
- **Known**: current project uses:
  - `switch_config.message = 1` for CC

### Other switch fields

#### `type`
- **Likely** enum mapping:
  - `0` = MOMENTARY
  - `1` = TOGGLE

#### `double_click`
- **Known** enum mapping:
  - `0` = NONE
  - `1` = JUMP TO MIN
  - `2` = JUMP TO CENTER
  - `3` = JUMP TO MAX

`JUMP TO CENTER` matches the previously observed behavior where double-clicking resets to center. Enabling any double-click mode also appears to introduce a small input delay while the controller waits to determine whether a second click is coming.

#### `mode`
- **Likely** enum mapping:
  - `0` = NONE
  - `1` = MIDI MESSAGE
  - `2` = SHIFT ROTARY ACTION
  - `3` = FINE ADJUST
  - `4` = DOUBLE CC

#### `qstb_bank` / `qstbn_note`
- **Observed**: numeric
- **Unknown**: likely related to special quick-step / bank-note functionality

---

## 5.4 `rotation_feedback`

Observed keys:

- `mode`
- `source`
- `channel`
- `message`
- `parameter`
- `parameter_lsb`
- `color_range_enable`
- `parameter_msb`
- `value_to_intensity`
- `color`

### `rotation_feedback.mode`
Likely enum mapping:

- `0` = SPOT
- `1` = MIRROR
- `2` = FILL
- `3` = PIVOT

`SPOT`, `FILL`, and `PIVOT` are already in active project use; `MIRROR` is newly documented from UI/reverse engineering and still needs real-world behavior confirmation.

### `source`
- **Likely**: source is a combination of USB / MIDI / LOCAL

Likely enum mapping:

- `1` = USB
- `2` = MIDI
- `3` = USB + MIDI
- `4` = LOCAL
- `5` = USB + LOCAL
- `6` = MIDI + LOCAL
- `7` = USB + MIDI + LOCAL

### `message`
- **Observed**: current encoder ring feedback uses `2`
- **Inferred**: likely same message enum family as `rotary_config.message`
- **Known in practice**: `2` works for CC-based ring feedback in this project

### `value_to_intensity`
- **Known**:
  - `0` = do not map incoming value to intensity
  - `1` = use incoming value as intensity
- Current project now expects:
  - `rotation_feedback.value_to_intensity = 1`

### `color_range_enable`
- **Known**: should be enabled when using dynamic color behavior

### `color`
- **Observed**: RGB triplet

---

## 5.5 `switch_feedback`

Observed keys:

- `source`
- `message`
- `channel`
- `local_behaviour`
- `color_range_enable`
- `parameter`
- `parameter_lsb`
- `parameter_msb`
- `color`
- `low_intensity_off_mode`
- `value_to_intensity`

### `switch_feedback.message` enum
**Known from UI observation:**

Same as `switch_config.message`, but without KEY STROKE:

- `0` = NOTE
- `1` = CC
- `2` = PC #
- `3` = PC -
- `4` = PC +
- `5` = NPRn
- `6` = RPN
- `7` = PITCH BEND

### Notes
- **Known**: current project uses:
  - `switch_feedback.message = 1` for CC

### `source`
- **Likely**: source is a combination of USB / MIDI / LOCAL

Likely enum mapping:

- `1` = USB
- `2` = MIDI
- `3` = USB + MIDI
- `4` = LOCAL
- `5` = USB + LOCAL
- `6` = MIDI + LOCAL
- `7` = USB + MIDI + LOCAL

### `value_to_intensity`
- **Known**: current project expects:
  - `switch_feedback.value_to_intensity = 1`

### `local_behaviour`
- **Likely**: only appears when source includes LOCAL

Likely enum mapping:

- `0` = ON WITH PRESS
- `1` = ALWAYS ON

### `low_intensity_off_mode`
- **Likely**: only available when `color_range_enable = 0`

Likely enum mapping:

- `0` = LED OFF
- `1` = LOW INTENSITY

---

## 5.6 `value`
- **Observed**: numeric stored/current encoder value
- **Inferred**: used for remembered state or UI persistence

---

# 6. Project-specific assumptions currently in use

These are the assumptions the Rust code and tracked config are currently built around.

## 6.1 MIDI channel numbering
- `.ytx` channels are zero-based
- Rust/app behavior is reasoned about in normal MIDI channel numbering

## 6.2 Encoder messaging
- rotary uses CC => `rotary_config.message = 2`
- switch uses CC => `switch_config.message = 1`
- switch feedback uses CC => `switch_feedback.message = 1`

## 6.3 Encoder ring modes
- banked encoders are Spot mode
- fixed row 1 uses Pivot mode
- other fixed rows use Fill/Pivot according to physical-slot role
- physical ring mode is treated as a **hardware-slot property**
- ring modes are extracted at build time from `config/controller.ytx` by `build.rs`

## 6.4 Intensity mapping
- encoder feedback expects `value_to_intensity = 1`
- digital color feedback uses `color_range_enable = 1`
- project also uses the Yaeltex special intensity feedback channel behavior

---

# 7. What we still do not know

These are the main schema gaps worth testing/documenting next.

## High value unknown enums
- analog `general_analog.analog_type` (still unclear; may be low-level/internal because it does not appear in the UI)
- `banksModes`
- exact semantics of `qstb_bank` / `qstbn_note`

## Practical hardware notes
- Avoid CC `99` and `101` in future mappings; user reports indicate these reserved NRPN/RPN-related CCs can cause odd LED behavior.
- The controller appears sensitive to high LED-update burst rates; user reports suggest encoder responsiveness can degrade around roughly `32 messages / 10ms`.

## Structural unknowns
- exact meaning of hardware inventory arrays in `hwconfig`
- exact semantics of `midi_port`
- exact role of bank root fields `button`, `midi_ch`, `color`, `mode`
- whether `value` fields are merely stored defaults or active runtime state

---

# 8. Suggested validation workflow

When testing unknown fields, capture three things:

1. **UI dropdown label**
2. **stored numeric value in `.ytx`**
3. **observed hardware/runtime behavior**

That lets us upgrade entries from:
- Observed -> Inferred -> Known

---

# 9. Related files

- `config/controller.ytx`
- `build.rs`
- `src/controllers/modulation_surface.rs`
- `docs/controller-schema-notes.md`

---

# 10. Note on `kilowhat`

The `Yaeltex/kilowhat` project may still be useful as:

- a reference for SysEx transfer/update mechanics
- a clue to historical field meanings
- a cross-check for names used internally by older Yaeltex tooling

But until compared directly against the current controller firmware/UI behavior, it should be treated as:

- **potentially helpful**, but
- **not authoritative for current schema semantics**

Especially for enum values and field meanings, the safest source remains:

1. current Yaeltex UI behavior
2. actual hardware testing
3. current `.ytx` files
