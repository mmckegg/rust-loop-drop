use ::chunk::{Triggerable, OutputValue, SystemTime};
use std::sync::{Arc, Mutex};
use scale::{Scale};

use std::collections::HashMap;

pub struct RootOffsetChunk {
    scale: Arc<Mutex<Scale>>,
    output_values: HashMap<u32, i32>
}

const OFFSETS: [i32; 8] = [ -4, -3, -2, -1, 1, 2, 3, 4 ];

impl RootOffsetChunk {
    pub fn new (scale: Arc<Mutex<Scale>>) -> Self {
        RootOffsetChunk {
            scale,
            output_values: HashMap::new()
        }
    }
}

impl Triggerable for RootOffsetChunk {
    fn trigger (&mut self, id: u32, value: OutputValue) {
        match value {
            OutputValue::Off => {
                self.output_values.remove(&id);
            },
            OutputValue::On(velocity) => {
                let offset = OFFSETS[id as usize % OFFSETS.len()];
                self.output_values.insert(id, offset);
            }
        }

        let mut scale = self.scale.lock().unwrap();
        scale.offset = self.output_values.values().sum();
    }
}