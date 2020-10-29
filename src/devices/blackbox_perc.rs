use ::chunk::{Triggerable, OutputValue, SystemTime, MidiTime};
use ::midi_connection;
use std::sync::{Arc, Mutex};

use std::collections::HashMap;

const DRUMS: [u8; 4] = [40, 41, 42, 43];

pub struct BlackboxPerc {
    midi_port: midi_connection::SharedMidiOutputConnection,
    midi_channel: u8,
    last_pos: MidiTime,
    velocities: Arc<Mutex<HashMap<u32, u8>>>,
    output_values: HashMap<u32, (u8, u8, u8)>
}

impl BlackboxPerc {
    pub fn new (midi_port: midi_connection::SharedMidiOutputConnection, channel: u8, velocities: Arc<Mutex<HashMap<u32, u8>>>) -> Self {
        BlackboxPerc {
            midi_port,
            last_pos: MidiTime::zero(),
            velocities,
            midi_channel: channel,
            output_values: HashMap::new()
        }
    }
}

impl Triggerable for BlackboxPerc {
    fn on_tick (&mut self, time: MidiTime) {
        self.last_pos = time;
    }

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
                let velocities = self.velocities.lock().unwrap();
                let velocity_pos = self.last_pos.ticks() / MidiTime::from_measure(1, 4).ticks() % 8;
                let pos = self.last_pos % MidiTime::from_measure(1, 4);
                let velocity = if pos.ticks() == 0 && velocities.get(&(velocity_pos as u32)).is_some() {
                    (base_velocity + 50).min(127)
                } else {
                    base_velocity
                };
                
                let channel = self.midi_channel;
                let note_id = DRUMS[id as usize % DRUMS.len()];
                
                // send note
                self.midi_port.send(&[144 - 1 + channel, note_id, velocity]).unwrap();
                self.output_values.insert(id, (channel, note_id, velocity));
            }
        }
    }
}