use ::midi_connection;
use std::collections::HashMap;
use std::time::SystemTime;
use std::sync::{Arc, Mutex};
use ::output_value::OutputValue;

pub use ::scale::{Scale, Offset};
pub use ::midi_connection::SharedMidiOutputConnection;

pub struct MidiKeys {
    midi_output: midi_connection::SharedMidiOutputConnection,
    midi_channel: u8,
    output_values: HashMap<u32, u8>,
    scale: Arc<Mutex<Scale>>,
    offset: Arc<Mutex<Offset>>
}

impl MidiKeys {
    pub fn new (midi_output: midi_connection::SharedMidiOutputConnection, channel: u8, scale: Arc<Mutex<Scale>>, offset: Arc<Mutex<Offset>>) -> Self {
        MidiKeys {
            midi_output,
            midi_channel: channel,
            output_values: HashMap::new(),
            offset,
            scale
        }
    }

    pub fn note (&mut self, id: u32, value: OutputValue, time: SystemTime) {
        match value {
            OutputValue::Off => {
                if self.output_values.contains_key(&id) {
                    let note_id = *self.output_values.get(&id).unwrap();
                    self.midi_output.send_at(&[144 + self.midi_channel - 1, note_id, 0], time).unwrap();
                    self.output_values.remove(&id);
                }
            },
            OutputValue::On(velocity) => {
                let scale = self.scale.lock().unwrap();
                let offset = self.offset.lock().unwrap();
                let scale_offset = offset.base + offset.offset;
                let note_id = (scale.get_note_at((id as i32) + scale_offset) + offset.pitch + (offset.oct * 12)) as u8;
                self.midi_output.send_at(&[144 + self.midi_channel - 1, note_id, velocity], time).unwrap();
                self.output_values.insert(id, note_id);
            }
        }
    }

    // pub fn midi_output (&self) -> &midi_connection::SharedMidiOutputConnection {
    //     &self.midi_output
    // }
}