use std::time::{SystemTime, Duration};
use std::sync::{Arc, Mutex, MutexGuard};
use ::chunk::{Triggerable, OutputValue, ScheduleMode, LatchMode};
use std::collections::HashSet;

pub use ::scale::{Scale, Offset};

pub struct ScaleSelect {
    scale: Arc<Mutex<Scale>>
}

impl ScaleSelect {
    pub fn new (scale: Arc<Mutex<Scale>>) -> Self {
        ScaleSelect { scale }
    }
}

impl Triggerable for ScaleSelect {
    fn trigger (&mut self, id: u32, value: OutputValue) {
        match value {
            OutputValue::Off => {},
            OutputValue::On(velocity) => {
                let mut current_scale = self.scale.lock().unwrap();
                current_scale.scale = id as i32;
            }
        }
    }

    fn get_active (&self) -> Option<HashSet<u32>> {
        let current_scale = self.scale.lock().unwrap();

        let mut result = HashSet::new();
        if current_scale.scale >= 0 {
            result.insert(current_scale.scale as u32);
        }
        Some(result)
    }

    fn latch_mode (&self) -> LatchMode { LatchMode::NoSuppress }
    fn schedule_mode (&self) -> ScheduleMode { ScheduleMode::Monophonic }
}