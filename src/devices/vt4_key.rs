use ::clock_source::{RemoteClock, FromClock};
use std::thread;
use ::midi_connection;
use std::sync::{Arc, Mutex};
use ::scale::{Scale};

pub struct VT4Key {
}

impl VT4Key {
    pub fn new (midi_output: midi_connection::SharedMidiOutputConnection, channel: u8, scale: Arc<Mutex<Scale>>, clock: RemoteClock) -> Self {
        thread::spawn(move || {
            for msg in clock.receiver {
                match msg {
                    FromClock::Schedule {..} => {
                        let key;
                        let scale = scale.lock().unwrap();

                        { // immutable borrow
                            let from_c = scale.root - 60;
                            let base_key = modulo(from_c, 12);
                            let offset = get_mode_offset(modulo(scale.scale, 7));
                            key = modulo(base_key - offset, 12) as u8;
                        }

                        if Some(key) != self.last_key {
                            self.midi_keys.midi_output.send(&[176, 48, key]).unwrap();
                            self.last_key = Some(key);
                            println!("Set Key {}", key);
                        }
                    }
                }
            }
        });

        VT4Key {}
    }
}

fn modulo (n: i32, m: i32) -> i32 {
    ((n % m) + m) % m
}