use ::chunk::{Triggerable, OutputValue, SystemTime};
use ::midi_keys::{MidiKeys, Scale, Offset};

use std::sync::{Arc, Mutex};

pub struct VolcaBass {
    midi_keys: MidiKeys
}

impl VolcaBass {
    pub fn new (midi_port_name: &str, channel: u8, scale: Arc<Mutex<Scale>>, offset: Arc<Mutex<Offset>>) -> Self {
        VolcaBass {
            midi_keys: MidiKeys::new(midi_port_name, channel, scale, offset)
        }
    }
}

impl Triggerable for VolcaBass {
    fn trigger (&mut self, id: u32, value: OutputValue, at: SystemTime) {
        self.midi_keys.note(id, value, at);
    }
}