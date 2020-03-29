use ::midi_connection;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use ::chunk::{Triggerable, OutputValue, SystemTime, MidiTime};

pub use ::scale::{Scale, Offset};
pub use ::midi_connection::SharedMidiOutputConnection;

pub struct MidiKeys {
    pub midi_outputs: Vec<midi_connection::SharedMidiOutputConnection>,
    pub midi_channel: u8,
    output_values: HashMap<u32, (u8, u8)>,
    scale: Arc<Mutex<Scale>>,
    offset: Arc<Mutex<Offset>>
}

impl MidiKeys {
    pub fn new (midi_outputs: Vec<midi_connection::SharedMidiOutputConnection>, channel: u8, scale: Arc<Mutex<Scale>>, offset: Arc<Mutex<Offset>>) -> Self {
        MidiKeys {
            midi_outputs,
            midi_channel: channel,
            output_values: HashMap::new(),
            offset,
            scale
        }
    }

    pub fn scale (&self) -> std::sync::MutexGuard<'_, Scale, > {
        self.scale.lock().unwrap()
    }
}

fn get_note_id (id: u32, scale: &Arc<Mutex<Scale>>, offset: &Arc<Mutex<Offset>>) -> u8 {
    let scale = scale.lock().unwrap();
    let offset = offset.lock().unwrap();
    let mut scale_offset = offset.base + offset.offset;

    let col = (id % 8) as i32;

    // hacky chord inversions
    if offset.offset > -7 && offset.offset < 7 {
        if col + offset.offset > 8 {
            scale_offset -= 7
        } else if col + offset.offset < 0 {
            scale_offset += 7
        }
    }

    (scale.get_note_at((id as i32) + scale_offset) + offset.pitch + (offset.oct * 12)) as u8
}

impl Triggerable for MidiKeys {
    fn trigger (&mut self, id: u32, value: OutputValue, time: SystemTime) {
        match value {
            OutputValue::Off => {
                if self.output_values.contains_key(&id) {
                    let (note_id, _) = *self.output_values.get(&id).unwrap();
                    for midi_output in &mut self.midi_outputs {
                        midi_output.send_at(&[144 + self.midi_channel - 1, note_id, 0], time).unwrap();
                    }
                    self.output_values.remove(&id);
                }
            },
            OutputValue::On(velocity) => {
                let note_id = get_note_id(id, &self.scale, &self.offset);
                for midi_output in &mut self.midi_outputs {
                    midi_output.send_at(&[144 + self.midi_channel - 1, note_id, velocity], time).unwrap();
                }
                self.output_values.insert(id, (note_id, velocity));
            }
        }
    }

    fn on_tick (&mut self, _: MidiTime) {
        let mut to_update = HashMap::new();
        for (id, (note_id, velocity)) in &self.output_values {
            let new_note_id = get_note_id(*id, &self.scale, &self.offset);
            if note_id != &new_note_id {
                for midi_output in &mut self.midi_outputs {
                    midi_output.send(&[144 + self.midi_channel - 1, new_note_id, *velocity]).unwrap();
                    midi_output.send(&[144 + self.midi_channel - 1, *note_id, 0]).unwrap();
                }
                to_update.insert(id.clone(), (new_note_id, velocity.clone()));
            }
        }

        for (id, item) in to_update {
            self.output_values.insert(id, item);
        }
    }
}