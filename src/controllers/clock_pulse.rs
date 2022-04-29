use std::sync::{Arc, Mutex};

use midi_connection;
use scheduler::MidiTime;

pub struct ClockPulse {
    midi_output: midi_connection::SharedMidiOutputConnection,
    channel: u8,
    divider: i32,
}

impl ClockPulse {
    pub fn new(
        midi_output: midi_connection::SharedMidiOutputConnection,
        channel: u8,
        divider: i32,
    ) -> Self {
        ClockPulse {
            midi_output,
            channel,
            divider,
        }
    }
}

impl ::controllers::Schedulable for ClockPulse {
    fn schedule(&mut self, pos: MidiTime, _length: MidiTime) {
        let tick = (pos.ticks() - 1) % self.divider;
        if tick == 0 {
            self.midi_output
                .send(&[144 - 1 + self.channel, 64, 127])
                .unwrap();
        } else if tick == 1 {
            self.midi_output
                .send(&[128 - 1 + self.channel, 64, 0])
                .unwrap();
        }
    }
}
