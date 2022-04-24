use indexmap::IndexSet;

use chunk::{LatchMode, MidiTime, OutputValue, ScheduleMode, Triggerable};
use controllers::Modulator;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

pub use scale::{Offset, Scale};

pub struct RootSelect {
    stack: IndexSet<u32>,
    scale: Arc<Mutex<Scale>>,
    modulators: Vec<Modulator>,
}

impl RootSelect {
    pub fn new(scale: Arc<Mutex<Scale>>, modulators: Vec<Modulator>) -> Self {
        RootSelect {
            scale,
            modulators,
            stack: IndexSet::new(),
        }
    }

    fn refresh_output(&mut self) {
        if let Some(id) = self.stack.last().cloned() {
            let mut current_scale = self.scale.lock().unwrap();
            current_scale.root = 52 + (id as i32);

            for modulator in &mut self.modulators {
                let pitch_mod = (id as f64 - 8.0) / 12.0;
                if let Modulator::MidiModulator(modulator) = modulator {
                    modulator.send_polar(pitch_mod);
                }
            }
        }
    }
}

impl Triggerable for RootSelect {
    fn trigger(&mut self, id: u32, value: OutputValue) {
        match value {
            OutputValue::Off => {
                self.stack.shift_remove(&id);
                self.refresh_output();
            }
            OutputValue::On(_velocity) => {
                self.stack.insert(id);
                self.refresh_output();
            }
        }
    }

    fn on_tick(&mut self, time: MidiTime) {
        if time.is_whole_beat() {
            self.refresh_output();
        }
    }

    fn check_triggering(&self, id: u32) -> Option<bool> {
        let current_scale = self.scale.lock().unwrap();
        if current_scale.root >= 52 {
            Some(current_scale.root as u32 - 52 == id)
        } else {
            None
        }
    }

    fn schedule_mode(&self) -> ScheduleMode {
        ScheduleMode::Monophonic
    }
}
