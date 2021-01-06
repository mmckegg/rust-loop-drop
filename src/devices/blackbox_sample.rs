use ::chunk::{Triggerable, OutputValue, LatchMode};
use ::midi_connection;

use std::collections::HashMap;

const TRIGGERS: [u8; 8] = [48, 49, 50, 51, 44, 45, 46, 47];

pub struct BlackboxSample {
    midi_port: midi_connection::SharedMidiOutputConnection,
    midi_channel: u8,
    output_values: HashMap<u32, (u8, u8, u8)>
}

impl BlackboxSample {
    pub fn new (midi_port: midi_connection::SharedMidiOutputConnection, channel: u8) -> Self {
        BlackboxSample {
            midi_port,
            midi_channel: channel,
            output_values: HashMap::new()
        }
    }
}

impl Triggerable for BlackboxSample {
    fn trigger (&mut self, id: u32, value: OutputValue) {
        match value {
            OutputValue::Off => {
                if self.output_values.contains_key(&id) {
                    let (channel, note_id, _) = *self.output_values.get(&id).unwrap();
                    self.midi_port.send(&[144 - 1 + channel, note_id, 0]).unwrap();
                    self.output_values.remove(&id);
                }
            },
            OutputValue::On(base_velocity) => {
                let velocity = base_velocity;

                let channel = self.midi_channel;
                let note_id = TRIGGERS[id as usize % TRIGGERS.len()];

                // send note
                self.midi_port.send(&[144 - 1 + channel, note_id, velocity]).unwrap();
                self.output_values.insert(id, (channel, note_id, velocity));
            }
        }
    }

    fn latch_mode (&self) -> LatchMode {
        LatchMode::LatchSuppress
    }
}
