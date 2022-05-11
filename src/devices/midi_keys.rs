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
    velocity_map: Option<Vec<u8>>,
    octave_offset: i32,
    offset_wrap: bool,
    current_tick: i32,
    note_on_tick: HashMap<u8, i32>,
    off_next_tick: HashSet<u8>,
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
    ) -> Self {
        MidiKeys {
            midi_port,
            midi_channel,
            velocity_map,
            output_values: HashMap::new(),
            note_on_tick: HashMap::new(),
            offset,
            octave_offset,
            scale,
            offset_wrap,
            current_tick: 0,
            last_velocity: 127,
            off_next_tick: HashSet::new(),
            trigger_stack: Vec::new(),
            monophonic,
        }
    }

    pub fn scale(&self) -> std::sync::MutexGuard<'_, Scale> {
        self.scale.lock().unwrap()
    }

    fn send_on(&mut self, note_id: u8, velocity: u8) {
        self.off_next_tick.remove(&note_id);
        self.note_on_tick.insert(note_id, self.current_tick);
        self.midi_port
            .send(&[144 + self.midi_channel - 1, note_id, velocity])
            .unwrap();
    }

    fn send_off_now(&mut self, note_id: u8) {
        self.off_next_tick.remove(&note_id);
        self.note_on_tick.remove(&note_id);

        self.midi_port
            .send(&[128 + self.midi_channel - 1, note_id, 0])
            .unwrap();
    }

    fn send_off(&mut self, note_id: u8) {
        // ensure that we never send an off note immediately after an on note, should wait at least one midi tick
        if let Some(tick) = self.note_on_tick.get(&note_id) {
            if tick == &self.current_tick {
                self.off_next_tick.insert(note_id);
            } else {
                self.send_off_now(note_id);
            }
        }
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

        self.trigger_stack.push(id);

        let note_id = get_note_id(
            id,
            &self.scale,
            &self.offset,
            self.octave_offset,
            self.offset_wrap,
        );

        // remove any pending off note since we now have an on note to replace
        self.off_next_tick.remove(&note_id);

        self.send_on(note_id, velocity);
        self.output_values.insert(id, (note_id, velocity));
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
                self.trigger_stack.retain(|&x| x != id);

                if !self.monophonic {
                    self.trigger_off(id);
                }
            }
            OutputValue::On(velocity) => {
                if self.trigger_stack.contains(&id) {
                    return;
                }
                let velocity = ::devices::map_velocity(&self.velocity_map, velocity);
                self.last_velocity = velocity;
                self.trigger_on(id, velocity);
                self.trigger_stack.push(id);
            }
        }
    }

    fn get_notes(&self) -> Option<HashSet<u8>> {
        Some(HashSet::from_iter(self.output_values.values().filter_map(
            |(note, velocity)| if velocity > &0 { Some(*note) } else { None },
        )))
    }

    fn on_tick(&mut self, pos: MidiTime) {
        // send off notes scheduled from previous tick
        for note_id in self.off_next_tick.clone() {
            self.send_off_now(note_id)
        }

        if self.monophonic {
            let top = if let Some(top) = self.trigger_stack.last().cloned() {
                self.trigger_on(top, self.last_velocity);
                Some(top)
            } else {
                None
            };

            let ids: Vec<u32> = self.output_values.keys().cloned().collect();
            for id in ids {
                if Some(id) != top {
                    self.trigger_off(id);
                }
            }
        }

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
        self.current_tick = pos.ticks();
    }

    fn check_lit(&self, id: u32) -> bool {
        let scale = self.scale.lock().unwrap();
        let offset = self.offset.lock().unwrap();
        let scale_offset = offset.base + offset.offset;
        scale.get_pentatonic_at((id as i32) + scale_offset)
    }
}
