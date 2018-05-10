use ::chunk::{Triggerable, OutputValue, SystemTime};
use ::midi_connection;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
pub use ::scale::Scale;
use std::collections::HashSet;

use std::collections::HashMap;

pub struct SP404 {
    output_values: HashMap<u32, (u8, u8, u8)>,
    offset: Arc<AtomicUsize>,
    midi_channel: u8,
    midi_port: midi_connection::SharedMidiOutputConnection
}

impl SP404 {
    pub fn new (midi_port: midi_connection::SharedMidiOutputConnection, midi_channel: u8, offset: Arc<AtomicUsize>) -> Self {
        SP404 {
            output_values: HashMap::new(),
            offset,
            midi_channel,
            midi_port
        }
    }
}

impl Triggerable for SP404 {
    fn trigger (&mut self, id: u32, value: OutputValue, _at: SystemTime) {
        match value {
            OutputValue::Off => {
                if self.output_values.contains_key(&id) {
                    let (channel, note_id, _) = *self.output_values.get(&id).unwrap();
                    self.midi_port.send(&[128 - 1 + channel, note_id, 0]).unwrap();
                    self.output_values.remove(&id);
                }
            },
            OutputValue::On(velocity) => {
                let mut offset_value = self.offset.load(Ordering::Relaxed);
                let mut channel = if offset_value < 5 {
                    self.midi_channel
                } else {
                    self.midi_channel + 1
                };

                let note_id = (47 + ((offset_value % 5) * 12) + (id as usize)) as u8;

                if let Some(&(previous_channel, previous_note, _)) = self.output_values.get(&id) {
                    if previous_channel != channel || previous_note != note_id {
                        self.midi_port.send(&[128 - 1 + previous_channel, previous_note, 0]).unwrap();
                    }
                }

                self.output_values.insert(id, (channel, note_id, 127));
                self.midi_port.send(&[144 - 1 + channel, note_id, 127]).unwrap();
            }
        }
    }
    fn shouldChokeAll (&self) -> bool { true }
}

enum SP404Message {
    Choke,
    Trigger(u32, u8)
}