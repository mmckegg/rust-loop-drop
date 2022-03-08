use chunk::{MidiTime, OutputValue, Triggerable};
use midi_connection;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

pub use midi_connection::SharedMidiOutputConnection;
pub use scale::{Offset, Scale};

pub struct MonoMidiKeys {
    pub midi_port: midi_connection::SharedMidiOutputConnection,
    midi_channel: u8,
    output_values: HashMap<u32, (u8, u8)>,
    output_stack: Vec<u32>,
    scale: Arc<Mutex<Scale>>,
    offset: Arc<Mutex<Offset>>,
    velocity_map: Option<Vec<u8>>,
    octave_offset: i32,
}

impl MonoMidiKeys {
    pub fn new(
        midi_port: midi_connection::SharedMidiOutputConnection,
        midi_channel: u8,
        scale: Arc<Mutex<Scale>>,
        offset: Arc<Mutex<Offset>>,
        octave_offset: i32,
        velocity_map: Option<Vec<u8>>,
    ) -> Self {
        MonoMidiKeys {
            midi_port,
            midi_channel,
            velocity_map,
            output_values: HashMap::new(),
            output_stack: Vec::new(),
            offset,
            octave_offset,
            scale,
        }
    }

    pub fn scale(&self) -> std::sync::MutexGuard<'_, Scale> {
        self.scale.lock().unwrap()
    }
}

fn get_note_id(
    id: u32,
    scale: &Arc<Mutex<Scale>>,
    offset: &Arc<Mutex<Offset>>,
    octave_offset: i32,
) -> u8 {
    let scale = scale.lock().unwrap();
    let offset = offset.lock().unwrap();
    let scale_offset = offset.base + offset.offset;
    (scale.get_note_at((id as i32) + scale_offset) + offset.pitch + (octave_offset * 12)) as u8
}

impl Triggerable for MonoMidiKeys {
    fn trigger(&mut self, id: u32, value: OutputValue) {
        match value {
            OutputValue::Off => {
                if self.output_values.contains_key(&id) {
                    let (note_id, _) = *self.output_values.get(&id).unwrap();

                    if self.output_stack.last() == Some(&id) {
                        self.output_stack.pop();

                        if let Some(last) = self.output_stack.last() {
                            if let Some(last_note) = self.output_values.get(last) {
                                self.midi_port
                                    .send(&[144 + self.midi_channel - 1, last_note.0, last_note.1])
                                    .unwrap();
                            }
                        }

                        self.midi_port
                            .send(&[128 + self.midi_channel - 1, note_id, 0])
                            .unwrap();
                    }

                    self.output_values.remove(&id);
                }
                self.output_stack.retain(|&x| x != id);
            }
            OutputValue::On(velocity) => {
                let note_id = get_note_id(id, &self.scale, &self.offset, self.octave_offset);
                let velocity = ::devices::map_velocity(&self.velocity_map, velocity);

                self.midi_port
                    .send(&[144 + self.midi_channel - 1, note_id, velocity])
                    .unwrap();
                self.output_values.insert(id, (note_id, velocity));

                if let Some(last) = self.output_stack.last() {
                    if let Some(last_note) = self.output_values.get(last) {
                        self.midi_port
                            .send(&[128 + self.midi_channel - 1, last_note.0, 0])
                            .unwrap();
                    }
                }

                self.output_stack.push(id);
            }
        }
    }

    fn on_tick(&mut self, _: MidiTime) {
        if let Some(id) = self.output_stack.last() {
            if let Some((note_id, velocity)) = self.output_values.get(id).cloned() {
                let new_note_id = get_note_id(*id, &self.scale, &self.offset, self.octave_offset);
                if note_id != new_note_id {
                    self.midi_port
                        .send(&[144 + self.midi_channel - 1, new_note_id, velocity])
                        .unwrap();

                    self.midi_port
                        .send(&[144 + self.midi_channel - 1, note_id, 0])
                        .unwrap();

                    self.output_values.insert(*id, (new_note_id, velocity));
                }
            }
        }
    }
}
