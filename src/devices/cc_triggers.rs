use chunk::{MidiTime, OutputValue, Triggerable};
use midi_connection;
use serde::{Deserialize, Serialize};

use std::collections::HashMap;

pub struct CcTriggers {
    midi_port: midi_connection::SharedMidiOutputConnection,
    last_pos: MidiTime,
    output_values: HashMap<u32, MidiTrigger>,
    velocity_map: Option<Vec<u8>>,
    triggers: Vec<MidiTrigger>,
}

impl CcTriggers {
    pub fn new(
        midi_port: midi_connection::SharedMidiOutputConnection,
        triggers: Vec<MidiTrigger>,
        velocity_map: Option<Vec<u8>>,
    ) -> Self {
        CcTriggers {
            midi_port,
            last_pos: MidiTime::zero(),
            output_values: HashMap::new(),
            velocity_map,
            triggers,
        }
    }

    fn trigger_on(&mut self, trigger: &MidiTrigger, velocity: u8) {
        let velocity = ::devices::map_velocity(&self.velocity_map, velocity);

        match trigger {
            MidiTrigger::Cc(channel, cc, value) => {
                self.midi_port
                    .send(&[176 - 1 + channel, *cc, *value])
                    .unwrap();
            }
            MidiTrigger::Note(channel, note, velocity) => {
                self.midi_port
                    .send(&[144 - 1 + channel, *note, *velocity])
                    .unwrap();
            }
            MidiTrigger::CcVelocity(channel, cc) => {
                self.midi_port
                    .send(&[176 - 1 + channel, *cc, velocity])
                    .unwrap();
            }
            MidiTrigger::NoteVelocity(channel, note) => {
                self.midi_port
                    .send(&[144 - 1 + channel, *note, velocity])
                    .unwrap();
            }
            MidiTrigger::Multi(triggers) => {
                for trigger in triggers {
                    self.trigger_on(trigger, velocity)
                }
            }
            MidiTrigger::None => {}
        }
    }

    fn trigger_off(&mut self, trigger: &MidiTrigger) {
        match trigger {
            MidiTrigger::Cc(channel, cc, _value) => {
                self.midi_port.send(&[176 - 1 + channel, *cc, 0]).unwrap();
            }
            MidiTrigger::Note(channel, note, _velocity) => {
                self.midi_port.send(&[144 - 1 + channel, *note, 0]).unwrap();
            }
            MidiTrigger::CcVelocity(channel, cc) => {
                self.midi_port.send(&[176 - 1 + channel, *cc, 0]).unwrap();
            }
            MidiTrigger::NoteVelocity(channel, note) => {
                self.midi_port.send(&[144 - 1 + channel, *note, 0]).unwrap();
            }
            MidiTrigger::Multi(triggers) => {
                for trigger in triggers {
                    self.trigger_off(trigger)
                }
            }
            MidiTrigger::None => {}
        }
    }
}

impl Triggerable for CcTriggers {
    fn on_tick(&mut self, time: MidiTime) {
        self.last_pos = time;
    }

    fn trigger(&mut self, id: u32, value: OutputValue) {
        match value {
            OutputValue::Off => {
                if self.output_values.contains_key(&id) {
                    let trigger = self.output_values.get(&id).unwrap().clone();
                    self.trigger_off(&trigger);
                    self.output_values.remove(&id);
                }
            }
            OutputValue::On(velocity) => {
                let trigger = self.triggers[id as usize % self.triggers.len()].clone();
                self.trigger_on(&trigger, velocity);
                self.output_values.insert(id, trigger);
            }
        }
    }
}
#[derive(Serialize, Deserialize, PartialEq, Clone)]
pub enum MidiTrigger {
    Cc(u8, u8, u8),
    CcVelocity(u8, u8),
    Note(u8, u8, u8),
    NoteVelocity(u8, u8),
    Multi(Vec<MidiTrigger>),
    None,
}
