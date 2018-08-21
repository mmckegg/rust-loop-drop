use ::chunk::{Triggerable, OutputValue, SystemTime, ScheduleMode};
use ::midi_connection;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicUsize, Ordering};
pub use ::scale::Scale;
use std::collections::HashSet;

use std::collections::HashMap;

pub struct SP404 {
    output_values: HashMap<u32, (u8, u8, u8)>,
    offset: Arc<AtomicUsize>,
    velocity_map: Arc<Mutex<SP404VelocityMap>>,
    chokes: Arc<Mutex<SP404Choke>>,
    midi_channel: u8,
    midi_port: midi_connection::SharedMidiOutputConnection
}

impl SP404 {
    pub fn new (midi_port: midi_connection::SharedMidiOutputConnection, midi_channel: u8, offset: Arc<AtomicUsize>, velocity_map: Arc<Mutex<SP404VelocityMap>>, chokes: Arc<Mutex<SP404Choke>>) -> Self {
        SP404 {
            output_values: HashMap::new(),
            velocity_map,
            offset,
            chokes,
            midi_channel,
            midi_port
        }
    }
}

impl Triggerable for SP404 {
    fn trigger (&mut self, id: u32, value: OutputValue, time: SystemTime) {
        match value {
            OutputValue::Off => {
                if self.output_values.contains_key(&id) {
                    let (channel, note_id, _) = *self.output_values.get(&id).unwrap();
                    self.midi_port.send_at(&[128 - 1 + channel, note_id, 0], time).unwrap();
                    self.output_values.remove(&id);
                }
            },
            OutputValue::On(_) => {
                let velocity_map = self.velocity_map.lock().unwrap();
                let mut offset_value = self.offset.load(Ordering::Relaxed);
                let mut channel = if offset_value < 5 {
                    self.midi_channel
                } else {
                    self.midi_channel + 1
                };

                let velocity_index = id as usize % velocity_map.triggers.len();
                let velocity = ((velocity_map.master as f64 / 128.0) * velocity_map.triggers[velocity_index] as f64).min(127.0) as u8;

                let note_id = (47 + ((offset_value % 5) * 12) + (id as usize)) as u8;

                if let Some(&(previous_channel, previous_note, _)) = self.output_values.get(&id) {
                    if previous_channel != channel || previous_note != note_id {
                        self.midi_port.send_at(&[128 - 1 + previous_channel, previous_note, 0], time).unwrap();
                    }
                }

                self.output_values.insert(id, (channel, note_id, velocity));
                self.midi_port.send_at(&[144 - 1 + channel, note_id, velocity], time).unwrap();
            }
        }
    }
    fn get_chokes_for (&self, id: u32) -> Option<Vec<u32>> { 
        let chokes = self.chokes.lock().unwrap();

        let mut from = 0;
        let mut to = 12;

        // WETTEST WORST LOGIC EVER! ICANTEVENBRIAN
        if chokes.a && chokes.b {
            if id >= 8 && id < 10 {
                from = 8;
                to = 10
            } else if id >= 10 {
                from = 10
            } else {
                to = 8
            }
        } else if chokes.a && !chokes.b {
            if id >= 8 {
                from = 8;
            } else {
                to = 8
            }
        } else if !chokes.a && chokes.b {
            if id >= 10 {
                from = 10
            } else {
                to = 10
            }
        }

        let mut result = Vec::new();

        for i in from..to {
            if self.output_values.contains_key(&i) {
                result.push(i);
            }
        }

        Some(result)
    }

    fn schedule_mode (&self) -> ScheduleMode {
        ScheduleMode::Percussion
    }
}

enum SP404Message {
    Choke,
    Trigger(u32, u8)
}

impl SP404VelocityMap {
    pub fn new () -> Self {
        SP404VelocityMap {
            master: 127,
            triggers: [ 100, 100, 100, 100, 100, 100, 100, 100, 100, 100, 100, 100 ]
        }
    }
}

pub struct SP404VelocityMap {
    pub master: u8,
    pub triggers: [u8; 12]
}


pub struct SP404Choke {
    pub a: bool,
    pub b: bool
}

impl SP404Choke {
    pub fn new () -> Self {
        SP404Choke {
            a: false,
            b: false
        }
    }
}