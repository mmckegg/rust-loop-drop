use indexmap::IndexSet;

use chunk::{LatchMode, OutputValue, ScheduleMode, Triggerable};
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

pub use scale::{Offset, Scale};

pub struct ScaleSelect {
    scale: Arc<Mutex<Scale>>,
    stack: IndexSet<u32>,
}

impl ScaleSelect {
    pub fn new(scale: Arc<Mutex<Scale>>) -> Self {
        ScaleSelect {
            scale,
            stack: IndexSet::new(),
        }
    }

    fn refresh_output(&mut self) {
        if let Some(id) = self.stack.last().cloned() {
            let mut current_scale = self.scale.lock().unwrap();
            current_scale.scale = id as i32;
        }
    }
}

impl Triggerable for ScaleSelect {
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

    fn get_active(&self) -> Option<HashSet<u32>> {
        let current_scale = self.scale.lock().unwrap();

        let mut result = HashSet::new();
        if current_scale.scale >= 0 {
            result.insert(current_scale.scale as u32);
        }
        Some(result)
    }

    fn latch_mode(&self) -> LatchMode {
        LatchMode::NoSuppress
    }
    fn schedule_mode(&self) -> ScheduleMode {
        ScheduleMode::Monophonic
    }
}
