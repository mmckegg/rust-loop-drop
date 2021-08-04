mod init;
mod twister;
mod umi3;
mod vt4_key;

use midi_time::MidiTime;

pub use self::init::Init;
pub use self::twister::Twister;
pub use self::umi3::Umi3;
pub use self::vt4_key::VT4Key;

pub struct Modulator {
    pub port: ::midi_connection::SharedMidiOutputConnection,
    pub channel: u8,
    pub modulator: ::config::Modulator,
    pub rx_port: Option<::config::MidiPortConfig>,
}

impl Modulator {
    pub fn send_polar(&mut self, value: f64) {
        if let ::config::Modulator::PitchBend(..) = self.modulator {
            let value = polar_to_msb_lsb(value);
            self.port
                .send(&[224 - 1 + self.channel, value.0, value.1])
                .unwrap();
        } else {
            self.send(polar_to_midi(value));
        }
    }

    pub fn send(&mut self, value: u8) {
        match self.modulator {
            ::config::Modulator::Cc(id, ..) => {
                self.port
                    .send(&[176 - 1 + self.channel, id, value])
                    .unwrap();
            }
            ::config::Modulator::MaxCc(id, max, ..) => {
                let f_value = value as f64 / 127.0 as f64;
                let u_value = (f_value * max as f64).min(127.0) as u8;
                self.port
                    .send(&[176 - 1 + self.channel, id, u_value])
                    .unwrap();
            }
            ::config::Modulator::PitchBend(..) => {
                let value = polar_to_msb_lsb(midi_to_polar(value));
                self.port
                    .send(&[224 - 1 + self.channel, value.0, value.1])
                    .unwrap();
            }
        }
    }

    pub fn send_default(&mut self) {
        match self.modulator {
            ::config::Modulator::Cc(id, value) => {
                self.port
                    .send(&[176 - 1 + self.channel, id, value])
                    .unwrap();
            }
            ::config::Modulator::MaxCc(id, max, value) => {
                self.port
                    .send(&[176 - 1 + self.channel, id, value.min(max)])
                    .unwrap();
            }
            ::config::Modulator::PitchBend(value) => {
                let value = ::controllers::polar_to_msb_lsb(value);
                self.port
                    .send(&[224 - 1 + self.channel, value.0, value.1])
                    .unwrap();
            }
        }
    }
}

pub trait Schedulable {
    fn schedule(&mut self, _pos: MidiTime, _length: MidiTime) {}
}

pub fn polar_to_msb_lsb(input: f64) -> (u8, u8) {
    let max = (2.0f64).powf(14.0) / 2.0;
    let input_14bit = (input.max(-1.0).min(0.99999999999) * max + max) as u16;

    let lsb = mask7(input_14bit as u8);
    let msb = mask7((input_14bit >> 7) as u8);

    (lsb, msb)
}

/// 7 bit mask
#[inline(always)]
pub fn mask7(input: u8) -> u8 {
    input & 0b01111111
}

pub fn midi_to_polar(value: u8) -> f64 {
    if value < 63 {
        (value as f64 - 63.0) / 63.0
    } else if value > 64 {
        (value as f64 - 64.0) / 63.0
    } else {
        0.0
    }
}

pub fn midi_to_float(value: u8) -> f64 {
    value as f64 / 127.0
}

pub fn float_to_midi(value: f64) -> u8 {
    (value * 127.0).max(0.0).min(127.0) as u8
}

pub fn polar_to_midi(value: f64) -> u8 {
    let midi = (value + 1.0) / 2.0 * 127.0;
    midi.max(0.0).min(127.0) as u8
}

// pub fn random_range(from: u8, to: u8) -> u8 {
//     rand::thread_rng().gen_range(from, to)
// }

pub fn midi_ease_out(value: u8) -> u8 {
    let f = midi_to_float(value);
    float_to_midi(f * (2.0 - f))
}
