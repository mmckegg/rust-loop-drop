extern crate midir;
use self::midir::{MidiInputConnection};
use std::sync::mpsc;
use std::thread;
use std::cmp::Ordering;
use std::collections::HashSet;
use std::collections::HashMap;
use std::collections::hash_map::Entry;

use ::midi_connection;

const side_buttons: [u8; 8] = [8, 24, 40, 56, 72, 88, 104, 120];

pub enum LoopGridMessage {
    Schedule(u64),
    GridInput(u64, u8, u8),
    LoopButton(bool),
    FlattenButton(bool),
    Event(LoopEvent),
    InitialLoop,
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

        let mut output = midi_connection::getOutput(&port_name).unwrap();
        let input = midi_connection::getInput(&port_name, move |stamp, message, _| {
            if message[0] == 144 || message[0] == 128 {
                tx_input.send(LoopGridMessage::GridInput(stamp, message[1], message[2])).unwrap();
            } else if message[0] == 176 {
                if message[1] == 104 {
                    tx_input.send(LoopGridMessage::LoopButton(message[2] > 0)).unwrap();
                } else if message[1] == 105 {
                    tx_input.send(LoopGridMessage::FlattenButton(message[2] > 0)).unwrap();
                }
            }
        }, ()).unwrap();

        thread::spawn(move || {
            let mut last_beat = 7;
            let mut recorder = LoopRecorder::new();
            let mut last_pos = 0.0;
            let mut loop_start_pos = -8.0;
            let mut loop_length = 8.0;
            let mut out_values: HashMap<u32, u8> = HashMap::new();
            let tick_pos_increment = 1.0 / 24.0;
            for received in rx {
                match received {
                    LoopGridMessage::Schedule(tick) => {
                        let position = (tick as f64) / 24.0;
                        let beat = position.floor() as usize;

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
                        let offset = loop_start_pos % loop_length;
                        let playback_pos = loop_start_pos + ((position - offset) % loop_length);
                        let playback_range = recorder.get_range(playback_pos, playback_pos + tick_pos_increment);

                        if playback_pos == loop_start_pos {
                            // restart loop
                            tx_feedback.send(LoopGridMessage::InitialLoop).unwrap();
                        }

                        for event in playback_range {
                            tx_feedback.send(LoopGridMessage::Event(LoopEvent {
                                event_type: event.event_type,
                                pos: position,
                                id: event.id
                            })).unwrap();
                        }

                        last_pos = position;
                    },
                    LoopGridMessage::GridInput(stamp, id, vel) => {
                        if vel > 0 {
                            tx_feedback.send(LoopGridMessage::Event(LoopEvent {
                                event_type: EventType::On,
                                pos: last_pos,
                                id: id as u32
                            })).unwrap();
                        } else {
                            tx_feedback.send(LoopGridMessage::Event(LoopEvent {
                                event_type: EventType::Off,
                                pos: last_pos,
                                id: id as u32
                            })).unwrap();
                        }
                        println!("Msg {} {} {}", stamp, id, vel);
                    },
                    LoopGridMessage::Event(event) => {
                        let new_value: u8 = match event.event_type {
                            EventType::On => 127,
                            EventType::Off => 0
                        };
                        
                        match maybe_update(&mut out_values, event.id, new_value) {
                            Some(value) => {
                                output.send(&[144, event.id as u8, new_value]).unwrap();
                                recorder.add(event);
                            },
                            None => {
                                println!("Nothing to update");
                            }
                        };
                    },
                    LoopGridMessage::LoopButton(pressed) => {
                        if pressed {
                            loop_start_pos = last_pos - loop_length;
                            println!("loop range {} -> {}", loop_start_pos, loop_start_pos + loop_length);
                            tx_feedback.send(LoopGridMessage::InitialLoop).unwrap();
                        }
                    },
                    LoopGridMessage::FlattenButton(pressed) => {
                        if pressed {
                            // quick hack to clear loop
                            loop_start_pos = 0.0 - loop_length;
                            tx_feedback.send(LoopGridMessage::InitialLoop).unwrap();
                        }
                    },
                    LoopGridMessage::InitialLoop => {
                        let mut used_keys = HashSet::new();
                        let playback_range = recorder.get_range(loop_start_pos, loop_start_pos + loop_length);
                        for event in playback_range {
                            used_keys.insert(event.id);
                        }
                        for key in out_values.keys() {
                            if !used_keys.contains(key) {
                                tx_feedback.send(LoopGridMessage::Event(LoopEvent {
                                    event_type: EventType::Off,
                                    pos: last_pos,
                                    id: key.clone()
                                })).unwrap();
                            }
                        }
                        for key in &used_keys {
                            match recorder.get_event_at(key.clone(), last_pos) {
                                Some(event) => {
                                    // use initial state if "off" or "on" for more than 1 beat
                                    if event.event_type == EventType::Off || last_pos - event.pos > 1.0 {
                                        tx_feedback.send(LoopGridMessage::Event(LoopEvent {
                                            event_type: EventType::Off,
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

#[derive(Debug, Eq, PartialEq, Copy, Clone)]
pub enum EventType {
    On,
    Off
}

#[derive(Debug)]
pub struct LoopEvent {
    event_type: EventType,
    pos: f64,
    id: u32
}

impl PartialOrd for LoopEvent {
    fn partial_cmp(&self, other: &LoopEvent) -> Option<Ordering> {
        // Some(self.cmp(other))
        let value = self.pos.partial_cmp(&other.pos).unwrap();
        if self.eq(other) {
            // replace the item if same type, 
            Some(Ordering::Equal)
        } else if value == Ordering::Equal {
            // or insert after if different (but same position)
            Some(Ordering::Greater)
        } else {
            Some(value)
        }
    }
}

impl PartialEq for LoopEvent {
    fn eq(&self, other: &LoopEvent) -> bool {
        self.pos == other.pos && self.event_type == other.event_type && self.id == other.id
    }
}

pub struct LoopRecorder {
    history: Vec<LoopEvent>
}

impl LoopRecorder {
    pub fn new () -> Self {
        Self {
            history: Vec::new()
        }
    }

    pub fn add (&mut self, event: LoopEvent) {
        match self.history.binary_search_by(|v| {
            v.partial_cmp(&event).expect("Cannot compare events (NaN?)")
        }) {
            Ok(index) => {
                self.history.push(event);
                // swap_remove removes at index and puts last item in its place
                self.history.swap_remove(index); 
            },
            Err(index) => self.history.insert(index, event)
        };
        println!("added {}", self.history.len());
    }

    pub fn get_range (&self, start_pos: f64, end_pos: f64) -> &[LoopEvent] {
        let start_index = match self.history.binary_search_by(|v| {
            v.pos.partial_cmp(&start_pos).expect("Cannot compare events (NaN?)")
        }) {
            Ok(index) => index,
            Err(index) => index
        };

        let end_index = match self.history.binary_search_by(|v| {
            v.pos.partial_cmp(&(end_pos + 0.000000001)).expect("Cannot compare events (NaN?)")
        }) {
            Ok(index) => index,
            Err(index) => index
        };

        &self.history[start_index..end_index]
    }

    pub fn get_event_at (&self, id: u32, pos: f64) -> Option<&LoopEvent> {
        let index = match self.history.binary_search_by(|v| {
            match v.pos.partial_cmp(&pos).expect("Cannot compare events (NaN?)") {
                Ordering::Greater => Ordering::Greater,
                Ordering::Less => Ordering::Less,
                Ordering::Equal => {
                    if v.id == id {
                        Ordering::Equal
                    } else {
                        Ordering::Less
                    }
                }
            }
        }) {
            Ok(index) => index,
            Err(index) => index - 1
        };

        self.history.get(index)
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