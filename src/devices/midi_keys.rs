use chunk::{MidiTime, OutputValue, Triggerable};
use midi_connection;
use std::collections::{HashMap, HashSet};
use std::iter::FromIterator;
use std::sync::{Arc, Mutex};

pub use midi_connection::SharedMidiOutputConnection;
pub use scale::{Offset, Scale};

pub struct MidiKeys {
    pub midi_port: midi_connection::SharedMidiOutputConnection,
    midi_channel: u8,
    output_values: HashMap<u32, (u8, u8)>,
    scale: Arc<Mutex<Scale>>,
    offset: Arc<Mutex<Offset>>,
    midi_offset: i8,
    velocity_map: Option<Vec<u8>>,
    octave_offset: i32,
    offset_wrap: bool,
    trigger_stack: Vec<u32>,
    last_velocity: u8,
    monophonic: bool,
}

impl MidiKeys {
    pub fn new(
        midi_port: midi_connection::SharedMidiOutputConnection,
        midi_channel: u8,
        scale: Arc<Mutex<Scale>>,
        offset: Arc<Mutex<Offset>>,
        octave_offset: i32,
        velocity_map: Option<Vec<u8>>,
        offset_wrap: bool,
        monophonic: bool,
        midi_offset: i8,
    ) -> Self {
        MidiKeys {
            midi_port,
            midi_channel,
            velocity_map,
            output_values: HashMap::new(),
            offset,
            octave_offset,
            scale,
            offset_wrap,
            last_velocity: 127,
            trigger_stack: Vec::new(),
            monophonic,
            midi_offset,
        }
    }

    fn send_on(&mut self, note_id: u8, velocity: u8) {
        self.midi_port
            .send(&[144 + self.midi_channel - 1, (note_id as i8 + self.midi_offset).max(0).min(127) as u8, velocity])
            .unwrap();
    }

    fn send_off(&mut self, note_id: u8) {
        self.midi_port
            .send(&[128 + self.midi_channel - 1, (note_id as i8 + self.midi_offset).max(0).min(127) as u8, 0])
            .unwrap();
    }

    fn trigger_off(&mut self, id: u32) {
        if self.output_values.contains_key(&id) {
            let (note_id, _) = *self.output_values.get(&id).unwrap();
            self.send_off(note_id);
            self.output_values.remove(&id);
        }
    }

    fn trigger_on(&mut self, id: u32, velocity: u8) {
        if self.output_values.contains_key(&id) {
            return;
        }

        let note_id = get_note_id(
            id,
            &self.scale,
            &self.offset,
            self.octave_offset,
            self.offset_wrap,
        );

        self.send_on(note_id, velocity);
        self.output_values.insert(id, (note_id, velocity));
    }

    fn refresh_scale(&mut self) {
        let mut note_ids = HashSet::new();
        let mut next_output_values = HashMap::new();

        for (id, (note_id, velocity)) in &self.output_values {
            let new_note_id = get_note_id(
                *id,
                &self.scale,
                &self.offset,
                self.octave_offset,
                self.offset_wrap,
            );
            next_output_values.insert(*id, (new_note_id, *velocity));

            if velocity > &0 && note_id != &new_note_id {
                note_ids.insert(*note_id);
                note_ids.insert(new_note_id);
            }
        }

        // find the difference between the current notes and the new ones
        // we do this as two different lists so that we can update the still held notes and then
        // remove the off notes last so that legato works nicely
        let mut off_notes = HashSet::new();
        let mut changed_notes = HashMap::new();

        for note_id in note_ids {
            let old_value = self
                .output_values
                .values()
                .find(|(id, old_velocity)| id == &note_id && old_velocity > &0);
            let new_value = next_output_values
                .values()
                .find(|(id, new_velocity)| id == &note_id && new_velocity > &0);

            if old_value != new_value {
                if let Some(new_value) = new_value {
                    changed_notes.insert(note_id, new_value.1);
                } else {
                    off_notes.insert(note_id);
                }
            }
        }

        for (note_id, velocity) in changed_notes {
            self.send_on(note_id, velocity);
        }

        for note_id in off_notes {
            self.send_off(note_id);
        }

        self.output_values = next_output_values;
    }
}

fn get_note_id(
    id: u32,
    scale: &Arc<Mutex<Scale>>,
    offset: &Arc<Mutex<Offset>>,
    octave_offset: i32,
    offset_wrap: bool,
) -> u8 {
    let scale = scale.lock().unwrap();
    let offset = offset.lock().unwrap();
    let mut scale_offset = offset.base + offset.offset;

    let col = (id % 8) as i32;

    if offset_wrap {
        // hacky chord inversions
        if offset.offset > -7 && offset.offset < 7 {
            if col + offset.offset > 8 {
                scale_offset -= 7
            } else if col + offset.offset < 0 {
                scale_offset += 7
            }
        }
    }

    (scale.get_note_at((id as i32) + scale_offset) + offset.pitch + (octave_offset * 12)) as u8
}

impl Triggerable for MidiKeys {
    fn trigger(&mut self, id: u32, value: OutputValue) {
        match value {
            OutputValue::Off => {
                if self.monophonic {
                    self.trigger_stack.retain(|&x| x != id);
                    let top = self.trigger_stack.last().cloned();
                    if let Some(top) = top {
                        self.trigger_on(top, self.last_velocity);
                    }
                }

                self.trigger_off(id);
            }
            OutputValue::On(velocity) => {
                if self.trigger_stack.contains(&id) {
                    return;
                }

                let velocity = ::devices::map_velocity(&self.velocity_map, velocity);
                self.last_velocity = velocity;
                self.trigger_on(id, velocity);

                if self.monophonic {
                    let top = self.trigger_stack.last().cloned();
                    if let Some(top) = top {
                        self.trigger_off(top);
                    }
                    self.trigger_stack.push(id);
                }
            }
        }
    }

    fn get_notes(&self) -> Option<HashSet<u8>> {
        Some(HashSet::from_iter(self.output_values.values().filter_map(
            |(note, velocity)| if velocity > &0 { Some(*note) } else { None },
        )))
    }

    fn on_tick(&mut self, _: MidiTime) {
        self.refresh_scale();
    }

    fn check_lit(&self, id: u32) -> bool {
        let scale = self.scale.lock().unwrap();
        let offset = self.offset.lock().unwrap();
        let scale_offset = offset.base + offset.offset;
        scale.get_pentatonic_at((id as i32) + scale_offset)
    }
}
