use ::chunk::{Triggerable, OutputValue, SystemTime, LatchMode};
use std::sync::{Arc, Mutex};
use std::collections::HashSet;

use std::collections::HashMap;

pub struct VelocityMap {
    values: Arc<Mutex<HashMap<u32, u8>>>,
}

impl VelocityMap {
    pub fn new (values: Arc<Mutex<HashMap<u32, u8>>>) -> Self {
        VelocityMap {
            values
        }
    }
}

impl Triggerable for VelocityMap {
    fn trigger (&mut self, id: u32, value: OutputValue, _at: SystemTime) {
        let mut current = self.values.lock().unwrap();
        match value {
            OutputValue::Off => {
                current.remove(&id);
            },
            OutputValue::On(velocity) => {
                current.insert(id, 127);
            }
        }
    }

    fn get_active (&self) -> Option<HashSet<u32>> {
        let values = self.values.lock().unwrap();

        let mut result = HashSet::new();
        for (key, _) in values.iter() { 
            result.insert(*key);
        }
        Some(result)
    }

    fn latch_mode (&self) -> LatchMode { LatchMode::NoSuppress }
}