use ::chunk::{Triggerable, OutputValue, MidiTime};
use ::midi_connection;
use std::sync::{Arc, Mutex};

use std::collections::HashMap;

pub struct MidiTriggers {
    midi_port: midi_connection::SharedMidiOutputConnection,
    midi_channel: u8,
    sidechain_output: Option<SidechainOutput>,
    last_pos: MidiTime,
    output_values: HashMap<u32, (u8, u8, u8)>,
    trigger_ids: Vec<u8>
}

pub struct SidechainOutput {
    pub midi_port: midi_connection::SharedMidiOutputConnection,
    pub midi_channel: u8,
    pub trigger_id: u8,
    pub id: u32
}

impl MidiTriggers {
    pub fn new (midi_port: midi_connection::SharedMidiOutputConnection, channel: u8, sidechain_output: Option<SidechainOutput>, trigger_ids: Vec<u8>) -> Self {
        MidiTriggers {
            midi_port,
            last_pos: MidiTime::zero(),
            sidechain_output,
            midi_channel: channel,
            output_values: HashMap::new(),
            trigger_ids
        }
    }
}

impl Triggerable for MidiTriggers {
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

                    if let Some(sidechain_output) = &mut self.sidechain_output {
                        if id == sidechain_output.id {
                            sidechain_output.midi_port.send(&[128 - 1 + sidechain_output.midi_channel, sidechain_output.trigger_id, 0]).unwrap();
                        }
                    }
                }
            },
            OutputValue::On(velocity) => {
                let channel = self.midi_channel;
                let note_id = self.trigger_ids[id as usize % self.trigger_ids.len()];

                // send note
                self.midi_port.send(&[144 - 1 + channel, note_id, velocity]).unwrap();           

                // send sync if kick
                if let Some(sidechain_output) = &mut self.sidechain_output {
                    if id == sidechain_output.id {
                        sidechain_output.midi_port.send(&[144 - 1 + sidechain_output.midi_channel, sidechain_output.trigger_id, 0]).unwrap();
                    }
                }                

                self.output_values.insert(id, (channel, note_id, velocity));
            }
        }
    }
}
