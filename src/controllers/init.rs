use midi_time::MidiTime;

pub struct Init {
    modulators: Vec<::controllers::Modulator>,
    scheduled: bool,
}

impl Init {
    pub fn new(modulators: Vec<::controllers::Modulator>) -> Self {
        Init {
            modulators,
            scheduled: false,
        }
    }
}

impl ::controllers::Schedulable for Init {
    fn schedule(&mut self, _pos: MidiTime, _length: MidiTime) {
        if !self.scheduled {
            for modulator in &mut self.modulators {
                if let ::controllers::Modulator::MidiModulator(instance) = modulator {
                    instance.send_default();
                }
            }
            self.scheduled = true
        }
    }
}
