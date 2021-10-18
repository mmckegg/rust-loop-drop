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
        if (pos.ticks() - 1) % self.divider == 0 {
            self.midi_output
                .send(&[144 - 1 + self.channel, 36, 127])
                .unwrap();
            self.midi_output
                .send(&[144 - 1 + self.channel, 36, 0])
                .unwrap();
        }
    }
}
