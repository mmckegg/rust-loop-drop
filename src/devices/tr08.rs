use ::chunk::{Triggerable, OutputValue, SystemTime};
use ::midi_connection;

use std::collections::HashMap;

pub struct TR08 {
    midi_port: midi_connection::MidiOutputConnection,
    midi_channel: u8,
    output_values: HashMap<u32, (u8, u8, u8)>
}

const TR08_MAP: [u8; 16] = [
    36, 38, 42, 46,
    43, 39, 70, 49,
    47, 37, 75, 56,
    50, 64, 63, 62
];

impl TR08 {
    pub fn new (midi_port_name: &str, channel: u8) -> Self {
        TR08 {
            midi_port: midi_connection::get_output(midi_port_name).unwrap(),
            midi_channel: channel,
            output_values: HashMap::new()
        }
    }
}

impl Triggerable for TR08 {
    fn trigger (&mut self, id: u32, value: OutputValue, at: SystemTime) {
        match value {
            OutputValue::Off => {
                if self.output_values.contains_key(&id) {
                    let (channel, note_id, _) = *self.output_values.get(&id).unwrap();
                    self.midi_port.send(&[128 - 1 + channel, note_id, 0]).unwrap();
                    self.output_values.remove(&id);
                }
            },
            OutputValue::On(velocity) => {
                let channel = self.midi_channel;
                let note_id = TR08_MAP[id as usize];

                self.midi_port.send(&[144 - 1 + channel, note_id, velocity]).unwrap();
                self.output_values.insert(id, (channel, note_id, velocity));
            }
        }
    }
}