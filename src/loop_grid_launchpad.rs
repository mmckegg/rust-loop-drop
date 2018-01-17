extern crate midir;
use self::midir::{MidiInputConnection};
use std::sync::mpsc;
use std::thread;
use std::collections::HashSet;
use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::collections::hash_map::Entry::{Occupied, Vacant};
use ::midi_connection;

use ::loop_recorder::{LoopRecorder, OutputValue, LoopEvent};
use ::loop_state::{Loop, LoopState, LoopTransform};

const SIDE_BUTTONS: [u8; 8] = [8, 24, 40, 56, 72, 88, 104, 120];
const REPEAT_RATES: [f64; 8] = [2.0, 1.0, 2.0 / 3.0, 1.0 / 2.0, 1.0 / 3.0, 1.0 / 4.0, 1.0 / 6.0, 1.0 / 8.0];

pub enum LoopGridMessage {
    Schedule(u64),
    GridInput(u64, u32, OutputValue),
    LoopButton(bool),
    FlattenButton(bool),
    UndoButton(bool),
    RedoButton(bool),
    SuppressButton(bool),
    SelectButton(bool),
    HoldButton(bool),
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
    SetRepeating(bool),
    SetRate(f64),
    None
}

#[derive(Debug, Eq, PartialEq, Copy, Clone, Hash)]
pub struct PlaybackRange {
    from_tick: i32,
    to_tick: i32
}

impl PlaybackRange {
    pub fn new (from: f64, to: f64) -> PlaybackRange {
        PlaybackRange {
            from_tick: (from * 24.0) as i32, 
            to_tick: (to * 24.0) as i32
        }
    }

    pub fn from (&self) -> f64 {
        (self.from_tick as f64) / 24.0
    }

