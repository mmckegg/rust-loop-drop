use ::chunk::{Triggerable, OutputValue, SystemTime, MidiTime};
use ::midi_connection;
use std::sync::{Arc, Mutex};

use std::collections::HashMap;

const DRUMS: [u8; 8] = [36, 38, 39, 37, 50, 41, 43, 52];

pub struct BlofeldDrums {
    midi_port: midi_connection::SharedMidiOutputConnection,
    midi_channel: u8,
    sync_port: midi_connection::SharedMidiOutputConnection,
    sync_channel: u8,
    last_pos: MidiTime,
    velocities: Arc<Mutex<HashMap<u32, u8>>>,
    output_values: HashMap<u32, (u8, u8, u8)>
}

impl BlofeldDrums {
    pub fn new (midi_port: midi_connection::SharedMidiOutputConnection, channel: u8, sync_port: midi_connection::SharedMidiOutputConnection, sync_channel: u8, velocities: Arc<Mutex<HashMap<u32, u8>>>) -> Self {
        BlofeldDrums {
            midi_port,
            sync_port,
            last_pos: MidiTime::zero(),
            sync_channel,
            velocities,
            midi_channel: channel,
            output_values: HashMap::new()
        }
    }
}

impl Triggerable for BlofeldDrums {
    fn on_tick (&mut self, time: MidiTime) {
        self.last_pos = time;
    }

    fn trigger (&mut self, id: u32, value: OutputValue, at: SystemTime) {
        match value {
            OutputValue::Off => {
                if self.output_values.contains_key(&id) {
                    let (channel, note_id, _) = *self.output_values.get(&id).unwrap();
                    // HACK: disable off notes because this is doing weird things to blofeld for drum envelopes
                    self.midi_port.send(&[128 - 1 + channel, note_id, 0]).unwrap();
                    self.output_values.remove(&id);

                    if id == 0 {
                        self.sync_port.send_at(&[128 - 1 + self.sync_channel, 24, 0], at).unwrap();
                    }
                }
            },
            OutputValue::On(_) => {
                let velocities = self.velocities.lock().unwrap();
                let base_velocity = 100;
                let velocity_pos = self.last_pos.ticks() / MidiTime::from_measure(1, 4).ticks() % 8;
                let pos = self.last_pos % MidiTime::from_measure(1, 4);
                let mut velocity = if pos.ticks() == 0 {
                    *velocities.get(&(velocity_pos as u32)).unwrap_or(&base_velocity)
                } else {
                    base_velocity
                };

                // let mod_value = params.x[velocity_index];
                // let pressure_value = params.y[velocity_index];

                let mut channel = self.midi_channel; // + id as u8;

                let note_id = DRUMS[id as usize % DRUMS.len()];

                // send note off (choke previous drum)
                // self.midi_port.send(&[128 - 1 + channel, note_id, 0]).unwrap();

                // send note
                self.midi_port.send_at(&[144 - 1 + channel, note_id, velocity], at).unwrap();

                // send sync if kick
                if id == 0 {
                    self.sync_port.send_at(&[144 - 1 + self.sync_channel, 24, velocity], at).unwrap();
                }

                self.output_values.insert(id, (channel, note_id, velocity));
            }
        }
    }
}

pub struct BlofeldDrumParams {
    pub velocities: [u8; 8],
    pub x: [u8; 8],
    // pub y: [u8; 4]
}
