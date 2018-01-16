extern crate midir;
use self::midir::{MidiInputConnection};
use std::sync::mpsc;
use std::thread;
use std::collections::HashSet;
use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::collections::hash_map::Entry::{Occupied, Vacant};
use ::loop_recorder::{LoopRecorder, OutputValue, LoopEvent};
use ::midi_connection;
use std::rc::Rc;
use std::rc::Weak;

const side_buttons: [u8; 8] = [8, 24, 40, 56, 72, 88, 104, 120];
const repeat_rates: [f64; 8] = [2.0, 1.0, 2.0 / 3.0, 1.0 / 2.0, 1.0 / 3.0, 1.0 / 4.0, 1.0 / 6.0, 1.0 / 8.0];

pub enum LoopGridMessage {
    Schedule(u64),
    GridInput(u64, u32, OutputValue),
    LoopButton(bool),
    FlattenButton(bool),
    UndoButton(bool),
    RedoButton(bool),
    Event(LoopEvent),
    InitialLoop,
    RefreshInput(u32),
    RefreshOverride(u32),
    SetRepeating(bool),
    SetRate(f64),
}

#[derive(Debug)]
pub struct Loop {
    length: f64,
    offset: f64,
    //transforms: Vec<LoopTransform>
}

#[derive(PartialEq, Debug)]
pub enum LoopTransform {
    On,
    None,
    Repeat(f64, f64),
    Suppress
}

pub struct LoopState {
    undos: Vec<Loop>,
    redos: Vec<Loop>,
    transforms: HashMap<u32, Vec<Rc<LoopTransform>>>,
    on_change: Box<FnMut(&Loop) + Send>
}

