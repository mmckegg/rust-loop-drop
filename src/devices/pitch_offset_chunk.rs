use ::chunk::{Triggerable, OutputValue, SystemTime};
use std::sync::{Arc, Mutex};
use scale::{Scale};
pub use ::midi_connection::SharedMidiOutputConnection;

use std::collections::HashMap;

pub struct PitchOffsetChunk {
    midi_output: SharedMidiOutputConnection,
    channel: u8,
    output_values: HashMap<u32, i32>
}

const OFFSETS: [i32; 8] = [ -4, -3, -2, -1, 1, 2, 3, 4 ];

impl PitchOffsetChunk {
    pub fn new (midi_output: SharedMidiOutputConnection, channel: u8) -> Self {
        PitchOffsetChunk {
            midi_output,
            channel,
            output_values: HashMap::new()
        }
    }
}

impl Triggerable for PitchOffsetChunk {
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

        let result: i32 = self.output_values.values().sum();
        let msb_lsb = polar_to_msb_lsb(result as f32 / 12.0);
        self.midi_output.send(&[224 + self.channel - 1, msb_lsb.0, msb_lsb.1]);
    }
}

pub fn polar_to_msb_lsb(input: f32) -> (u8, u8) {
    let max = (2.0f32).powf(14.0) / 2.0;
    let input_14bit = (input.max(-1.0).min(1.0) * max + max) as u16;

    let lsb = mask7(input_14bit as u8);
    let msb = mask7((input_14bit >> 7) as u8);
    (lsb, msb)
}

/// 7 bit mask
#[inline(always)]
pub fn mask7(input: u8) -> u8 {
    input & 0b01111111
}