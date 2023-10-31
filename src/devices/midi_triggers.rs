use chunk::{MidiTime, OutputValue, Triggerable};
use midi_connection;

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use crate::loop_grid_launchpad::LoopGridParams;

pub struct MidiTriggers {
    midi_port: midi_connection::SharedMidiOutputConnection,
    midi_channel: u8,
    sidechain_output: Option<SidechainOutput>,
    last_pos: MidiTime,
    velocity_map: Option<Vec<u8>>,
    output_values: HashMap<u32, (u8, u8, u8)>,
    trigger_ids: Vec<u8>,
}

pub struct SidechainOutput {
    pub params: Arc<Mutex<LoopGridParams>>,
    pub id: u32,
}

impl MidiTriggers {
    pub fn new(
        midi_port: midi_connection::SharedMidiOutputConnection,
        channel: u8,
        sidechain_output: Option<SidechainOutput>,
        trigger_ids: Vec<u8>,
        velocity_map: Option<Vec<u8>>,
    ) -> Self {
        MidiTriggers {
            midi_port,
            last_pos: MidiTime::zero(),
            sidechain_output,
            midi_channel: channel,
            output_values: HashMap::new(),
            velocity_map,
            trigger_ids,
        }
    }
}

impl Triggerable for MidiTriggers {
    fn on_tick(&mut self, time: MidiTime) {
        self.last_pos = time;
    }

    fn trigger(&mut self, id: u32, value: OutputValue) {
        match value {
            OutputValue::Off => {
                if self.output_values.contains_key(&id) {
                    let (channel, note_id, _) = *self.output_values.get(&id).unwrap();
                    self.midi_port
                        .send(&[144 - 1 + channel, note_id, 0])
                        .unwrap();
                    self.output_values.remove(&id);
                }
            }
            OutputValue::On(velocity) => {
                let channel = self.midi_channel;
                let note_id = self.trigger_ids[id as usize % self.trigger_ids.len()];
                let mapped_velocity = ::devices::map_velocity(&self.velocity_map, velocity);

                // send note
                self.midi_port
                    .send(&[144 - 1 + channel, note_id, mapped_velocity])
                    .unwrap();

                // send sync if kick
                if let Some(sidechain_output) = &mut self.sidechain_output {
                    if id == sidechain_output.id {
                        let mut params = sidechain_output.params.lock().unwrap();
                        params.duck_triggered = true;
                    }
                }

                self.output_values
                    .insert(id, (channel, note_id, mapped_velocity));
            }
        }
    }
}
