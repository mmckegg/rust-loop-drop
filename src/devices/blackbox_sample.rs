use ::chunk::{Triggerable, OutputValue};
use ::midi_connection;

use std::collections::HashMap;

const DRUMS: [u8; 8] = [36, 37, 38, 39, 40, 41, 42, 43];

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
                let note_id = DRUMS[id as usize % DRUMS.len()];

                // send note
                self.midi_port.send(&[144 - 1 + channel, note_id, velocity]).unwrap();
                self.output_values.insert(id, (channel, note_id, velocity));
            }
        }
    }
}
