extern crate midir;
use self::midir::{MidiInputConnection};
use std::time::{Duration, SystemTime};
use std::sync::mpsc;
use std::thread;
use std::collections::HashSet;
use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::collections::hash_map::Entry::{Occupied, Vacant};
use std::sync::{Arc, Mutex};

use ::midi_connection;
use ::midi_time::MidiTime;

use ::output_value::OutputValue;
use ::loop_recorder::{LoopRecorder, LoopEvent};
use ::loop_state::{Loop, LoopState, LoopTransform};
use ::clock_source::{RemoteClock, ToClock, FromClock};
use ::chunk::{Triggerable, MidiMap, ChunkMap, Coords};
use ::scale::Scale;

const SIDE_BUTTONS: [u8; 8] = [8, 24, 40, 56, 72, 88, 104, 120];
const DEFAULT_VELOCITY: u8 = 100;

static volca_keys_channel: u8 = 13;
static sp404_channel: u8 = 11;
static volca_bass_channel: u8 = 14;

lazy_static! {
    static ref REPEAT_RATES: [MidiTime; 8] = [
        MidiTime::from_measure(2, 1),
        MidiTime::from_measure(1, 1),
        MidiTime::from_measure(2, 3),
        MidiTime::from_measure(1, 2),
        MidiTime::from_measure(1, 3),
        MidiTime::from_measure(1, 4),
        MidiTime::from_measure(1, 6),
        MidiTime::from_measure(1, 8)
    ];
}

#[derive(Debug, Copy, Clone)]
pub enum LoopGridMessage {
    Schedule(MidiTime, MidiTime),
    GridInput(u64, u32, OutputValue),
    LoopButton(bool),
    FlattenButton(bool),
    UndoButton(bool),
    RedoButton(bool),
    SuppressButton(bool),
    SelectButton(bool),
    HoldButton(bool),
    ScaleButton(bool),
    Event(LoopEvent),
    InitialLoop,
    RefreshInput(u32),
    RefreshOverride(u32),
    RefreshGridButton(u32),
    RefreshSelectionOverride,
    RefreshSideButtons,
    RefreshShouldFlatten,
    RefreshActive,
    RefreshRecording,
    RefreshSelectState,
    ClearSelection,
    RefreshUndoRedoLights,
    SetRate(MidiTime),
    RateButton(usize, bool),
    TriggerChunk(MidiMap, OutputValue),
    None
}

#[derive(Debug, Eq, PartialEq, Copy, Clone, Hash)]
pub struct PlaybackRange {
    from: MidiTime,
    to: MidiTime
}

impl PlaybackRange {
    pub fn new (from: MidiTime, to: MidiTime) -> PlaybackRange {
        PlaybackRange { from, to }
    }

    pub fn from (&self) -> MidiTime {
        self.from
    }

    pub fn to (&self) -> MidiTime {
        self.to
    }
}

#[derive(Debug, Eq, PartialEq, Copy, Clone, Hash)]
enum Light {
    Yellow = 127,
    YellowMed = 110,
    Lime = 126,
    LimeLow = 109,
    Green = 124,
    GreenMed = 108,
    GreenLow = 92,
    Orange = 95,
    OrangeMed = 111,
    OrangeLow = 93,
    Red = 79,
    RedMed = 78,
    RedLow = 77,
    Off = 0,
    None
}

impl Light {
    pub fn unwrap_or (self, value: Light) -> Light {
        match self {
            Light::None => value,
            _ => self
        }
    }

    pub fn maybe (self, expr: bool) -> Light {
        if expr {
            self
        } else {
            Light::None
        }
    }
}

pub struct LoopGridLaunchpad {
    port_name: String,
    input: MidiInputConnection<()>,
    tx: mpsc::Sender<LoopGridMessage>
}

