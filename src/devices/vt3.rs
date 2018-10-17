use ::midi_connection;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::thread;
use ::chunk::{Triggerable, OutputValue, SystemTime, ScheduleMode};
use std::time::Duration;

pub use ::scale::{Scale, Offset};
pub use ::midi_connection::SharedMidiOutputConnection;

pub struct VT3 {
    midi_output: midi_connection::SharedMidiOutputConnection,
    output_values: HashMap<u32, (u8, u8)>,
    scale: Arc<Mutex<Scale>>,
    offset: Arc<Mutex<Offset>>,
    triggering: HashSet<u32>,
    trigger_stack: Vec<(u32, u8, u8)>
}

impl VT3 {
    pub fn new (midi_output: midi_connection::SharedMidiOutputConnection, scale: Arc<Mutex<Scale>>, offset: Arc<Mutex<Offset>>) -> Self {
        VT3 {
            midi_output,
            output_values: HashMap::new(),
            offset,
            triggering: HashSet::new(),
            trigger_stack: Vec::new(),
            scale
        }
    }
}

fn get_note_id (id: u32, scale: &Arc<Mutex<Scale>>, offset: &Arc<Mutex<Offset>>) -> u8 {
    let scale = scale.lock().unwrap();
    let offset = offset.lock().unwrap();
    let scale_offset = offset.base + offset.offset;
    (scale.get_note_at((id as i32) + scale_offset) + offset.pitch + (offset.oct * 12)) as u8
}

impl Triggerable for VT3 {
    fn trigger (&mut self, id: u32, value: OutputValue, time: SystemTime) {
        let velocity = value.value();

        if self.triggering.len() == 0 {
            // enable robot mode and then sleep for 1 ms (to ensure message is handled)
            self.midi_output.send(&[176, 17, 127]).unwrap();
            thread::sleep(Duration::from_millis(1));
        }

        // monophonic midi output using trigger stack!
        if velocity > 0 {
            let note_id = get_note_id(id, &self.scale, &self.offset);
            if let Some((_, last_note_id, last_velocity)) = self.trigger_stack.last() {
                self.midi_output.send(&[128, *last_note_id, *last_velocity]).unwrap();
            }
            self.trigger_stack.push((id, note_id, velocity));
            self.midi_output.send(&[144, note_id as u8, velocity]).unwrap();
            self.triggering.insert(id);
        } else {
            let mut should_update = false;
            if let Some((last_id, last_note_id, _)) = self.trigger_stack.last() {
                if *last_id == id {
                    self.midi_output.send(&[128, *last_note_id, 0]).unwrap();
                    should_update = true;
                }
            }
            self.trigger_stack.retain(|(item_id, _, _)| *item_id != id);
            self.triggering.remove(&id);

            if should_update {
                if let Some((_, last_note_id, last_vel)) = self.trigger_stack.last() {
                    self.midi_output.send(&[144, *last_note_id as u8, *last_vel]).unwrap();
                }
            }
        }

        if self.triggering.len() == 0 {
            self.midi_output.send(&[176, 17, 0]).unwrap();
        }
    }

    fn on_tick (&mut self) {
        if let Some((last_id, last_note_id, last_velocity)) = self.trigger_stack.last_mut() {
            let new_note_id = get_note_id(*last_id, &self.scale, &self.offset);
            if last_note_id != &new_note_id {
                self.midi_output.send(&[144, new_note_id, *last_velocity]).unwrap();
                self.midi_output.send(&[128, *last_note_id, 0]).unwrap();
                *last_note_id = new_note_id;
            }
        }

        if self.trigger_stack.len() > 1 {
            for (last_id, last_note_id, _) in &mut self.trigger_stack {
                let new_note_id = get_note_id(*last_id, &self.scale, &self.offset);
                if last_note_id != &new_note_id {
                    *last_note_id = new_note_id;
                }
            }
        }
    }

    fn schedule_mode (&self) -> ScheduleMode {
        ScheduleMode::Monophonic
    }
}