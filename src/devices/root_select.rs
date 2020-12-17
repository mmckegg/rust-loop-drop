use std::time::{SystemTime, Duration};
use std::sync::{Arc, Mutex, MutexGuard};
use ::chunk::{Triggerable, OutputValue, ScheduleMode, LatchMode};
use std::collections::HashSet;

pub use ::scale::{Scale, Offset};

pub struct RootSelect {
    scale: Arc<Mutex<Scale>>
}

impl RootSelect {
    pub fn new (scale: Arc<Mutex<Scale>>) -> Self {
        RootSelect { scale }
    }
}

impl Triggerable for RootSelect {
    fn trigger (&mut self, id: u32, value: OutputValue) {
        match value {
            OutputValue::Off => {},
            OutputValue::On(velocity) => {
                let mut current_scale = self.scale.lock().unwrap();
                current_scale.root = 52 + (id as i32);
            }
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