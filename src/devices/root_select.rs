
use ::indexmap::IndexSet;

use std::sync::{Arc, Mutex};
use ::chunk::{Triggerable, OutputValue, ScheduleMode, LatchMode, MidiTime};
use std::collections::HashSet;
use ::controllers::Modulator;

pub use ::scale::{Scale, Offset};

pub struct RootSelect {
    stack: IndexSet<u32>,
    scale: Arc<Mutex<Scale>>,
    modulators: Vec<Option<Modulator>>
}

impl RootSelect {
    pub fn new (scale: Arc<Mutex<Scale>>, modulators: Vec<Option<Modulator>>) -> Self {
        RootSelect { 
            scale, 
            modulators,
            stack: IndexSet::new() 
        }
    }

    fn refresh_output (&mut self) {
        if let Some(id) = self.stack.last().cloned() {
            let mut current_scale = self.scale.lock().unwrap();
            current_scale.root = 52 + (id as i32);

            for modulator in &mut self.modulators {
                let pitch_mod = (id as f64 - 8.0)  / 12.0;
                if let Some(modulator) = modulator {
                    modulator.send_polar(pitch_mod);
                }
            }
        }
    }
}

impl Triggerable for RootSelect {
    fn trigger (&mut self, id: u32, value: OutputValue) {
        match value {
            OutputValue::Off => {
                self.stack.shift_remove(&id);
                self.refresh_output();
            },
            OutputValue::On(_velocity) => {
                self.stack.insert(id);
                self.refresh_output();
            }
        }
    }

    fn on_tick (&mut self, time: MidiTime) {
        if time.is_whole_beat() {
            self.refresh_output();
        }
    }

    fn get_active (&self) -> Option<HashSet<u32>> {
        let current_scale = self.scale.lock().unwrap();

        let mut result = HashSet::new();
        if current_scale.root >= 52 {
            result.insert(current_scale.root as u32 - 52);
        }

        Some(result)
    }

    fn latch_mode (&self) -> LatchMode { LatchMode::NoSuppress }
    fn schedule_mode (&self) -> ScheduleMode { ScheduleMode::Monophonic }
}