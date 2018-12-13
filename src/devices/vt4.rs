use ::midi_connection;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::thread;
use ::chunk::{Triggerable, OutputValue, SystemTime, ScheduleMode};
use ::devices::MidiKeys;
use std::time::Duration;

pub use ::scale::{Scale, Offset};
pub use ::midi_connection::SharedMidiOutputConnection;

pub struct VT4 {
    midi_keys: MidiKeys,
    last_key: Option<u8>
}

impl VT4 {
    pub fn new (midi_output: midi_connection::SharedMidiOutputConnection, scale: Arc<Mutex<Scale>>, offset: Arc<Mutex<Offset>>) -> Self {
        VT4 {
            midi_keys: MidiKeys::new(midi_output, 1, scale, offset),
            last_key: None
        }
    }
}

impl Triggerable for VT4 {
    fn trigger (&mut self, id: u32, value: OutputValue, time: SystemTime) {
        self.midi_keys.trigger(id, value, time)
    }

    fn on_tick (&mut self) {
        let key;

        { // immutable borrow
            let scale = self.midi_keys.scale();
            let from_c = scale.root - 60;
            let base_key = modulo(from_c, 12);
            let offset = get_mode_offset(modulo(scale.scale, 7));
            key = modulo(base_key - offset, 12) as u8;
        }
        

        if Some(key) != self.last_key {
            self.midi_keys.midi_output.send(&[176, 48, key]).unwrap();
            self.last_key = Some(key);
        }
    }

    fn schedule_mode (&self) -> ScheduleMode {
        ScheduleMode::Monophonic
    }
}

fn modulo (n: i32, m: i32) -> i32 {
    ((n % m) + m) % m
}

fn get_mode_offset (mode: i32) -> i32 {
    let mut offset = 0;
    let intervals = [2, 2, 1, 2, 2, 2, 1];

    for i in 0..6 {
        if (i as i32) >= mode {
            break
        }
        offset += intervals[i];
    }

    offset
}