impl LoopState {
    pub fn new<F> (defaultLength: f64, on_change: F) -> LoopState
    where F: FnMut(&Loop) + Send + 'static  {
        let default_loop = Loop {
            offset: 0.0 - defaultLength,
            length: defaultLength
        };
        LoopState {
            undos: vec![default_loop],
            redos: Vec::new(),
            transforms: HashMap::new(),
            on_change: Box::new(on_change)
        }
    }

    pub fn get (&self) -> &Loop {
        &self.undos.last().unwrap()
    }

    pub fn set (&mut self, value: Loop) {
        self.clear_transforms();
        self.undos.push(value);
        (self.on_change)(self.undos.last().unwrap());
    }

    pub fn add_transform (&mut self, id: u32, transform: LoopTransform) -> Weak<LoopTransform> {
        let transform_rc = Rc::new(transform);
        let transform_weak = Rc::downgrade(&transform_rc);

        match self.transforms.entry(id) {
            Occupied(mut entry) => {
                entry.get_mut().push(transform_rc);
            },
            Vacant(mut entry) => {
                entry.insert(vec![transform_rc]);
            }
        };

        transform_weak
    }

    pub fn remove_transform (&mut self, id: u32, transform: Weak<LoopTransform>) {
        let mut i = 0;
        let transforms = self.transforms.get_mut(&id).unwrap();
        let transform_upgrade = transform.upgrade().unwrap();
        while i < transforms.len() {
            if transform_upgrade == transforms[i] {
                transforms.remove(i);
            } else {
                i += 1;
            }
        }
    }

    pub fn clear_transforms (&mut self) {
        self.transforms.clear();
    }

    pub fn undo (&mut self) {
        if self.undos.len() > 1 {
            match self.undos.pop() {
                Some(value) => {
                    self.redos.push(value);
                    (self.on_change)(self.undos.last().unwrap());
                },
                None => ()
            };
        }
    }

    pub fn redo (&mut self) {
        match self.redos.pop() {
            Some(value) => {
                self.undos.push(value);
                (self.on_change)(self.undos.last().unwrap());
            },
            None => ()
        };
    }
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
        let mut tx_feedback =  mpsc::Sender::clone(&tx);
        let mut tx_loop_state =  mpsc::Sender::clone(&tx);

        let mut output = midi_connection::get_output(&port_name).unwrap();
        let input = midi_connection::get_input(&port_name, move |stamp, message, _| {
            if message[0] == 144 || message[0] == 128 {
                let side_button = side_buttons.binary_search(&message[1]);
                if side_button.is_ok() {
                    let rate_index = side_button.unwrap();
                    let rate = repeat_rates[rate_index];
                    if message[2] > 0 {
                        tx_input.send(LoopGridMessage::SetRate(rate)).unwrap();
                        tx_input.send(LoopGridMessage::SetRepeating(rate_index > 0)).unwrap();
                    }
                } else {
                    let value = if message[2] > 0 {
                        OutputValue::On
                    } else {
                        OutputValue::Off
                    };
                    tx_input.send(LoopGridMessage::GridInput(stamp, message[1] as u32, value)).unwrap();
                } ;
            } else if message[0] == 176 {
                if message[1] == 104 {
                    tx_input.send(LoopGridMessage::LoopButton(message[2] > 0)).unwrap();
                } else if message[1] == 105 {
                    tx_input.send(LoopGridMessage::FlattenButton(message[2] > 0)).unwrap();
                } else if message[1] == 106 {
                    tx_input.send(LoopGridMessage::UndoButton(message[2] > 0)).unwrap();
                } else if message[1] == 107 {
                    tx_input.send(LoopGridMessage::RedoButton(message[2] > 0)).unwrap();
                }
            }
        }, ()).unwrap();

        thread::spawn(move || {
            let mut loop_length = 8.0;
            let mut loop_state = LoopState::new(loop_length, move |_value| {
                tx_loop_state.send(LoopGridMessage::InitialLoop).unwrap();
            });
            let mut repeating = false;
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
                        let last_beat_light = side_buttons[last_beat % 8];
                        if last_beat != beat {
                            let beat_light = side_buttons[beat % 8];
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

                        let transforms = &loop_state.transforms;

                        // trigger events for current tick
                        for event in playback_range {
                            let override_value = override_values.get(&event.id).unwrap_or(&LoopTransform::None);
                            if !transforms.contains_key(&event.id) && override_value == &LoopTransform::None {
                                tx_feedback.send(LoopGridMessage::Event(LoopEvent {
                                    value: event.value,
                                    pos: position,
                                    id: event.id
                                })).unwrap();
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
                    LoopGridMessage::GridInput(stamp, id, value) => {
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
                        let transform = override_values.get(&id).unwrap_or(&LoopTransform::None);
                        let value = match transform {
                            &LoopTransform::On => OutputValue::On,
                            &LoopTransform::None => {
                                match recorder.get_event_at(id, last_playback_pos) {
                                    Some(event) => event.value,
                                    None => OutputValue::Off
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
                    LoopGridMessage::Event(event) => {
                        let new_value: u8 = match event.value {
                            OutputValue::On => 127,
                            OutputValue::Off => 0
                        };
                        match maybe_update(&mut out_values, event.id, new_value) {
                            Some(value) => {
                                output.send(&[144, event.id as u8, new_value]).unwrap();
                                recorder.add(event);
                            },
                            None => ()
                        };
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
                    LoopGridMessage::SetRate(value) => {
                        rate = value;
                        for (id, value) in &override_values {
                            if value != &LoopTransform::None {
                                tx_feedback.send(LoopGridMessage::RefreshInput(*id)).unwrap();
                            }
                        }
                    },
                    LoopGridMessage::SetRepeating(value) => {
                        repeating = value;
                    },                           
                    LoopGridMessage::InitialLoop => {
                        let mut used_keys = HashSet::new();
                        let current_loop = loop_state.get();
                        let playback_range = recorder.get_range(current_loop.offset, current_loop.offset + current_loop.length);
                        for event in playback_range {
                            used_keys.insert(event.id);
                        }
                        for key in out_values.keys() {
                            let overriding = override_values.get(&key).unwrap_or(&LoopTransform::None);
                            if overriding == &LoopTransform::None && !used_keys.contains(key) {
                                tx_feedback.send(LoopGridMessage::Event(LoopEvent {
                                    value: OutputValue::Off,
                                    pos: last_pos,
                                    id: key.clone()
                                })).unwrap();
                            }
                        }
                        for key in &used_keys {
                            let overriding = override_values.get(&key).unwrap_or(&LoopTransform::None);
                            if overriding == &LoopTransform::None {
                                match recorder.get_event_at(key.clone(), last_playback_pos) {
                                    Some(event) => {
                                        // get offset between playback time and this event's time
                                        let playback_offset = if last_playback_pos < event.pos {
                                            last_playback_pos + current_loop.offset - event.pos
                                        } else {
                                            last_playback_pos - event.pos
                                        };

                                        // use initial state if "off" or "on" for more than 1 beat
                                        if event.value == OutputValue::Off || playback_offset > 1.0 {
                                            tx_feedback.send(LoopGridMessage::Event(LoopEvent {
                                                value: event.value,
                                                pos: last_pos,
                                                id: key.clone()
                                            })).unwrap();
                                        }
                                    },
                                    None => ()
                                }
                            }
                        }
                    }
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