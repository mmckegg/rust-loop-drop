use std::sync::{Arc, Mutex};

use midi_connection;
use scheduler::MidiTime;

use crate::loop_grid_launchpad::LoopGridParams;

pub struct ClockPulse {
    midi_output: midi_connection::SharedMidiOutputConnection,
    channel: u8,
    divider: i32,
    reset_tick_step: u32,
    last_pitch: u8,
    params: Arc<Mutex<LoopGridParams>>,
}

impl ClockPulse {
    pub fn new(
        midi_output: midi_connection::SharedMidiOutputConnection,
        channel: u8,
        divider: i32,
        params: Arc<Mutex<LoopGridParams>>,
    ) -> Self {
        ClockPulse {
            midi_output,
            channel,
            divider,
            last_pitch: 0,
            reset_tick_step: 0,
            params,
        }
    }
}

impl ::controllers::Schedulable for ClockPulse {
    fn schedule(&mut self, pos: MidiTime, _length: MidiTime) {
        let tick = (pos.ticks() - 1) % self.divider;
        if tick == 0 {
            self.last_pitch = if self.reset_tick_step == 0 { 127 } else { 0 };

            self.midi_output
                .send(&[144 - 1 + self.channel, self.last_pitch, 127])
                .unwrap();

            let params = self.params.lock().unwrap();
            if params.reset_beat > 0 {
                self.reset_tick_step = (self.reset_tick_step + 1) % params.reset_beat;
            }
        } else if tick == 1 {
            self.midi_output
                .send(&[144 - 1 + self.channel, self.last_pitch, 0])
                .unwrap();
        }
    }
}