    pub fn to (&self) -> f64 {
        (self.to_tick as f64) / 24.0
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
    pub fn new(port_name: &str) -> Self {
        let (tx, rx) = mpsc::channel();
        
        let tx_input =  mpsc::Sender::clone(&tx);
        let tx_feedback =  mpsc::Sender::clone(&tx);
        let tx_loop_state =  mpsc::Sender::clone(&tx);

        let (midi_to_id, id_to_midi) = get_grid_map();

        let mut output = midi_connection::get_output(&port_name).unwrap();
        let input = midi_connection::get_input(&port_name, move |stamp, message, _| {
            if message[0] == 144 || message[0] == 128 {
                let side_button = SIDE_BUTTONS.binary_search(&message[1]);
                let grid_button = midi_to_id.get(&message[1]);
                if side_button.is_ok() {
                    let rate_index = side_button.unwrap();
                    let rate = REPEAT_RATES[rate_index];
                    if message[2] > 0 {
                        tx_input.send(LoopGridMessage::SetRate(rate)).unwrap();
                        tx_input.send(LoopGridMessage::SetRepeating(rate_index > 0)).unwrap();
                    }
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
                    111 => LoopGridMessage::SelectButton(active),
                    _ => LoopGridMessage::None
                };
                tx_input.send(to_send).unwrap();
            }
        }, ()).unwrap();

        thread::spawn(move || {
            let mut loop_length = 8.0;
            let mut loop_state = LoopState::new(loop_length, move |value| {
                loop_length = value.length;
                tx_loop_state.send(LoopGridMessage::InitialLoop).unwrap();
                tx_loop_state.send(LoopGridMessage::RefreshActive).unwrap();
            });
            let mut repeating = false;
            let (midi_to_id, id_to_midi) = get_grid_map();

            // selecton
            let mut selection_override = LoopTransform::None;
            let mut selection: HashSet<u32> = HashSet::new();
            let mut suppressing = false;
            let mut holding = false;
            let mut selecting = false;
            let mut loop_from = 0.0;
            let mut should_flatten = false;

            let mut rate = 2.0;
            let mut recorder = LoopRecorder::new();
            let mut last_pos = 0.0;
            let mut last_tick = 0;
            let mut last_playback_pos = 0.0;
            let mut override_values: HashMap<u32, LoopTransform> = HashMap::new();
            let mut input_values: HashMap<u32, OutputValue> = HashMap::new();

            // out state
            let mut out_values: HashMap<u32, u8> = HashMap::new();
            let mut grid_out: HashMap<u32, Light> = HashMap::new();
            let mut select_out = Light::Off;

            // display state
            let mut active: HashSet<u32> = HashSet::new();
            let mut recording: HashSet<u32> = HashSet::new();

            let mut last_beat_light = SIDE_BUTTONS[7];
            let mut last_repeat_light = SIDE_BUTTONS[7];

            let tick_pos_increment = 1.0 / 24.0;
            
            // default button lights
            output.send(&[176, 104, Light::YellowMed as u8]).unwrap();
            output.send(&[176, 106, Light::RedLow as u8]).unwrap();
            output.send(&[176, 107, Light::RedLow as u8]).unwrap();

            for received in rx {
                match received {
                    LoopGridMessage::Schedule(tick) => {
                        let position = (tick as f64) / 24.0;
                        let current_loop = loop_state.get();

                        // loop playback
                        let offset = current_loop.offset % current_loop.length;
                        let playback_pos = current_loop.offset + ((position - offset) % current_loop.length);
                        let playback_range = recorder.get_range(playback_pos, playback_pos + tick_pos_increment);

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
                                    let repeat_position = position % rate;
                                    let half = rate / 2.0;
                                    if repeat_position < tick_pos_increment {
                                        tx_feedback.send(LoopGridMessage::Event(LoopEvent {
                                            value: OutputValue::On,
                                            pos: position,
                                            id: id.clone()
                                        })).unwrap();
                                    } else if repeat_position >= half && repeat_position < half + tick_pos_increment {
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
                                    let to = from + tick_pos_increment;
                                    let playback_range = PlaybackRange::new(from, to);
                                    let events = match playback_cache.entry(playback_range) {
                                        Occupied(mut entry) => entry.into_mut(),
                                        Vacant(entry) => entry.insert(recorder.get_range(from, to))
                                    };

                                    if playback_pos % rate < tick_pos_increment {
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
                        last_tick = tick;
                        last_playback_pos = playback_pos;

                        tx_feedback.send(LoopGridMessage::RefreshSideButtons).unwrap();
                        tx_feedback.send(LoopGridMessage::RefreshRecording).unwrap();
                    },
                    LoopGridMessage::RefreshSideButtons => {
                        let beat_display_multiplier = 8.0 / loop_length;
                        let shifted_beat_position = (last_pos * beat_display_multiplier) as usize;
                        let current_beat_light = SIDE_BUTTONS[shifted_beat_position % 8];
                        let current_repeat_light = SIDE_BUTTONS[REPEAT_RATES.iter().position(|v| v == &rate).unwrap_or(0)];
                        let rate_color = Light::YellowMed;

                        if current_repeat_light != last_repeat_light {
                            output.send(&[144, last_repeat_light, 0]).unwrap();
                            output.send(&[144, current_repeat_light, Light::Yellow as u8]).unwrap();
                        }

                        let beat_start = last_tick % 24 == 0;

                        let base_last_beat_light = rate_color.maybe(current_repeat_light == last_beat_light);
                        let base_beat_light = rate_color.maybe(current_repeat_light == current_beat_light);

                        if current_beat_light != last_beat_light {
                            output.send(&[144, last_beat_light, base_last_beat_light.unwrap_or(Light::Off) as u8]).unwrap();
                            if !beat_start {
                                output.send(&[144, current_beat_light, base_beat_light.unwrap_or(Light::GreenLow) as u8]).unwrap();
                            }
                        }

                        if beat_start {
                            output.send(&[144, current_beat_light, Light::Green as u8]).unwrap();
                        } else if last_tick % 24 == 3 {
                            output.send(&[144, current_beat_light, base_beat_light.unwrap_or(Light::GreenLow) as u8]).unwrap();
                        }

                        last_beat_light = current_beat_light;
                        last_repeat_light = current_repeat_light;
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
                                if repeating {
                                    LoopTransform::Repeat(rate, 0.0)
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
                        let value = match get_transform(&id, &override_values, &selection, &selection_override, &current_loop.transforms) {
                            &LoopTransform::On => Some(OutputValue::On),
                            &LoopTransform::None => {
                                match recorder.get_event_at(id, last_playback_pos) {
                                    Some(event) if event.value != OutputValue::Off => {
                                        match recorder.get_next_event_at(id, last_playback_pos)  {
                                            Some(next_event) if (next_event.pos - last_playback_pos) > 0.5 => Some(event.value.clone()),
                                            _ => Some(OutputValue::Off)
                                        }
                                    },
                                    _ => Some(OutputValue::Off)
                                }
                            },
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
                        let out_value = out_values.get(&id).unwrap_or(&0);
                        let old_value = grid_out.remove(&id).unwrap_or(Light::Off);
                        let new_value = if out_value > &0 {
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
                            output.send(&[144, *midi_id.unwrap(), new_value as u8]).unwrap();
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
                        let ids = recorder.get_ids_in_range(current_loop.offset, current_loop.offset + current_loop.length);
                        
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
                            output.send(&[176, 111, new_state as u8]).unwrap();
                            select_out = new_state;
                        }
                    },
                    LoopGridMessage::Event(event) => {
                        let new_value: u8 = match event.value {
                            OutputValue::On => 127,
                            OutputValue::Off => 0
                        };
                        let midi_id = id_to_midi.get(&event.id);
                        if midi_id.is_some() {
                            match maybe_update(&mut out_values, event.id, new_value) {
                                Some(value) => {
                                    tx_feedback.send(LoopGridMessage::RefreshGridButton(event.id)).unwrap();
                                    // TODO: actual playback here
                                    recorder.add(event);
                                },
                                None => ()
                            };
                        }
                    },
                    LoopGridMessage::LoopButton(pressed) => {
                        if pressed {
                            loop_from = last_pos;
                        } else {
                            let beats_since_press = last_pos - loop_from;
                            let threshold = tick_pos_increment * 12.0;
                            if beats_since_press > threshold {
                                let quantized_length = quantize_length(last_pos - loop_from);
                                loop_state.set(Loop::new(last_pos - quantized_length, quantized_length));
                            } else {
                                loop_state.set(Loop::new(loop_from - loop_length, loop_length));
                            }
                        }
                    },
                    LoopGridMessage::RefreshShouldFlatten => {
                        let new_value = &selection_override != &LoopTransform::None || override_values.values().any(|value| value != &LoopTransform::None);
                        if new_value != should_flatten {
                            should_flatten = new_value;
                            let color = if should_flatten {
                                Light::GreenLow
                            } else {
                                Light::Off
                            };
                            output.send(&[176, 105, color as u8]).unwrap();
                        }
                    },
                    LoopGridMessage::FlattenButton(pressed) => {
                        if pressed {
                            // quick hack to clear loop
                            if should_flatten {
                                let mut new_loop = loop_state.get().clone();

                                // add suppressor and holder transforms
                                if selection_override != LoopTransform::None {
                                    if selection.len() > 0 {
                                        for id in &selection {
                                            new_loop.transforms.insert(id.clone(), selection_override.clone());
                                        }
                                    } else {
                                        for id in id_to_midi.keys() {
                                            new_loop.transforms.insert(id.clone(), selection_override.clone());
                                        }
                                    }
                                }

                                // add repeater transforms
                                for (id, transform) in &override_values {
                                    if transform != &LoopTransform::None {
                                        new_loop.transforms.insert(id.clone(), transform.clone());
                                    }
                                }

                                loop_state.set(new_loop);
                            } else {
                                loop_state.set(Loop::new(0.0 - loop_length, loop_length));
                            }
                        }
                    },
                    LoopGridMessage::UndoButton(pressed) => {
                        if pressed {
                            if selecting {
                                loop_length = (loop_length / 2.0).max(1.0 / 4.0);
                            } else {
                                loop_state.undo();
                            }
                        }
                    },   
                    LoopGridMessage::RedoButton(pressed) => {
                        if pressed {
                            if selecting {
                                loop_length = (loop_length * 2.0).min(4.0 * 8.0);
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
                            for id in &selection {
                                tx_feedback.send(LoopGridMessage::RefreshGridButton(*id)).unwrap();
                            }
                            selection.clear();
                        }
                        tx_feedback.send(LoopGridMessage::RefreshSelectState).unwrap();
                    },
                    LoopGridMessage::SetRepeating(value) => {
                        repeating = value;
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
                    LoopGridMessage::None => ()
                }
            }
        });

        LoopGridLaunchpad {
            port_name: String::from(port_name),
            tx,
            input
        }
    }

    pub fn get_channel(&self) -> mpsc::Sender<LoopGridMessage> {
        mpsc::Sender::clone(&self.tx)
    }
}

fn maybe_update (hash_map: &mut HashMap<u32, u8>, key: u32, new_value: u8) -> Option<u8> {
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

    if (selection_override != &LoopTransform::None) && in_selection {
        selection_override
    } else {
        let id_override = override_values.get(&id).unwrap_or(&LoopTransform::None);
        if id_override != &LoopTransform::None {
            id_override
        } else {
            loop_transforms.get(id).unwrap_or(&LoopTransform::None)
        }
    }
}

fn quantize_length (length: f64) -> f64 {
    let grid = get_quantize_grid(length);
    (length / grid).round() * grid
}

fn get_quantize_grid (length: f64) -> f64 {
  if length < 0.7 {
    0.5
  } else if length < 1.7 {
    1.0
  } else {
    2.0
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