impl LoopGridLaunchpad {
    pub fn new(launchpad_port_name: &str, chunk_map: Vec<Box<ChunkMap>>, scale: Arc<Mutex<Scale>>, clock: RemoteClock) -> Self {
        let (tx, rx) = mpsc::channel();
        
        let tx_input =  mpsc::Sender::clone(&tx);
        let tx_clock =  mpsc::Sender::clone(&tx);
        let tx_feedback =  mpsc::Sender::clone(&tx);
        let tx_loop_state =  mpsc::Sender::clone(&tx);

        let mut tick_pos = MidiTime::zero();

        let (midi_to_id, id_to_midi) = get_grid_map();

        let mut launchpad_output = midi_connection::get_output(&launchpad_port_name).unwrap();

        let input = midi_connection::get_input(&launchpad_port_name, move |stamp, message, _| {
            if message[0] == 144 || message[0] == 128 {
                let side_button = SIDE_BUTTONS.binary_search(&message[1]);
                let grid_button = midi_to_id.get(&message[1]);
                if side_button.is_ok() {
                    let rate_index = side_button.unwrap();
                    let active = message[2] > 0;
                    tx_input.send(LoopGridMessage::RateButton(rate_index, active)).unwrap();
                } else if grid_button.is_some() {
                    let value = if message[2] > 0 {
                        OutputValue::On(DEFAULT_VELOCITY)
                    } else {
                        OutputValue::Off
                    };

                    tx_input.send(LoopGridMessage::GridInput(stamp, *grid_button.unwrap(), value)).unwrap();
                } ;
            } else if message[0] == 176 {
                let active = message[2] > 0;
                let to_send = match message[1] {
                    104 => LoopGridMessage::LoopButton(active),
                    105 => LoopGridMessage::FlattenButton(active),
                    106 => LoopGridMessage::UndoButton(active),
                    107 => LoopGridMessage::RedoButton(active),
                    108 => LoopGridMessage::HoldButton(active),
                    109 => LoopGridMessage::SuppressButton(active),
                    110 => LoopGridMessage::ScaleButton(active),
                    111 => LoopGridMessage::SelectButton(active),
                    _ => LoopGridMessage::None
                };
                tx_input.send(to_send).unwrap();
            }
        }, ()).unwrap();

        let mut clock_sender = clock.sender.clone();

        // receive updates from clock
        thread::spawn(move || {
            for msg in clock.receiver {
                match msg {
                    FromClock::Schedule {pos, length} => {
                        tx_clock.send(LoopGridMessage::Schedule(pos, length)).unwrap();
                    },
                    FromClock::Tempo(value) => {

                    },
                    FromClock::Jump => {
                        tx_clock.send(LoopGridMessage::InitialLoop).unwrap();
                    }
                }
            }
        });

        thread::spawn(move || {
            let mut mapping: HashMap<Coords, MidiMap> = HashMap::new();
            let mut chunks: Vec<Box<Triggerable>> = Vec::new();
            let mut scale = scale;

            for item in chunk_map {
                let mut id = 0;
                let chunk_index = chunks.len();
                for row in (item.coords.row)..(item.coords.row + item.shape.rows) {
                    for col in (item.coords.col)..(item.coords.col + item.shape.cols) {
                        mapping.insert(Coords::new(row, col), MidiMap {chunk_index, id});
                        id += 1;
                    }
                }
                chunks.push(item.chunk);
            }


            let mut loop_length = MidiTime::from_beats(8);
            let mut loop_state = LoopState::new(loop_length, move |value| {
                loop_length = value.length;
                tx_loop_state.send(LoopGridMessage::InitialLoop).unwrap();
                tx_loop_state.send(LoopGridMessage::RefreshActive).unwrap();
            });
            let mut repeating = false;
            let mut repeat_off_beat = false;
            let (_midi_to_id, id_to_midi) = get_grid_map();

            // selection
            let mut selection_override = LoopTransform::None;
            let mut selection: HashSet<u32> = HashSet::new();
            let mut suppressing = false;
            let mut holding = false;
            let mut selecting = false;
            let mut loop_from = MidiTime::from_ticks(0);
            let mut should_flatten = false;

            let mut selecting_scale = false;

            let mut rate = MidiTime::from_beats(2);
            let mut recorder = LoopRecorder::new();
            let mut last_pos = MidiTime::from_ticks(0);
            let mut last_playback_pos = MidiTime::from_ticks(0);
            let mut override_values: HashMap<u32, LoopTransform> = HashMap::new();
            let mut input_values: HashMap<u32, OutputValue> = HashMap::new();
            let mut currently_held_inputs: Vec<u32> = Vec::new();
            let mut currently_held_rates: Vec<usize> = Vec::new();

            // nudge
            let mut nudge_next_tick: i32 = 0;

            // out state
            let mut out_values: HashMap<u32, OutputValue> = HashMap::new();
            let mut grid_out: HashMap<u32, Light> = HashMap::new();
            let mut select_out = Light::Off;
            let mut last_repeat_light_out = Light::Off;
            let mut last_scale_light_out = Light::Off;

            // display state
            let mut active: HashSet<u32> = HashSet::new();
            let mut recording: HashSet<u32> = HashSet::new();

            let mut last_beat_light = SIDE_BUTTONS[7];
            let mut last_repeat_light = SIDE_BUTTONS[7];
            let mut last_scale_light = SIDE_BUTTONS[7];

            let tick_pos_increment = MidiTime::tick();
            let half_tick_increment = MidiTime::half_tick();

            // default button lights
            launchpad_output.send(&[176, 104, Light::YellowMed as u8]).unwrap();
            launchpad_output.send(&[176, 109, Light::RedLow as u8]).unwrap();
            tx_feedback.send(LoopGridMessage::RefreshUndoRedoLights).unwrap();

            for received in rx {
                match received {
                    LoopGridMessage::Schedule(position, length) => {
                        if length > MidiTime::zero() {
                            let mut to_trigger = Vec::new();
                            let current_loop = loop_state.get();

                            // loop playback
                            let offset = current_loop.offset % current_loop.length;
                            let playback_pos = current_loop.offset + ((position - offset) % current_loop.length);
                            let playback_range = recorder.get_range(playback_pos, playback_pos + length);

                            // restart loop
                            if playback_pos == current_loop.offset {
                                tx_feedback.send(LoopGridMessage::InitialLoop).unwrap();
                            }

                            let mut transformed: HashSet<&u32> = HashSet::new();

                            for id in id_to_midi.keys() {
                                let transform = get_transform(&id, &override_values, &selection, &selection_override, &current_loop.transforms);
                                match transform {
                                    &LoopTransform::Repeat(rate, offset, output_value) => {
                                        let repeat_position = (position + offset) % rate;
                                        let half = rate.half().whole();
                                        if repeat_position.is_zero() {
                                            to_trigger.push(LoopEvent {
                                                value: output_value,
                                                pos: position,
                                                id: id.clone()
                                            });
                                        } else if repeat_position == half {
                                            to_trigger.push(LoopEvent {
                                                value: OutputValue::Off,
                                                pos: position,
                                                id: id.clone()
                                            });
                                        }
                                    },
                                    &LoopTransform::Hold(hold_position, rate) => {
                                        let offset = hold_position % rate;
                                        let playback_pos = hold_position + ((position - offset) % rate);
                                        let playback_range = recorder.get_range(playback_pos, playback_pos + length);
                                        
                                        if playback_pos == hold_position {
                                            match recorder.get_event_at(*id, playback_pos) {
                                                Some(event) if event.value == OutputValue::Off => {
                                                    to_trigger.push(event.with_pos(position))
                                                },
                                                _ => ()
                                            }
                                        }

                                        for event in playback_range {
                                            if event.id == *id {
                                                to_trigger.push(event.with_pos(position));
                                            }
                                        }
                                    },
                                    _ => ()
                                }

                                if transform != &LoopTransform::None {
                                    transformed.insert(id);
                                }
                            }

                            // trigger events for current tick if not overriden above
                            for event in playback_range {
                                if !transformed.contains(&event.id) {
                                    to_trigger.push(event.with_pos(position));
                                }
                            }

                            to_trigger.sort_unstable();

                            for event in to_trigger {
                                tx_feedback.send(LoopGridMessage::Event(event)).unwrap();
                            }

                            last_pos = position;
                            last_playback_pos = playback_pos;

                            tx_feedback.send(LoopGridMessage::RefreshSideButtons).unwrap();
                            tx_feedback.send(LoopGridMessage::RefreshRecording).unwrap();
                        }

                    },
                    LoopGridMessage::RefreshSideButtons => {
                        let current_scale = scale.lock().unwrap();
                        let beat_display_multiplier = (24.0 * 8.0) / loop_length.ticks() as f64;
                        let shifted_beat_position = (last_pos.ticks() as f64 * beat_display_multiplier / 24.0) as usize;

                        let current_beat_light = SIDE_BUTTONS[shifted_beat_position % 8];
                        let current_repeat_light = SIDE_BUTTONS[REPEAT_RATES.iter().position(|v| v == &rate).unwrap_or(0)];
                        let current_scale_light = *SIDE_BUTTONS.get(current_scale.scale as usize).unwrap_or(&SIDE_BUTTONS[0]);

                        let rate_color = if repeat_off_beat { Light::RedMed } else { Light::YellowMed };
                        let scale_color = Light::RedLow;

                        if current_repeat_light != last_repeat_light || last_repeat_light_out != rate_color {
                            launchpad_output.send(&[144, last_repeat_light, 0]).unwrap();
                            launchpad_output.send(&[144, current_repeat_light, rate_color as u8]).unwrap();
                        }

                        if current_scale_light != last_scale_light || last_scale_light_out != scale_color {
                            launchpad_output.send(&[144, last_scale_light, 0]).unwrap();
                            launchpad_output.send(&[144, current_scale_light, scale_color as u8]).unwrap();
                        }

                        let beat_start = last_pos.is_whole_beat();

                        let base_last_beat_light = if current_repeat_light == last_beat_light {
                            rate_color
                        } else if current_scale_light == last_beat_light {
                            scale_color
                        } else {
                            Light::None
                        };
                        
                        let base_beat_light = if current_repeat_light == current_beat_light {
                            rate_color
                        } else if current_scale_light == current_beat_light {
                            scale_color
                        } else {
                            Light::None
                        };

                        if current_beat_light != last_beat_light {
                            launchpad_output.send(&[144, last_beat_light, base_last_beat_light.unwrap_or(Light::Off) as u8]).unwrap();
                            if !beat_start {
                                launchpad_output.send(&[144, current_beat_light, base_beat_light.unwrap_or(Light::GreenLow) as u8]).unwrap();
                            }
                        }

                        if beat_start {
                            launchpad_output.send(&[144, current_beat_light, Light::Green as u8]).unwrap();
                        } else if last_pos.beat_tick() == 3 {
                            launchpad_output.send(&[144, current_beat_light, base_beat_light.unwrap_or(Light::GreenLow) as u8]).unwrap();
                        }

                        last_beat_light = current_beat_light;
                        last_repeat_light = current_repeat_light;
                        last_repeat_light_out = rate_color;
                        last_scale_light = current_scale_light;
                        last_scale_light_out = scale_color;
                    },
                    LoopGridMessage::RefreshUndoRedoLights => {
                        let color = if selecting_scale && selecting {
                            // nudging
                            Light::Orange
                        } else if selecting {
                            Light::GreenLow
                        } else if selecting_scale {
                            Light::YellowMed
                        } else {
                            Light::RedLow
                        };

                        launchpad_output.send(&[176, 106, color as u8]).unwrap();
                        launchpad_output.send(&[176, 107, color as u8]).unwrap();
                    },
                    LoopGridMessage::GridInput(_stamp, id, value) => {
                        let current_index = currently_held_inputs.iter().position(|v| v == &id);
                        
                        if value.is_on() && current_index == None {
                            currently_held_inputs.push(id);
                        } else if let Some(index) = current_index {
                            currently_held_inputs.remove(index);
                        }


                        if selecting && value.is_on() {
                            if selection.contains(&id) {
                                selection.remove(&id);
                            } else {
                                selection.insert(id);
                            }

                            // range selection
                            if currently_held_inputs.len() == 2 {

                                let from = Coords::from(currently_held_inputs[0]);
                                let to = Coords::from(currently_held_inputs[1]);

                                let from_row = u32::min(from.row, to.row);
                                let to_row = u32::max(from.row, to.row) + 1;
                                let from_col = u32::min(from.col, to.col);
                                let to_col = u32::max(from.col, to.col) + 1;

                                for row in from_row..to_row {
                                    for col in from_col..to_col {
                                        let id = Coords::id_from(row, col);
                                        selection.insert(id);
                                        tx_feedback.send(LoopGridMessage::RefreshGridButton(id)).unwrap();
                                    }
                                }
                            }

                            tx_feedback.send(LoopGridMessage::RefreshGridButton(id)).unwrap();
                        } else {
                            input_values.insert(id, value);
                            tx_feedback.send(LoopGridMessage::RefreshInput(id)).unwrap();
                        }
                        tx_feedback.send(LoopGridMessage::RefreshShouldFlatten).unwrap();
                    },
                    LoopGridMessage::RefreshInput(id) => {
                        let value = input_values.get(&id).unwrap_or(&OutputValue::Off);
                        let transform = match value {
                            &OutputValue::On(velocity) => {
                                if repeating {
                                    let repeat_offset = if repeat_off_beat { rate / 2 } else { MidiTime::zero() };
                                    LoopTransform::Repeat(rate, repeat_offset, OutputValue::On(velocity))
                                } else {
                                    LoopTransform::Value(OutputValue::On(velocity))
                                }
                            },
                            &OutputValue::Off => LoopTransform::None
                        };

                        let changed = match override_values.entry(id) {
                            Occupied(mut entry) => {
                                let different = entry.get() != &transform;
                                entry.insert(transform);
                                different
                            },
                            Vacant(entry) => {
                                let different = transform != LoopTransform::None;
                                entry.insert(transform);
                                different
                            }
                        };

                        if changed {
                            tx_feedback.send(LoopGridMessage::RefreshOverride(id)).unwrap();
                        }
                    },
                    LoopGridMessage::RefreshOverride(id) => {
                        let current_loop = loop_state.get();
                        let transform = get_transform(&id, &override_values, &selection, &selection_override, &current_loop.transforms);
                        let fallback = match recorder.get_event_at(id, last_playback_pos) {
                            Some(event) if event.value != OutputValue::Off => {
                                match recorder.get_next_event_at(id, last_playback_pos) {
                                    Some(next_event) => {
                                        if (next_event.pos - last_playback_pos) > MidiTime::from_beats(1) {
                                            Some(event.value.clone())
                                        } else {
                                            None
                                        }
                                    },
                                    _ => {
                                        Some(event.value)
                                    }
                                }
                            },
                            _ => Some(OutputValue::Off)
                        };

                        let value = match transform {
                            &LoopTransform::Value(value) => Some(value),
                            &LoopTransform::None => fallback,
                            &LoopTransform::Repeat(..) | &LoopTransform::Hold(..) => None
                        };
                        
                        if let Some(v) = value {
                            tx_feedback.send(LoopGridMessage::Event(LoopEvent {
                                value: v,
                                pos: last_pos,
                                id
                            })).unwrap();
                        }
                    },  
                    LoopGridMessage::RefreshGridButton(id) => {
                        let out_value = out_values.get(&id).unwrap_or(&OutputValue::Off);
                        let old_value = grid_out.remove(&id).unwrap_or(Light::Off);
                        let new_value = if out_value != &OutputValue::Off {
                            Light::Yellow
                        } else if selection.contains(&id) {
                            Light::Green
                        } else if active.contains(&id) {
                            Light::GreenLow
                        } else if recording.contains(&id) {
                            Light::RedLow
                        } else {
                            Light::Off
                        };

                        if new_value != old_value {
                            let midi_id = id_to_midi.get(&id);
                            launchpad_output.send(&[144, *midi_id.unwrap(), new_value as u8]).unwrap();
                        }

                        grid_out.insert(id, new_value);
                    },
                    LoopGridMessage::RefreshSelectionOverride => {
                        selection_override = if suppressing {
                            LoopTransform::Value(OutputValue::Off)
                        } else if holding {
                            LoopTransform::Hold(last_playback_pos, rate)
                        } else {
                            LoopTransform::None
                        };

                        for id in id_to_midi.keys() {
                            tx_feedback.send(LoopGridMessage::RefreshOverride(*id)).unwrap();
                        }
                    },
                    LoopGridMessage::RefreshActive => {
                        let current_loop = loop_state.get();
                        let mut ids = HashSet::new();
                        for id in recorder.get_ids_in_range(current_loop.offset, current_loop.offset + current_loop.length) {
                            if current_loop.transforms.get(&id).unwrap_or(&LoopTransform::None) != &LoopTransform::Value(OutputValue::Off) {
                                ids.insert(id);
                            }
                        }                        
                        let (added, removed) = update_ids(&ids, &mut active);
 
                        for id in added {
                            tx_feedback.send(LoopGridMessage::RefreshGridButton(id)).unwrap();
                        }

                        for id in removed {
                            tx_feedback.send(LoopGridMessage::RefreshGridButton(id)).unwrap();
                        }
                    },
                    LoopGridMessage::RefreshRecording => {
                        let current_loop = loop_state.get();
                        let ids = recorder.get_ids_in_range(last_pos - loop_length, last_pos);

                        let (added, removed) = update_ids(&ids, &mut recording);

                        for id in added {
                            tx_feedback.send(LoopGridMessage::RefreshGridButton(id)).unwrap();
                        }

                        for id in removed {
                            tx_feedback.send(LoopGridMessage::RefreshGridButton(id)).unwrap();
                        }
                    },
                    LoopGridMessage::RefreshSelectState => {
                        let new_state = if selecting {
                            Light::Green
                        } else if selection.len() > 0 {
                            Light::GreenLow
                        } else {
                            Light::Off
                        };

                        if select_out != new_state {
                            launchpad_output.send(&[176, 111, new_state as u8]).unwrap();
                            select_out = new_state;
                        }
                    },
                    LoopGridMessage::Event(event) => {
                        let new_value = event.value.clone();
                        let midi_id = id_to_midi.get(&event.id);
                        if midi_id.is_some() {
                            match maybe_update(&mut out_values, event.id, new_value) {
                                Some(value) => {
                                    tx_feedback.send(LoopGridMessage::RefreshGridButton(event.id)).unwrap();
                                    // TODO: actual playback here
                                    if let Some(mapped) = mapping.get(&Coords::from(event.id)) {
                                        tx_feedback.send(LoopGridMessage::TriggerChunk(*mapped, new_value)).unwrap();
                                    }
                                },
                                None => ()
                            };
                        }

                        recorder.add(event);
                    },
                    LoopGridMessage::LoopButton(pressed) => {
                        if selecting_scale && selecting {
                            if pressed {
                                clock_sender.send(ToClock::TapTempo).unwrap();
                            }
                        } else {
                            if pressed {
                                loop_from = last_pos;
                                tx_feedback.send(LoopGridMessage::ClearSelection).unwrap();
                            } else {
                                let since_press = last_pos - loop_from;
                                let threshold = MidiTime::from_ticks(12);
                                if since_press > threshold {
                                    let quantized_length = MidiTime::quantize_length(last_pos - loop_from);
                                    loop_length = quantized_length;
                                    loop_state.set(Loop::new(last_pos - quantized_length, quantized_length));
                                } else {
                                    loop_state.set(Loop::new(loop_from - loop_length, loop_length));
                                }
                            }
                        }
                    },
                    LoopGridMessage::ClearSelection => {
                        for id in &selection {
                            tx_feedback.send(LoopGridMessage::RefreshGridButton(*id)).unwrap();
                        }
                        selection.clear();
                        tx_feedback.send(LoopGridMessage::RefreshSelectState).unwrap();
                    }
                    LoopGridMessage::RefreshShouldFlatten => {
                        let new_value = &selection_override != &LoopTransform::None || override_values.values().any(|value| value != &LoopTransform::None);
                        if new_value != should_flatten {
                            should_flatten = new_value;
                            let color = if should_flatten {
                                Light::GreenLow
                            } else {
                                Light::Off
                            };
                            launchpad_output.send(&[176, 105, color as u8]).unwrap();
                        }
                    },
                    LoopGridMessage::FlattenButton(pressed) => {
                        if pressed {
                            if selection.len() > 0 {
                                let mut new_loop = loop_state.get().clone();

                                for id in &selection {
                                    new_loop.transforms.insert(id.clone(), LoopTransform::Value(OutputValue::Off));
                                }

                                loop_state.set(new_loop);
                            } else if should_flatten {
                                let mut new_loop = loop_state.get().clone();

                                if &selection_override != &LoopTransform::None {
                                    for id in id_to_midi.keys() {
                                        new_loop.transforms.insert(id.clone(), selection_override.clone());
                                    }
                                }

                                // repeaters
                                for (id, transform) in &override_values {
                                    if transform != &LoopTransform::None {
                                        new_loop.transforms.insert(id.clone(), transform.clone());
                                    }
                                }

                                loop_state.set(new_loop);
                            } else {
                                let mut new_loop = loop_state.get().clone();

                                for id in id_to_midi.keys() {
                                    new_loop.transforms.insert(id.clone(), LoopTransform::Value(OutputValue::Off));
                                }

                                loop_state.set(new_loop);
                            }
                            tx_feedback.send(LoopGridMessage::ClearSelection).unwrap();
                        }
                    },
                    LoopGridMessage::UndoButton(pressed) => {
                        if pressed {
                            if selecting && selecting_scale {
                                clock_sender.send(ToClock::Nudge(MidiTime::from_ticks(-1)));
                            } else if selecting {
                                loop_length = (loop_length / 2).max(MidiTime::from_measure(1, 4));
                            } else if selecting_scale {
                                let mut current_scale = scale.lock().unwrap();
                                current_scale.root -= 1;
                            } else {
                                loop_state.undo();
                            }
                        }
                    },   
                    LoopGridMessage::RedoButton(pressed) => {
                        if pressed {
                            if selecting && selecting_scale {
                                clock_sender.send(ToClock::Nudge(MidiTime::from_ticks(1)));
                            } else if selecting {
                                loop_length = (loop_length * 2).min(MidiTime::from_beats(32));
                            } else if selecting_scale {
                                let mut current_scale = scale.lock().unwrap();
                                current_scale.root += 1;
                            } else {
                                loop_state.redo();
                            }
                        }
                    },  
                    LoopGridMessage::HoldButton(pressed) => {
                        holding = pressed;
                        tx_feedback.send(LoopGridMessage::RefreshSelectionOverride).unwrap();
                        tx_feedback.send(LoopGridMessage::RefreshShouldFlatten).unwrap();
                    },
                    LoopGridMessage::SuppressButton(pressed) => {
                        suppressing = pressed;
                        tx_feedback.send(LoopGridMessage::RefreshSelectionOverride).unwrap();
                        tx_feedback.send(LoopGridMessage::RefreshShouldFlatten).unwrap();
                    },
                    LoopGridMessage::SelectButton(pressed) => {
                        selecting = pressed;
                        if pressed {
                            tx_feedback.send(LoopGridMessage::ClearSelection).unwrap()
                        }
                        tx_feedback.send(LoopGridMessage::RefreshSelectState).unwrap();
                        tx_feedback.send(LoopGridMessage::RefreshUndoRedoLights).unwrap();
                    },
                    LoopGridMessage::ScaleButton(pressed) => {
                        let new_state = if pressed {
                            Light::Yellow
                        } else {
                            Light::Off
                        };
                        launchpad_output.send(&[176, 110, new_state as u8]).unwrap();

                        selecting_scale = pressed;
                        tx_feedback.send(LoopGridMessage::RefreshUndoRedoLights).unwrap();
                    },
                    LoopGridMessage::RateButton(button_id, pressed) => {
                        let current_index = currently_held_rates.iter().position(|v| v == &button_id);

                        if pressed && current_index == None {
                            currently_held_rates.push(button_id);
                        } else if let Some(index) = current_index {
                            currently_held_rates.remove(index);
                        }

                        if currently_held_rates.len() > 0 {
                            let id = *currently_held_rates.iter().last().unwrap();
                            if selecting_scale {
                                let mut current_scale = scale.lock().unwrap();
                                current_scale.scale = id as i32;
                            } else {
                                let rate = REPEAT_RATES[id];
                                tx_feedback.send(LoopGridMessage::SetRate(rate)).unwrap();
                                repeat_off_beat = selecting;
                                repeating = id > 0 || repeat_off_beat;
                            }
                        }
                    },
                    LoopGridMessage::SetRate(value) => {
                        rate = value;
                        tx_feedback.send(LoopGridMessage::RefreshSelectionOverride).unwrap();
                        tx_feedback.send(LoopGridMessage::RefreshSideButtons).unwrap();
                        for (id, value) in &override_values {
                            if value != &LoopTransform::None {
                                tx_feedback.send(LoopGridMessage::RefreshInput(*id)).unwrap();
                            }
                        }
                    },                 
                    LoopGridMessage::InitialLoop => {
                        for id in id_to_midi.keys() {
                            tx_feedback.send(LoopGridMessage::RefreshOverride(*id)).unwrap();
                        }
                    },
                    LoopGridMessage::TriggerChunk(map, value) => {
                        if let Some(chunk) = chunks.get_mut(map.chunk_index) {
                            chunk.trigger(map.id, value, SystemTime::now())
                        }
                    },
                    LoopGridMessage::None => ()
                }
            }
        });

        LoopGridLaunchpad {
            port_name: String::from(launchpad_port_name),
            tx,
            input
        }
    }

