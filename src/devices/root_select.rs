use std::time::{SystemTime, Duration};
use std::sync::{Arc, Mutex, MutexGuard};
use ::chunk::{Triggerable, OutputValue, LatchMode};
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
    fn trigger (&mut self, id: u32, value: OutputValue, _at: SystemTime) {
        match value {
            OutputValue::Off => {},
            OutputValue::On(velocity) => {
                let mut current_scale = self.scale.lock().unwrap();
                current_scale.root = 64 + (id as i32);
            }
        }
    }

    fn latch_mode (&self) -> LatchMode { LatchMode::LatchSingle }
}