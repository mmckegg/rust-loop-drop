use chunk::{OutputValue, Triggerable};
use scale::Scale;
use std::sync::{Arc, Mutex};

use std::collections::HashMap;

pub struct ScaleOffsetChunk {
    scale: Arc<Mutex<Scale>>,
    output_values: HashMap<u32, i32>,
}

const OFFSETS: [i32; 8] = [-4, -3, -2, -1, 1, 2, 3, 4];

impl ScaleOffsetChunk {
    pub fn new(scale: Arc<Mutex<Scale>>) -> Self {
        ScaleOffsetChunk {
            scale,
            output_values: HashMap::new(),
        }
    }
}

impl Triggerable for ScaleOffsetChunk {
    fn trigger(&mut self, id: u32, value: OutputValue) {
        match value {
            OutputValue::Off => {
                self.output_values.remove(&id);
            }
            OutputValue::On(_velocity) => {
                let offset = OFFSETS[id as usize % OFFSETS.len()];
                self.output_values.insert(id, offset);
            }
        }

        let mut scale = self.scale.lock().unwrap();
        scale.scale = modulo(self.output_values.values().sum(), 7);
    }
}

fn modulo(n: i32, m: i32) -> i32 {
    ((n % m) + m) % m
}
