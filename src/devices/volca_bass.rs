use ::chunk::{Triggerable, OutputValue, SystemTime};
use ::midi_keys::{MidiKeys, Scale, Offset, SharedMidiOutputConnection};

use std::sync::{Arc, Mutex};

pub struct VolcaBass {
    midi_keys: MidiKeys
}

impl VolcaBass {
    pub fn new (midi_port: SharedMidiOutputConnection, channel: u8, scale: Arc<Mutex<Scale>>, offset: Arc<Mutex<Offset>>) -> Self {
        VolcaBass {
            midi_keys: MidiKeys::new(midi_port, channel, scale, offset)
        }
    }
}

impl Triggerable for VolcaBass {
    fn trigger (&mut self, id: u32, value: OutputValue, at: SystemTime) {
        self.midi_keys.note(id, value, at);
    }

    fn on_tick (&mut self) {
        self.midi_keys.on_tick();
    }
}