    pub fn get_channel(&self) -> mpsc::Sender<LoopGridMessage> {
        mpsc::Sender::clone(&self.tx)
    }
}

fn maybe_update (hash_map: &mut HashMap<u32, OutputValue>, key: u32, new_value: OutputValue) -> Option<OutputValue> {
    let entry = hash_map.entry(key);
    match entry {
        Entry::Occupied(mut entry) => {
            let old_value = entry.insert(new_value);

            if old_value != new_value {
                Some(new_value)
            } else {
                None
            }
        },
        Entry::Vacant(entry) => {
            entry.insert(new_value);
            Some(new_value)
        }
    }
}

fn get_grid_map () -> (HashMap<u8, u32>, HashMap<u32, u8>) {
    let mut midi_to_id: HashMap<u8, u32> = HashMap::new();
    let mut id_to_midi: HashMap<u32, u8> = HashMap::new();

    for r in 0..8 {
        for c in 0..8 {
            let midi = (r * 16 + c) as u8;
            let id = (r * 8 + c) as u32;
            midi_to_id.insert(midi, id);
            id_to_midi.insert(id, midi);
        }
    } 

    (midi_to_id, id_to_midi)
}

fn get_transform<'a> (id: &u32, override_values: &'a HashMap<u32, LoopTransform>, selection: &HashSet<u32>, selection_override: &'a LoopTransform, loop_transforms: &'a HashMap<u32, LoopTransform>) -> &'a LoopTransform {
    let in_selection = selection.len() == 0 || selection.contains(&id);
    let id_override = override_values.get(&id).unwrap_or(&LoopTransform::None);

    if id_override != &LoopTransform::None {
        id_override
    } else if (selection_override != &LoopTransform::None) && in_selection {
        selection_override
    } else {
        loop_transforms.get(id).unwrap_or(&LoopTransform::None)
    }
}

fn update_ids <'a> (a: &'a HashSet<u32>, b: &'a mut HashSet<u32>) -> (Vec<u32>, Vec<u32>) {
    let mut added = Vec::new();
    let mut removed = Vec::new();

    for id in a {
        if !b.contains(id) {
            added.push(*id)
        }
    }

    for id in b.iter() {
        if !a.contains(id) {
            removed.push(*id)
        }
    }

    for id in &added {
        b.insert(*id);
    }

    for id in &removed {
        b.remove(id);
    }

    (added, removed)
}