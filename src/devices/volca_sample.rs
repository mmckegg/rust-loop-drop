use ::chunk::{Triggerable, OutputValue, SystemTime};
use ::midi_connection;

pub struct VolcaSample {
    midi_port: midi_connection::MidiOutputConnection
}

impl VolcaSample {
    pub fn new (midi_port_name: &str) -> Self {
        VolcaSample {
            midi_port: midi_connection::get_output(midi_port_name).unwrap()
        }
    }
}

impl Triggerable for VolcaSample {
    fn trigger (&mut self, id: u32, value: OutputValue, at: SystemTime) {
        match value {
            OutputValue::Off => {},
            OutputValue::On => {
                let channel_map: [u8; 16] = [
                  0, 1, 8, 9, 
                  2, 3, 4, 5, 
                  6, 6, 6, 6,
                  7, 7, 7, 7
                ];

                let channel = channel_map[id as usize];

                if id >= 8 {
                    let pos = id % 4;
                    let offset: i32 = match pos {
                        1 => -14,
                        2 => 14,
                        3 => 18,
                        _ => 0
                    };
                    self.midi_port.send(&[176 + channel, 43, (64 + offset) as u8]).unwrap();
                } 

                self.midi_port.send(&[144 + channel, 0, 127]).unwrap();
            }
        }
    }
}