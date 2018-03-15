use ::chunk::{Triggerable, OutputValue, SystemTime};
use ::midi_connection;

use std::collections::HashMap;

pub struct SP404 {
    midi_port: midi_connection::SharedMidiOutputConnection,
    midi_channel: u8,
    output_values: HashMap<u32, (u8, u8, u8)>,
    offset_value: u8
}

impl SP404 {
    pub fn new (midi_port: midi_connection::SharedMidiOutputConnection, channel: u8) -> Self {
        SP404 {
            midi_port,
            midi_channel: channel,
            output_values: HashMap::new(),
            offset_value: 0
        }
    }
}

impl Triggerable for SP404 {
    fn trigger (&mut self, id: u32, value: OutputValue, _at: SystemTime) {
        match value {
            OutputValue::Off => {
                // if self.output_values.contains_key(&id) {
                //     let (channel, note_id, _) = *self.output_values.get(&id).unwrap();
                //     self.midi_port.send(&[128 - 1 + channel, note_id, 0]).unwrap();
                //     self.output_values.remove(&id);
                // }
            },
            OutputValue::On(velocity) => {
                let mut offset_value = self.offset_value;
                let mut channel = self.midi_channel;

                if offset_value >= 5 {
                    channel += 1;
                    offset_value -= 5;
                }

                // choke
                for (_, &(channel, note_id, _)) in &self.output_values {
                    self.midi_port.send(&[128 - 1 + channel, note_id, 0]).unwrap();
                }

                self.output_values.clear();

                let note_id = 47 + (offset_value * 12) + (id as u8);
                self.midi_port.send(&[144 - 1 + channel, note_id, velocity]).unwrap();
                self.output_values.insert(id, (channel, note_id, velocity));
            }
        }
    }
}