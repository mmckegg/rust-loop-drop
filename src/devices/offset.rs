use ::chunk::{Triggerable, OutputValue, SystemTime};
use ::midi_keys::Offset;
use std::sync::{Arc, Mutex};

use std::collections::HashMap;

pub struct OffsetChunk {
    offset: Arc<Mutex<Offset>>,
    output_values: HashMap<u32, i32>
}

const OFFSET_MAP: [i32; 8] = [
    -4, -3, -2, -1, 1, 2, 3, 4
];

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
        let offset_value = match value { 
            OutputValue::On(_) => *OFFSET_MAP.get(id as usize).unwrap_or(&0),
            OutputValue::Off => 0 
        };
        self.output_values.insert(id, offset_value);

        let mut current = self.offset.lock().unwrap();
        current.offset = self.output_values.values().sum();
    }
}