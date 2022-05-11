use chunk::{MidiTime, OutputValue, Triggerable};
use midi_connection;
use serde::{Deserialize, Serialize};

use std::{
    collections::{HashMap, HashSet},
    sync::{atomic, Arc},
};

use super::SidechainOutput;

const TRIGGERS: [u8; 24] = [
    20, 21, 22, 23, 16, 17, 18, 19, 12, 13, 14, 15, 8, 9, 10, 11, 4, 5, 6, 7, 0, 1, 2, 3,
];

pub struct Sp404Mk2 {
    output: midi_connection::SharedMidiOutputConnection,
    _input: midi_connection::ThreadReference,
    mappings: HashMap<u32, (u8, u8)>,
    pending_cue: Arc<(atomic::AtomicU8, atomic::AtomicU8)>,
    selected: HashSet<u32>,
    output_values: HashMap<u32, (u8, u8, u8)>,
    velocity_map: Option<Vec<u8>>,
    sidechain_output: Option<SidechainOutput>,
    updating: bool,
}

impl Sp404Mk2 {
    pub fn new(
        port_name: &str,
        default_mapping: Vec<(u8, u8, u8)>,
        velocity_map: Option<Vec<u8>>,
        sidechain_output: Option<SidechainOutput>,
    ) -> Self {
        let pending_cue = Arc::new((atomic::AtomicU8::new(0), atomic::AtomicU8::new(0)));
        let pending_cue_input = pending_cue.clone();
        let mut mappings = HashMap::new();

        for (i, (bank, row, col)) in default_mapping.iter().enumerate() {
            let position = row * 4 + col;
            if let Some(trigger) = TRIGGERS.get(position as usize) {
                let note = trigger + 36;
                mappings.insert(i as u32, (bank + 1, note));
            }
        }

        let output = midi_connection::get_shared_output(port_name);
        let mut note_down = HashSet::new();

        let input = midi_connection::get_input(port_name, move |_stamp, message| {
            // detect triggered from CUE mode on SP-404 (in cue mode, only note off events are sent)
            if message[0] >= 144 && message[0] < 154 {
                let channel = message[0] - 144 + 1;
                note_down.insert((channel, message[1]));
            } else if message[0] >= 128 && message[0] < 144 {
                let channel = message[0] - 128 + 1;
                if channel <= 10 || channel == 16 {
                    if note_down.contains(&(channel, message[1])) {
                        note_down.remove(&(channel, message[1]));
                    } else {
                        pending_cue_input
                            .0
                            .store(channel, atomic::Ordering::Relaxed);
                        pending_cue_input
                            .1
                            .store(message[1], atomic::Ordering::Relaxed);
                    }
                }
            }
        });

        Sp404Mk2 {
            output,
            selected: HashSet::new(),
            mappings,
            output_values: HashMap::new(),
            pending_cue,
            velocity_map,
            sidechain_output,
            _input: input,
            updating: false,
        }
    }
}

impl Triggerable for Sp404Mk2 {
    fn on_tick(&mut self, _time: MidiTime) {
        // reset updating (stop light from flashing)
        self.updating = false;

        // detect cue triggers
        let channel = self.pending_cue.0.load(atomic::Ordering::Relaxed);
        let note = self.pending_cue.1.load(atomic::Ordering::Relaxed);
        if channel > 0 && note >= 36 {
            let trigger = note - 36;
            let position = TRIGGERS.iter().position(|v| v == &trigger).unwrap_or(24) as i32;
            let row = position / 4;
            let col = position % 4;

            let start_col = if col == 1 && self.selected.len() == 2 {
                0
            } else {
                let offset = self.selected.len() as i32;
                let overflow = (col + offset - 4).max(0);
                (col - overflow).max(0)
            };

            let start_position = (row * 4 + start_col) as usize;

            let mut selected: Vec<u32> = self.selected.iter().cloned().collect();
            selected.sort();

            for (i, id) in selected.iter().enumerate() {
                let position = start_position + i;
                let trigger = TRIGGERS.get(position).unwrap_or(&24);
                let note = trigger + 36;
                self.mappings.insert(*id, (channel, note));
                self.updating = true;
            }

            // clear them out for next press
            self.pending_cue.0.store(0, atomic::Ordering::Relaxed);
            self.pending_cue.1.store(0, atomic::Ordering::Relaxed);
        }
    }

    fn check_lit(&self, id: u32) -> bool {
        if self.updating && self.selected.contains(&id) {
            false
        } else {
            self.mappings.contains_key(&id)
        }
    }

    fn select(&mut self, id: u32, selected: bool) {
        if selected {
            self.selected.insert(id);
        } else {
            self.selected.remove(&id);
        }
    }

    fn trigger(&mut self, id: u32, value: OutputValue) {
        match value {
            OutputValue::Off => {
                if self.output_values.contains_key(&id) {
                    let (channel, note, _velocity) = self.output_values.get(&id).unwrap().clone();

                    self.output.send(&[128 + channel - 1, note, 0]).unwrap();
                    self.output_values.remove(&id);
                }
            }
            OutputValue::On(velocity) => {
                let velocity = ::devices::map_velocity(&self.velocity_map, velocity);
                if let Some((channel, note)) = self.mappings.get(&id) {
                    self.output
                        .send(&[144 + channel - 1, *note, velocity])
                        .unwrap();

                    // send sync if kick
                    if let Some(sidechain_output) = &mut self.sidechain_output {
                        if id == sidechain_output.id {
                            let mut params = sidechain_output.params.lock().unwrap();
                            params.duck_triggered = true;
                        }
                    }

                    self.output_values.insert(id, (*channel, *note, velocity));
                }
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
