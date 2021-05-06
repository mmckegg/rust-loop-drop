use ::midi_connection;
use std::sync::{Arc, Mutex};
use ::scale::{Scale};
use ::scheduler::MidiTime;

pub struct VT4Key {
    midi_output: midi_connection::SharedMidiOutputConnection,
    channel: u8,
    scale: Arc<Mutex<Scale>>,
    last_key: Option<u8>
}

impl VT4Key {
    pub fn new (midi_output: midi_connection::SharedMidiOutputConnection, channel: u8, scale: Arc<Mutex<Scale>>) -> Self {
        VT4Key {
            midi_output,
            channel,
            scale,
            last_key: None
        }
    }
}

impl ::controllers::Schedulable for VT4Key {
    fn schedule (&mut self, _pos: MidiTime, _length: MidiTime) {
        let key;
        let scale = self.scale.lock().unwrap();

        { // immutable borrow
            let from_c = scale.root - 60;
            let base_key = modulo(from_c, 12);
            let offset = get_mode_offset(modulo(scale.scale, 7));
            key = modulo(base_key - offset, 12) as u8;
        }

        if Some(key) != self.last_key {
            self.midi_output.send(&[176 - 1 + self.channel, 48, key]).unwrap();
            self.last_key = Some(key);
        }
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