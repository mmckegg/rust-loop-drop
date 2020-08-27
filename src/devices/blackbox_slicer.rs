use ::chunk::{Triggerable, OutputValue};
use ::midi_connection;

use std::collections::HashMap;

pub struct BlackboxSlicer {
    midi_port: midi_connection::SharedMidiOutputConnection,
    midi_channel: u8,
    output_values: HashMap<u32, (u8, u8, u8)>
}

impl BlackboxSlicer {
    pub fn new (midi_port: midi_connection::SharedMidiOutputConnection, channel: u8) -> Self {
        BlackboxSlicer {
            midi_port,
            midi_channel: channel,
            output_values: HashMap::new()
        }
    }
}

impl Triggerable for BlackboxSlicer {
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
                let note_id = id as u8 + 36;

                // send note
                self.midi_port.send(&[144 - 1 + channel, note_id, velocity]).unwrap();
                self.output_values.insert(id, (channel, note_id, velocity));
            }
        }
    }
}
