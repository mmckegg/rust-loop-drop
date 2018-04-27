use ::midi_keys::Offset;
use ::chunk::{Triggerable, OutputValue, SystemTime};
use std::sync::{Arc, Mutex};

use std::collections::HashMap;

pub struct OffsetChunk {
    offset: Arc<Mutex<Offset>>,
    output_values: HashMap<u32, i32>
}

const OFFSETS: [i32; 8] = [ -4, -3, -2, -1, 1, 2, 3, 4 ];

impl OffsetChunk {
    pub fn new (offset: Arc<Mutex<Offset>>) -> Self {
        OffsetChunk {
            offset,
            output_values: HashMap::new()
        }
    }
}

impl Triggerable for OffsetChunk {
    fn trigger (&mut self, id: u32, value: OutputValue, _at: SystemTime) {
        match value {
            OutputValue::Off => {
                self.output_values.remove(&id);
            },
            OutputValue::On(velocity) => {
                let offset = OFFSETS[id as usize % OFFSETS.len()];
                self.output_values.insert(id, offset);
            }
        }

        let mut current = self.offset.lock().unwrap();
        current.offset = self.output_values.values().sum();
    }
}