use ::chunk::{Triggerable, OutputValue, SystemTime};
use ::midi_connection;
use std::sync::{Arc, Mutex};

use std::collections::HashMap;

pub struct BlofeldDrums {
    midi_port: midi_connection::SharedMidiOutputConnection,
    midi_channel: u8,
    params: Arc<Mutex<BlofeldDrumParams>>,
    output_values: HashMap<u32, (u8, u8, u8)>
}

impl BlofeldDrums {
    pub fn new (midi_port: midi_connection::SharedMidiOutputConnection, channel: u8, params: Arc<Mutex<BlofeldDrumParams>>) -> Self {
        BlofeldDrums {
            midi_port,
            params,
            midi_channel: channel,
            output_values: HashMap::new()
        }
    }
}

impl Triggerable for BlofeldDrums {
    fn trigger (&mut self, id: u32, value: OutputValue, at: SystemTime) {
        match value {
            OutputValue::Off => {
                if self.output_values.contains_key(&id) {
                    let (channel, note_id, _) = *self.output_values.get(&id).unwrap();
                    // HACK: disable off notes because this is doing weird things to blofeld for drum envelopes
                    // self.midi_port.send(&[128 - 1 + channel, note_id, 0]).unwrap();
                    self.output_values.remove(&id);
                }
            },
            OutputValue::On(_) => {
                let params = self.params.lock().unwrap();
                let velocity_index = id as usize % params.velocities.len();
                let velocity = params.velocities[velocity_index];
                let mod_value = params.x[velocity_index];
                let pressure_value = params.y[velocity_index];

                let channel = self.midi_channel + id as u8;
                let note_id = 36;

                // ensure velocity enabled
                self.midi_port.send_at(&[176 - 1 + channel, 91, 120], at).unwrap();

                // set mod
                self.midi_port.send_at(&[176 - 1 + channel, 1, mod_value], at).unwrap();
                self.midi_port.send_at(&[208 - 1 + channel, pressure_value], at).unwrap();

                // send note
                self.midi_port.send_at(&[144 - 1 + channel, note_id, velocity], at).unwrap();
                self.output_values.insert(id, (channel, note_id, velocity));
            }
        }
    }
}

pub struct BlofeldDrumParams {
    pub velocities: [u8; 4],
    pub x: [u8; 4],
    pub y: [u8; 4]
}
