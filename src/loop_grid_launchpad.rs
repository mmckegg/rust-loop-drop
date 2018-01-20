extern crate midir;
use self::midir::{MidiInputConnection};
use std::sync::mpsc;
use std::thread;
use std::collections::HashSet;
use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::collections::hash_map::Entry::{Occupied, Vacant};
use ::midi_connection;
use ::midi_time::MidiTime;

use ::loop_recorder::{LoopRecorder, OutputValue, LoopEvent};
use ::loop_state::{Loop, LoopState, LoopTransform};

const SIDE_BUTTONS: [u8; 8] = [8, 24, 40, 56, 72, 88, 104, 120];

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

pub enum LoopGridMessage {
    Schedule(MidiTime),
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
    SetRepeating(bool),
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
    pub fn new(launchpad_port_name: &str, output_port_name: &str) -> Self {
        let (tx, rx) = mpsc::channel();
        
        let tx_input =  mpsc::Sender::clone(&tx);
        let tx_feedback =  mpsc::Sender::clone(&tx);
        let tx_loop_state =  mpsc::Sender::clone(&tx);

        let (midi_to_id, id_to_midi) = get_grid_map();

        let mapping = get_mapping();

        let mut midi_output = midi_connection::get_output(&output_port_name).unwrap();
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
                        OutputValue::On
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

        thread::spawn(move || {
            let mut loop_length = MidiTime::from_beats(8);
            let mut loop_state = LoopState::new(loop_length, move |value| {
                loop_length = value.length;
                tx_loop_state.send(LoopGridMessage::InitialLoop).unwrap();
                tx_loop_state.send(LoopGridMessage::RefreshActive).unwrap();
            });
            let mut repeating = false;
            let mut repeat_off_beat = false;
            let (midi_to_id, id_to_midi) = get_grid_map();

            // selecton
            let mut selection_override = LoopTransform::None;
            let mut selection: HashSet<u32> = HashSet::new();
            let mut suppressing = false;
            let mut holding = false;
            let mut selecting = false;
            let mut loop_from = MidiTime::from_ticks(0);
            let mut should_flatten = false;

            let mut selecting_scale = false;
            let mut current_scale: i32 = 0;
            let mut current_scale_root: i32 = 69;

            let mut rate = MidiTime::from_beats(2);
            let mut recorder = LoopRecorder::new();
            let mut last_pos = MidiTime::from_ticks(0);
            let mut last_playback_pos = MidiTime::from_ticks(0);
            let mut override_values: HashMap<u32, LoopTransform> = HashMap::new();
            let mut input_values: HashMap<u32, OutputValue> = HashMap::new();

            // nudge
            let mut nudge_next_tick: i32 = 0;
            let mut nudge_offset: i32 = 0;

            // out state
            let mut out_values: HashMap<u32, OutputValue> = HashMap::new();
            let mut grid_out: HashMap<u32, Light> = HashMap::new();
            let mut select_out = Light::Off;
            let mut last_repeat_light_out = Light::Off;
            let mut last_scale_light_out = Light::Off;

            // midi state
            let mut volca_keys_offset: HashMap<u32, i32> = HashMap::new();
            let mut volca_bass_offset: HashMap<u32, i32> = HashMap::new();
            let mut sp404_a_offset: HashMap<u32, i32> = HashMap::new();
            let mut sp404_b_offset: HashMap<u32, i32> = HashMap::new();
            let mut volca_keys_out: HashMap<u32, u8> = HashMap::new();
            let mut volca_bass_out: HashMap<u32, u8> = HashMap::new();
            let mut sp404_a_out: HashMap<u32, u8> = HashMap::new();
            let mut sp404_b_out: HashMap<u32, u8> = HashMap::new();
            let mut tr08_out: HashMap<u32, u8> = HashMap::new();

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
            launchpad_output.send(&[176, 1010, Light::RedLow as u8]).unwrap();
            tx_feedback.send(LoopGridMessage::RefreshUndoRedoLights).unwrap();

            for received in rx {
                match received {
                    LoopGridMessage::Schedule(position) => {
                        // rebroadcast tick
                        midi_output.send(&[248]);

                        // nudge sync
                        let length = if nudge_next_tick > 0 {
                            nudge_next_tick -= 1;
                            nudge_offset += 1;
                            MidiTime::tick() * 2
                        } else if nudge_next_tick < 0 {
                            nudge_next_tick += 1;
                            nudge_offset -= 1;
                            MidiTime::zero()
                        } else {
                            MidiTime::tick()
                        };

                        if length > MidiTime::zero() {
                            let position = position + MidiTime::from_ticks(nudge_offset);

                            let current_loop = loop_state.get();

                            // loop playback
                            let offset = current_loop.offset % current_loop.length;
                            let playback_pos = current_loop.offset + ((position - offset) % current_loop.length);
                            let playback_range = recorder.get_range(playback_pos, playback_pos + length);

                            // restart loop
                            if playback_pos == current_loop.offset {
                                //tx_feedback.send(LoopGridMessage::InitialLoop).unwrap();
                            }

                            let mut transformed: HashSet<&u32> = HashSet::new();
                            let mut playback_cache: HashMap<PlaybackRange, &[LoopEvent]> = HashMap::new();

                            for id in id_to_midi.keys() {
                                let transform = get_transform(&id, &override_values, &selection, &selection_override, &current_loop.transforms);
                                match transform {
                                    &LoopTransform::Repeat(rate, offset) => {
                                        let repeat_position = (position + offset) % rate;
                                        let half = rate.half().whole();
                                        if repeat_position.is_zero() {
                                            tx_feedback.send(LoopGridMessage::Event(LoopEvent {
                                                value: OutputValue::On,
                                                pos: position,
                                                id: id.clone()
                                            })).unwrap();
                                        } else if repeat_position == half {
                                            tx_feedback.send(LoopGridMessage::Event(LoopEvent {
                                                value: OutputValue::Off,
                                                pos: position,
                                                id: id.clone()
                                            })).unwrap();
                                        }
                                    },
                                    &LoopTransform::Hold(hold_position, rate) => {
                                        let offset = hold_position % rate;
                                        let from = hold_position + ((position - offset) % rate);
                                        let to = from + length;
                                        let playback_range = PlaybackRange::new(from, to);
                                        let events = match playback_cache.entry(playback_range) {
                                            Occupied(mut entry) => entry.into_mut(),
                                            Vacant(entry) => entry.insert(recorder.get_range(from, to))
                                        };
                                        
                                        let pos = (playback_pos % rate);

                                        if pos.is_zero() {
                                            match recorder.get_event_at(*id, last_playback_pos) {
                                                Some(event) if event.value == OutputValue::Off => {
                                                    tx_feedback.send(LoopGridMessage::Event(event.with_pos(position))).unwrap();
                                                },
                                                _ => ()
                                            }
                                        }

                                        for event in events.iter() {
                                            if event.id == *id {
                                                tx_feedback.send(LoopGridMessage::Event(event.with_pos(position))).unwrap();
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
                                    tx_feedback.send(LoopGridMessage::Event(event.with_pos(position))).unwrap();
                                }
                            }

                            last_pos = position;
                            last_playback_pos = playback_pos;

                            tx_feedback.send(LoopGridMessage::RefreshSideButtons).unwrap();
                            tx_feedback.send(LoopGridMessage::RefreshRecording).unwrap();
                        }

                    },
                    LoopGridMessage::RefreshSideButtons => {
                        let beat_display_multiplier = (24.0 * 8.0) / loop_length.ticks() as f64;
                        let shifted_beat_position = (last_pos.ticks() as f64 * beat_display_multiplier / 24.0) as usize;

                        let current_beat_light = SIDE_BUTTONS[shifted_beat_position % 8];
                        let current_repeat_light = SIDE_BUTTONS[REPEAT_RATES.iter().position(|v| v == &rate).unwrap_or(0)];
                        let current_scale_light = *SIDE_BUTTONS.get(current_scale as usize).unwrap_or(&SIDE_BUTTONS[0]);

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
                        if selecting && value == OutputValue::On {
                            if selection.contains(&id) {
                                selection.remove(&id);
                            } else {
                                selection.insert(id);
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
                            &OutputValue::On => {
                                let allows_repeat = match mapping.get(&Coords::from(id)) {
                                    Some(map) if map.group == Group::VolcaKeysOffset => false,
                                    Some(map) if map.group == Group::SP404AOffset => false,
                                    Some(map) if map.group == Group::SP404BOffset => false,
                                    Some(map) if map.group == Group::VolcaBassOffset => false,
                                    _ => true
                                };
                                if repeating && allows_repeat {
                                    let repeat_offset = if repeat_off_beat { rate / 2 } else { MidiTime::zero() };
                                    LoopTransform::Repeat(rate, repeat_offset)
                                } else {
                                    LoopTransform::On
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
                                match recorder.get_next_event_at(id, last_playback_pos)  {
                                    Some(next_event) if (next_event.pos - last_playback_pos) > MidiTime::from_measure(1, 2) => Some(event.value.clone()),
                                    _ => Some(OutputValue::Off)
                                }
                            },
                            _ => Some(OutputValue::Off)
                        };

                        let value = match transform {
                            &LoopTransform::On => Some(OutputValue::On),
                            &LoopTransform::None => fallback,
                            &LoopTransform::Repeat(_, _) | &LoopTransform::Hold(_, _) => None,
                            &LoopTransform::Suppress => Some(OutputValue::Off)
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
                            LoopTransform::Suppress
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
                            if current_loop.transforms.get(&id).unwrap_or(&LoopTransform::None) != &LoopTransform::Suppress {
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
                                    recorder.add(event);
                                },
                                None => ()
                            };
                        }
                    },
                    LoopGridMessage::LoopButton(pressed) => {
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

                                // use current transform, or suppress if no transform active
                                let transform = match &selection_override {
                                    &LoopTransform::None => LoopTransform::Suppress,
                                    _ => selection_override.clone()
                                };
                                for id in &selection {
                                    new_loop.transforms.insert(id.clone(), LoopTransform::Suppress);
                                }

                                loop_state.set(new_loop);
                            } else if should_flatten {
                                let mut new_loop = loop_state.get().clone();

                                // repeaters
                                for (id, transform) in &override_values {
                                    if transform != &LoopTransform::None {
                                        new_loop.transforms.insert(id.clone(), transform.clone());
                                    }
                                }

                                loop_state.set(new_loop);
                            } else {
                                loop_state.set(Loop::new(MidiTime::zero() - loop_length, loop_length));
                            }
                            tx_feedback.send(LoopGridMessage::ClearSelection).unwrap();
                        }
                    },
                    LoopGridMessage::UndoButton(pressed) => {
                        if pressed {
                            if selecting && selecting_scale {
                                nudge_next_tick -= 1;
                            } else if selecting {
                                loop_length = (loop_length / 2).max(MidiTime::from_measure(1, 4));
                            } else if selecting_scale {
                                current_scale_root -= 1;
                            } else {
                                loop_state.undo();
                            }
                        }
                    },   
                    LoopGridMessage::RedoButton(pressed) => {
                        if pressed {
                            if selecting && selecting_scale {
                                nudge_next_tick += 1;
                            } else if selecting {
                                loop_length = (loop_length * 2).min(MidiTime::from_beats(32));
                            } else if selecting_scale {
                                current_scale_root += 1;
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
                        selecting_scale = pressed;
                        tx_feedback.send(LoopGridMessage::RefreshUndoRedoLights).unwrap();
                    },
                    LoopGridMessage::SetRepeating(value) => {
                        repeat_off_beat = selecting;
                        repeating = value;
                    },
                    LoopGridMessage::RateButton(index, pressed) => {
                        if pressed {
                            if selecting_scale {
                                current_scale = (index as i32);
                            } else {
                                let rate = REPEAT_RATES[index];
                                tx_feedback.send(LoopGridMessage::SetRate(rate)).unwrap();
                                repeat_off_beat = selecting;
                                repeating = index > 0;
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
                        let midi_value: u8 = match value {
                            OutputValue::Off => 0,
                            OutputValue::On => 100
                        };
                        match map.group {
                            Group::VolcaKeys => {
                                match value {
                                    OutputValue::Off => {
                                        if volca_keys_out.contains_key(&map.id) {
                                            let note_id = *volca_keys_out.get(&map.id).unwrap();
                                            midi_output.send(&[144 + volca_keys_channel - 1, note_id, 0]).unwrap();
                                            volca_keys_out.remove(&map.id);
                                        }
                                    },
                                    OutputValue::On => {
                                        let offset_value: i32 = volca_keys_offset.values().sum();
                                        let octave = -2;
                                        let note_id = get_scaled(current_scale_root + (octave * 12), current_scale, (map.id as i32) + offset_value) as u8;
                                        midi_output.send(&[144 + volca_keys_channel - 1, note_id, midi_value]).unwrap();
                                        volca_keys_out.insert(map.id, note_id);
                                    }
                                }
                            },
                            Group::VolcaKeysOffset => {
                                let offsets = [-4, -3, -2, -1, 1, 2, 3, 4];
                                let offset_value = match value { 
                                    OutputValue::On => offsets[map.id as usize],
                                    OutputValue::Off => 0 
                                };
                                volca_keys_offset.insert(map.id, offset_value);
                            },
                            Group::VolcaBass => {
                                match value {
                                    OutputValue::Off => {
                                        if volca_bass_out.contains_key(&map.id) {
                                            let note_id = *volca_bass_out.get(&map.id).unwrap();
                                            midi_output.send(&[144 + volca_bass_channel - 1, note_id, 0]).unwrap();
                                            volca_bass_out.remove(&map.id);
                                        }
                                    },
                                    OutputValue::On => {
                                        let offset_value: i32 = volca_bass_offset.values().sum();
                                        let octave = -3;
                                        let note_id = get_scaled(current_scale_root + (octave * 12), current_scale, (map.id as i32) + offset_value) as u8;
                                        midi_output.send(&[144 + volca_bass_channel - 1, note_id, midi_value]).unwrap();
                                        volca_bass_out.insert(map.id, note_id);
                                    }
                                }
                            },
                            Group::VolcaBassOffset => {
                                let offsets = [-4, -3, -2, -1, 1, 2, 3, 4];
                                let offset_value = match value { 
                                    OutputValue::On => offsets[map.id as usize],
                                    OutputValue::Off => 0 
                                };
                                volca_bass_offset.insert(map.id, offset_value);
                            },
                            Group::TR08 => {
                                match value {
                                    OutputValue::Off => {
                                        if sp404_a_out.contains_key(&map.id) {
                                            let note_id = *tr08_out.get(&map.id).unwrap();
                                            midi_output.send(&[128 + 10 - 1, note_id, 0]).unwrap();
                                            tr08_out.remove(&map.id);
                                        }
                                    },
                                    OutputValue::On => {
                                        let tr08_map: [u8; 16] = [
                                          36, 38, 42, 46,
                                          43, 39, 70, 49,
                                          47, 37, 75, 56,
                                          50, 64, 63, 62
                                        ];
                                        let note_id = tr08_map[map.id as usize];
                                        midi_output.send(&[144 + 10 - 1, note_id, midi_value]).unwrap();
                                        tr08_out.insert(map.id, note_id);
                                    }
                                }
                            },
                            Group::VolcaSample => {
                                match value {
                                    OutputValue::Off => (),
                                    OutputValue::On => {
                                        let channel_map: [u8; 16] = [
                                          0, 1, 8, 9, 6, 6, 6, 6,
                                          2, 3, 4, 5, 7, 7, 7, 7
                                        ];

                                        let channel = channel_map[map.id as usize];

                                        if (map.id >= 4 && map.id < 8) || map.id >= 12 {
                                            let pos = map.id % 4;
                                            let offset: i32 = match pos {
                                                1 => -14,
                                                2 => 14,
                                                3 => 18,
                                                _ => 0
                                            };
                                            midi_output.send(&[176 + channel, 43, (64 + offset) as u8]).unwrap();
                                        } 

                                        midi_output.send(&[144 + channel, 0, 127]).unwrap();
                                    }
                                }
                            },
                            Group::SP404A => {
                                match value {
                                    OutputValue::Off => {
                                        if sp404_a_out.contains_key(&map.id) {
                                            let note_id = *sp404_a_out.get(&map.id).unwrap();
                                            midi_output.send(&[144 - 1 + sp404_channel, note_id, 0]).unwrap();
                                            sp404_a_out.remove(&map.id);
                                        }
                                    },
                                    OutputValue::On => {
                                        let offset_value: i32 = *sp404_a_offset.values().max().unwrap_or(&0);
                                        let note_id = (47 + offset_value + map.id as i32) as u8;
                                        midi_output.send(&[144 - 1 + sp404_channel, note_id, midi_value]).unwrap();
                                        sp404_a_out.insert(map.id, note_id);
                                    }
                                }
                            },
                            Group::SP404B => {
                                match value {
                                    OutputValue::Off => {
                                        // ignore off events, choke instead!
                                    },
                                    OutputValue::On => {

                                        // choke
                                        for id in sp404_b_out.values() {
                                            let message = [144 + sp404_channel, *id, 0];
                                            midi_output.send(&message).unwrap();
                                        }
                                        sp404_b_out.clear();

                                        let offset_value: i32 = *sp404_b_offset.values().max().unwrap_or(&0);
                                        let note_id = (47 + offset_value + map.id as i32) as u8;
                                        let message = [144 + sp404_channel, note_id, midi_value];

                                        midi_output.send(&message).unwrap();

                                        sp404_b_out.insert(map.id, note_id);
                                    }
                                }
                            },
                            Group::SP404AOffset => {
                                let offsets = [12, 24, 36, 48];
                                let offset_value = match value { 
                                    OutputValue::On => offsets[map.id as usize],
                                    OutputValue::Off => 0 
                                };
                                sp404_a_offset.insert(map.id, offset_value);
                            },
                            Group::SP404BOffset => {
                                let offsets = [12, 24, 36, 48];
                                let offset_value = match value { 
                                    OutputValue::On => offsets[map.id as usize],
                                    OutputValue::Off => 0 
                                };
                                sp404_b_offset.insert(map.id, offset_value);
                            },
                            _ => ()
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

#[derive(Debug, Eq, PartialEq, Copy, Clone, Hash)]
enum Group {
    VolcaKeys,
    VolcaKeysOffset,
    VolcaSample,
    VolcaBass,
    VolcaBassOffset,
    TR08,
    SP404A,
    SP404AOffset,
    SP404B,
    SP404BOffset
}

#[derive(Debug, Eq, PartialEq, Copy, Clone, Hash)]
struct Coords {
    row: u32,
    col: u32
}

impl Coords {
    pub fn new (row: u32, col: u32) -> Coords {
        Coords { row, col }
    }

    pub fn from (id: u32) -> Coords {
        Coords {
            row: id / 8, 
            col: id % 8
        }
    }
}

struct Shape {
    rows: u32,
    cols: u32
}

impl Shape {
    pub fn new (rows: u32, cols: u32) -> Shape {
        Shape { rows, cols }
    }
}

#[derive(Debug, Eq, PartialEq, Copy, Clone, Hash)]
struct MidiMap {
    group: Group,
    id: u32
}

fn get_mapping () -> HashMap<Coords, MidiMap> {
    let mut result = HashMap::new();

    put_map(&mut result, 
        Group::VolcaSample, 
        Coords::new(0, 0), 
        Shape::new(2, 8)
    );

    // put_map(&mut result, 
    //     Group::TR08, 
    //     Coords::new(0, 0), 
    //     Shape::new(4, 4)
    // );

    // put_map(&mut result, 
    //     Group::SP404A, 
    //     Coords::new(0, 0), 
    //     Shape::new(3, 4)
    // );

    // put_map(&mut result, 
    //     Group::SP404B, 
    //     Coords::new(0, 4), 
    //     Shape::new(3, 4)
    // );

    // put_map(&mut result, 
    //     Group::SP404AOffset, 
    //     Coords::new(3, 0), 
    //     Shape::new(1, 4)
    // );
    
    // put_map(&mut result, 
    //     Group::SP404BOffset, 
    //     Coords::new(3, 4), 
    //     Shape::new(1, 4)
    // );

    put_map(&mut result, 
        Group::VolcaBass, 
        Coords::new(2, 0), 
        Shape::new(1, 8)
    );

    put_map(&mut result, 
        Group::VolcaBassOffset, 
        Coords::new(3, 0), 
        Shape::new(1, 8)
    );

    put_map(&mut result, 
        Group::VolcaKeys, 
        Coords::new(4, 0), 
        Shape::new(3, 8)
    );

    put_map(&mut result, 
        Group::VolcaKeysOffset, 
        Coords::new(7, 0), 
        Shape::new(1, 8)
    );

    result
}

fn put_map (map: &mut HashMap<Coords, MidiMap>, group: Group, pos: Coords, size: Shape) {
    let mut id = 0;
    for row in (pos.row)..(pos.row + size.rows) {
        for col in (pos.col)..(pos.col + size.cols) {
            map.insert(Coords::new(row, col), MidiMap {group, id});
            id += 1;
        }
    }
}

fn get_scaled (root: i32, scale: i32, value: i32) -> i32 {
    let intervals = [2, 2, 1, 2, 2, 1];
    let mut scale_notes = vec![0];
    let mut last_value = 0;
    for i in 0..6 {
        last_value += intervals[modulo(i + scale, 6) as usize];
        scale_notes.push(last_value);
    }
    let length = scale_notes.len() as i32;
    let interval = scale_notes[modulo(value, length) as usize];
    let octave = (value as f64 / length as f64).floor() as i32;
    root + (octave * 12) + interval
}

fn modulo (n: i32, m: i32) -> i32 {
    ((n % m) + m) % m
}