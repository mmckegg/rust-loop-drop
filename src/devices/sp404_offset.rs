use ::chunk::{Triggerable, OutputValue, SystemTime};
use std::sync::{Arc};
use std::sync::atomic::{AtomicUsize, Ordering};

use std::collections::HashMap;

pub struct SP404Offset {
    offset: Arc<AtomicUsize>
}

const OFFSETS: [usize; 10] = [ 0, 1, 2, 3, 4, 5, 6, 7, 8, 9 ];

impl SP404Offset {
    pub fn new (offset: Arc<AtomicUsize>) -> Self {
        SP404Offset {
            offset
        }
    }
}

impl Triggerable for SP404Offset {
    fn trigger (&mut self, id: u32, value: OutputValue, _at: SystemTime) {
        match value {
            OutputValue::Off => {},
            OutputValue::On(velocity) => {
                let offset = OFFSETS[id as usize % OFFSETS.len()];
                self.offset.store(offset, Ordering::Relaxed)
            }
        }

    }
}