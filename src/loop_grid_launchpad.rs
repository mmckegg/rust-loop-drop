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
    HoldButton(bool),
    Event(LoopEvent),
    InitialLoop,
    RefreshInput(u32),
    RefreshOverride(u32),
    RefreshSelectionOverride,
    SetRepeating(bool),
    SetRate(f64),
    None
}

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
    RedLow = 77
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
                    _ => LoopGridMessage::None
                };
                tx_input.send(to_send).unwrap();
            }
        }, ()).unwrap();

        thread::spawn(move || {
            let mut loop_length = 8.0;
            let mut loop_state = LoopState::new(loop_length, move |_value| {
                tx_loop_state.send(LoopGridMessage::InitialLoop).unwrap();
            });
            let mut repeating = false;
            let (midi_to_id, id_to_midi) = get_grid_map();

            // selecton
            let mut selection_override = LoopTransform::None;
            let mut selection: HashSet<u32> = HashSet::new();
            let mut suppressing = false;
            let mut holding = false;

            let mut rate = 2.0;
            let mut last_beat = 7;
            let mut recorder = LoopRecorder::new();
            let mut last_pos = 0.0;
            let mut last_playback_pos = 0.0;
            let mut out_values: HashMap<u32, u8> = HashMap::new();
            let mut override_values: HashMap<u32, LoopTransform> = HashMap::new();
            let mut input_values: HashMap<u32, OutputValue> = HashMap::new();

            let tick_pos_increment = 1.0 / 24.0;

            //loop_state.add_transform(0, LoopTransform::Repeat(0.5, 0.0));
            
            // light up undo buttons
            output.send(&[176, 106, Light::RedLow as u8]).unwrap();
            output.send(&[176, 107, Light::RedLow as u8]).unwrap();

            for received in rx {
                match received {
                    LoopGridMessage::Schedule(tick) => {
                        let position = (tick as f64) / 24.0;
                        let beat = position.floor() as usize;
                        let current_loop = loop_state.get();

                        // visual beat ticker
                        let last_beat_light = SIDE_BUTTONS[last_beat % 8];
                        if last_beat != beat {
                            let beat_light = SIDE_BUTTONS[beat % 8];
                            output.send(&[144, last_beat_light, 0]).unwrap();
                            output.send(&[144, beat_light, Light::Green as u8]).unwrap();
                            last_beat = beat
                        } else if tick % 24 == 3 {
                            output.send(&[144, last_beat_light, Light::GreenLow as u8]).unwrap();
                        }

                        // loop playback
                        let offset = current_loop.offset % current_loop.length;
                        let playback_pos = current_loop.offset + ((position - offset) % current_loop.length);
                        let playback_range = recorder.get_range(playback_pos, playback_pos + tick_pos_increment);

                        // restart loop
                        if playback_pos == current_loop.offset {
                            tx_feedback.send(LoopGridMessage::InitialLoop).unwrap();
                        }

                        // trigger events for current tick
                        for event in playback_range {
                            let transform = get_transform(&event.id, &override_values, &selection, &selection_override);
                            if transform == &LoopTransform::None {
                                tx_feedback.send(LoopGridMessage::Event(LoopEvent {
                                    value: event.value,
                                    pos: position,
                                    id: event.id
                                })).unwrap();
                            }
                        }

                        for id in id_to_midi.keys() {
                            match get_transform(&id, &override_values, &selection, &selection_override) {
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
                                _ => ()
                            }
                        }

                        for (id, value) in &override_values {
                            match value {
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
                                _ => ()
                            }
                        }

                        last_pos = position;
                        last_playback_pos = playback_pos;
                    },
                    LoopGridMessage::GridInput(_stamp, id, value) => {
                        input_values.insert(id, value);
                        tx_feedback.send(LoopGridMessage::RefreshInput(id)).unwrap();
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
                        let value = match get_transform(&id, &override_values, &selection, &selection_override) {
                            &LoopTransform::On => OutputValue::On,
                            &LoopTransform::None => {
                                match recorder.get_event_at(id, last_playback_pos) {
                                    Some(event) if event.value != OutputValue::Off => {
                                        match recorder.get_next_event_at(id, last_playback_pos)  {
                                            Some(next_event) if (next_event.pos - last_playback_pos) > 0.5 => event.value.clone(),
                                            _ => OutputValue::Off
                                        }
                                    },
                                    _ => OutputValue::Off
                                }
                            },
                            _ => OutputValue::Off
                        };

                        tx_feedback.send(LoopGridMessage::Event(LoopEvent {
                            value,
                            pos: last_pos,
                            id
                        })).unwrap();
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
                    LoopGridMessage::Event(event) => {
                        let new_value: u8 = match event.value {
                            OutputValue::On => 127,
                            OutputValue::Off => 0
                        };
                        let midi_id = id_to_midi.get(&event.id);
                        if midi_id.is_some() {
                            match maybe_update(&mut out_values, event.id, new_value) {
                                Some(value) => {
                                    output.send(&[144, *midi_id.unwrap(), new_value]).unwrap();
                                    recorder.add(event);
                                },
                                None => ()
                            };
                        }
                    },
                    LoopGridMessage::LoopButton(pressed) => {
                        if pressed {
                            loop_state.set(Loop {
                                offset: last_pos - loop_length,
                                length: loop_length
                            });
                        }
                    },
                    LoopGridMessage::FlattenButton(pressed) => {
                        if pressed {
                            // quick hack to clear loop
                            loop_state.set(Loop {
                                offset: 0.0 - loop_length,
                                length: loop_length
                            });
                        }
                    },
                    LoopGridMessage::UndoButton(pressed) => {
                        if pressed {
                            loop_state.undo();
                        }
                    },   
                    LoopGridMessage::RedoButton(pressed) => {
                        if pressed {
                            loop_state.redo();
                        }
                    },  
                    LoopGridMessage::HoldButton(pressed) => {
                        holding = pressed;
                        tx_feedback.send(LoopGridMessage::RefreshSelectionOverride).unwrap();
                    },
                    LoopGridMessage::SuppressButton(pressed) => {
                        suppressing = pressed;
                        tx_feedback.send(LoopGridMessage::RefreshSelectionOverride).unwrap();
                    },
                    LoopGridMessage::SetRepeating(value) => {
                        repeating = value;
                    },
                    LoopGridMessage::SetRate(value) => {
                        rate = value;
                        tx_feedback.send(LoopGridMessage::RefreshSelectionOverride).unwrap();
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

fn get_transform<'a> (id: &u32, override_values: &'a HashMap<u32, LoopTransform>, selection: &HashSet<u32>, selection_override: &'a LoopTransform) -> &'a LoopTransform {
    let in_selection = selection.len() == 0 || selection.contains(&id);
    if (selection_override != &LoopTransform::None) && in_selection {
        selection_override
    } else {
        override_values.get(&id).unwrap_or(&LoopTransform::None)
    }
}