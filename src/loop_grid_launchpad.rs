extern crate midir;
use self::midir::{MidiInputConnection};
use std::sync::mpsc;
use std::thread;
use std::collections::HashSet;
use std::collections::HashMap;
use std::collections::hash_map::Entry;

use ::loop_recorder::{LoopRecorder, EventType, LoopEvent};
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

        let mut output = midi_connection::get_output(&port_name).unwrap();
        let input = midi_connection::get_input(&port_name, move |stamp, message, _| {
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