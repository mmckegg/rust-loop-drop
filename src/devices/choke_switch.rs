use std::time::{SystemTime, Duration};
use std::sync::{Arc, Mutex, MutexGuard};
use ::chunk::{Triggerable, OutputValue, LatchMode};
use ::devices::SP404Choke;
use std::collections::HashSet;

pub use ::scale::{Scale, Offset};

pub struct ChokeSwitch {
    chokes: Arc<Mutex<SP404Choke>>
}

impl ChokeSwitch {
    pub fn new (chokes: Arc<Mutex<SP404Choke>>) -> Self {
        ChokeSwitch { chokes }
    }
}

impl Triggerable for ChokeSwitch {
    fn trigger (&mut self, id: u32, value: OutputValue, _at: SystemTime) {
        let mut current_choke = self.chokes.lock().unwrap();
        if id == 0 {
            current_choke.a = value.is_on()
        } else if id == 1 {
            current_choke.b = value.is_on()
        }
    }
}