mod clock_pulse;
mod duck_output;
mod init;
mod launchpad_tempo;
mod mod_twister;
mod twister;
mod umi3;

use std::collections::HashSet;

use midi_time::MidiTime;

use crate::scheduler::ScheduleRange;

pub use self::clock_pulse::ClockPulse;
pub use self::duck_output::DuckOutput;
pub use self::init::Init;
pub use self::launchpad_tempo::LaunchpadTempo;
pub use self::mod_twister::ModTwister;
pub use self::twister::Twister;
pub use self::umi3::Umi3;

pub enum Modulator {
    None,
    MidiModulator(MidiModulator),
    DuckDecay(u8),
    DuckAmount(u8),
    Swing(u8),
    LfoAmount(usize, u8),
    LfoSkew(u8),
    LfoSpeed(u8),
    LfoOffset(u8),
    LfoHold(u8),
}

pub struct MidiModulator {
    pub port: ::midi_connection::SharedMidiOutputConnection,
    pub channel: u8,
    pub modulator: ::config::Modulator,
    pub rx_port: Option<::config::MidiPortConfig>,
    triggered: HashSet<u8>,
}

impl MidiModulator {
    pub fn new(
        port: ::midi_connection::SharedMidiOutputConnection,
        channel: u8,
        modulator: ::config::Modulator,
        rx_port: Option<::config::MidiPortConfig>,
    ) -> Self {
        Self {
            port,
            channel,
            modulator,
            rx_port,
            triggered: HashSet::new(),
        }
    }

    pub fn send_polar(&mut self, value: f64) {
        match self.modulator {
            ::config::Modulator::PitchBend(..) | ::config::Modulator::PositivePitchBend(..) => {
                let value = polar_to_msb_lsb(value);
                self.port
                    .send(&[224 - 1 + self.channel, value.0, value.1])
                    .unwrap();
            }
            _ => {
                self.send(polar_to_midi(value));
            }
        }
    }

    pub fn send(&mut self, value: u8) {
        for modulator in self.modulator.all() {
            match modulator {
                ::config::Modulator::Cc(id, ..) => {
                    self.port
                        .send(&[176 - 1 + self.channel, id, value])
                        .unwrap();
                }
                ::config::Modulator::Aftertouch(..) => {
                    self.port.send(&[208 - 1 + self.channel, value]).unwrap();
                }
                ::config::Modulator::InvertCc(id, ..) => {
                    self.port
                        .send(&[176 - 1 + self.channel, id, 127 - value])
                        .unwrap();
                }
                ::config::Modulator::InvertMaxCc(id, max, ..) => {
                    let f_value = value as f64 / 127.0 as f64;
                    let u_value = (f_value * max as f64).min(127.0) as u8;
                    self.port
                        .send(&[176 - 1 + self.channel, id, max - u_value])
                        .unwrap();
                }
                ::config::Modulator::PolarCcSwitch {
                    cc_low,
                    cc_high,
                    cc_switch,
                    ..
                } => {
                    let polar_value = midi_to_polar(value);
                    if polar_value < 0.0 {
                        if let Some(cc) = cc_low {
                            let abs = polar_value * -1.0;
                            let value = float_to_midi(abs * abs);
                            self.port
                                .send(&[176 - 1 + self.channel, cc, value])
                                .unwrap();
                        }
                    } else {
                        if let Some(cc) = cc_high {
                            let value = float_to_midi(polar_value);
                            self.port
                                .send(&[176 - 1 + self.channel, cc, value])
                                .unwrap();
                        }
                    }

                    if let Some(cc) = cc_switch {
                        let value = if polar_value < 0.0 { 0 } else { 127 };

                        self.port
                            .send(&[176 - 1 + self.channel, cc, value])
                            .unwrap();
                    }
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
                ::config::Modulator::PositivePitchBend(..) => {
                    let value = polar_to_msb_lsb(midi_to_float(value));
                    self.port
                        .send(&[224 - 1 + self.channel, value.0, value.1])
                        .unwrap();
                }
                ::config::Modulator::TriggerWhen(condition, (note, velocity)) => {
                    if condition.check(value) {
                        if !self.triggered.contains(&note) {
                            self.port
                                .send(&[144 - 1 + self.channel, note, velocity])
                                .unwrap();
                            self.triggered.insert(note);
                        }
                    } else {
                        if self.triggered.contains(&note) {
                            self.port.send(&[128 - 1 + self.channel, note, 0]).unwrap();
                            self.triggered.remove(&note);
                        }
                    }
                }
                ::config::Modulator::Multi(..) => (/* handled by .all() */),
            }
        }
    }

    pub fn send_default(&mut self) {
        for modulator in self.modulator.all() {
            match modulator {
                ::config::Modulator::Cc(id, value) => {
                    self.port
                        .send(&[176 - 1 + self.channel, id, value])
                        .unwrap();
                }
                ::config::Modulator::Aftertouch(value) => {
                    self.port.send(&[208 - 1 + self.channel, value]).unwrap();
                }
                ::config::Modulator::InvertCc(id, value) => {
                    self.port
                        .send(&[176 - 1 + self.channel, id, 127 - value])
                        .unwrap();
                }
                ::config::Modulator::PolarCcSwitch { default, .. } => {
                    self.send(default);
                }
                ::config::Modulator::MaxCc(id, max, value) => {
                    self.port
                        .send(&[176 - 1 + self.channel, id, value.min(max)])
                        .unwrap();
                }
                ::config::Modulator::InvertMaxCc(id, max, value) => {
                    self.port
                        .send(&[176 - 1 + self.channel, id, max - value.min(max)])
                        .unwrap();
                }
                ::config::Modulator::PitchBend(value)
                | ::config::Modulator::PositivePitchBend(value) => {
                    let value = ::controllers::polar_to_msb_lsb(value);
                    self.port
                        .send(&[224 - 1 + self.channel, value.0, value.1])
                        .unwrap();
                }
                ::config::Modulator::TriggerWhen(..) => (),
                ::config::Modulator::Multi(..) => (/* handled by .all() */),
            }
        }
    }
}

pub trait Schedulable {
    fn schedule(&mut self, range: ScheduleRange) {}
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
