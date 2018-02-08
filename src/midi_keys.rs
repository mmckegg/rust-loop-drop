use ::midi_connection;
use std::collections::HashMap;
use std::time::SystemTime;
use std::sync::{Arc, Mutex};

use ::output_value::OutputValue;
pub use ::scale::{Scale, Offset};

pub struct MidiKeys {
    midi_output: midi_connection::MidiOutputConnection,
    midi_channel: u8,
    output_values: HashMap<u32, u8>,
    scale: Arc<Mutex<Scale>>,
    offset: Arc<Mutex<Offset>>
}

impl MidiKeys {
    pub fn new (port_name: &str, channel: u8, scale: Arc<Mutex<Scale>>, offset: Arc<Mutex<Offset>>) -> Self {
        MidiKeys {
            midi_output: midi_connection::get_output(port_name).unwrap(),
            midi_channel: channel,
            output_values: HashMap::new(),
            offset,
            scale
        }
    }

    pub fn note (&mut self, id: u32, value: OutputValue, at: SystemTime) {
        match value {
            OutputValue::Off => {
                if self.output_values.contains_key(&id) {
                    let note_id = *self.output_values.get(&id).unwrap();
                    self.midi_output.send(&[144 + self.midi_channel - 1, note_id, 0]).unwrap();
                    self.output_values.remove(&id);
                }
            },
            OutputValue::On(velocity) => {
                let octave = -2;
                let offsets = [-9, -7, -5, -4, -3, 0, 2, 3, 4, 7, 9];
                let scale = self.scale.lock().unwrap();
                let offset = self.offset.lock().unwrap();
                let third_offset = offsets[(((offsets.len() as i32) / 2) + offset.third) as usize];
                let scale_offset = third_offset + offset.offset;
                let note_id = (scale.get_note_at((id as i32) + scale_offset) + offset.pitch + (offset.oct * 12)) as u8;
                self.midi_output.send(&[144 + self.midi_channel - 1, note_id, velocity]).unwrap();
                self.output_values.insert(id, note_id);
            }
        }
    }

    pub fn midi_output (&self) -> &midi_connection::MidiOutputConnection {
        &self.midi_output
    }
}

fn modulo (n: i32, m: i32) -> i32 {
    ((n % m) + m) % m
}
