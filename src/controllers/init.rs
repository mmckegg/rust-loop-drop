use ::midi_time::MidiTime;

pub struct Init {
    modulators: Vec<Option<::controllers::Modulator>>,
    scheduled: bool
}

impl Init {
    pub fn new (modulators: Vec<Option<::controllers::Modulator>>) -> Self {
        Init {
            modulators,
            scheduled: false
        }
    }
}

impl ::controllers::Schedulable for Init {
    fn schedule (&mut self, _pos: MidiTime, _length: MidiTime) {
        if !self.scheduled {
            for modulator in &mut self.modulators {
                if let Some(modulator) = modulator {
                    match modulator.modulator {
                        ::config::Modulator::Cc(id, value) => {
                            modulator.port.send(&[176 - 1 + modulator.channel, id, value]).unwrap();
                        },
                        ::config::Modulator::MaxCc(id, max, value) => {
                            modulator.port.send(&[176 - 1 + modulator.channel, id, value.min(max)]).unwrap();
                        },
                        ::config::Modulator::PitchBend(value) => {
                            let value = ::controllers::float_to_msb_lsb(value);
                            modulator.port.send(&[224 - 1 + modulator.channel, value.0, value.1]).unwrap();
                        }
                    }
                }
            }
            self.scheduled = true
        }
    }
}