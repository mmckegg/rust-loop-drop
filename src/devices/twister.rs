use ::midi_connection;
use std::sync::mpsc;
use ::loop_recorder::{LoopRecorder, LoopEvent};
use ::clock_source::{RemoteClock, FromClock, ToClock, MidiTime};
use ::output_value::OutputValue;
use ::loop_grid_launchpad::LoopGridParams;
use ::throttled_output::ThrottledOutput;
use ::lfo::Lfo;
use rand::Rng;

use std::thread;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

pub struct Twister {
    _midi_input: midi_connection::ThreadReference
}

impl Twister {
    pub fn new (port_name: &str, main_output: midi_connection::SharedMidiOutputConnection, params: Arc<Mutex<LoopGridParams>>, clock: RemoteClock) -> Self {
        let (tx, rx) = mpsc::channel();
        let clock_sender = clock.sender.clone();
        let control_ids = get_control_ids();

        let channel_map = [1, 4, 2, 3];

        let drum_channel = 1;
        let bass_channel = 2;
        let synth_channel = 3;
        let slicer_channel = 4;
        let reverb_channel = 6;
        let delay_channel = 7;

        let mixer_offset = 7;

        let channel_offsets = [11, 21, 31, 41];

        let tx_input = tx.clone();
        let tx_clock = tx.clone();
        let tx_feedback = tx.clone();

        // pipe clock in
        thread::spawn(move || {
            for msg in clock.receiver {
                tx_clock.send(TwisterMessage::Clock(msg)).unwrap();
            }
        });

        let mut output = midi_connection::get_shared_output(port_name);

        let input = midi_connection::get_input(port_name, move |_stamp, message| {
            let control = Control::from_id(message[1] as u32);
            if message[0] == 176 {
                tx_input.send(TwisterMessage::ControlChange(control, OutputValue::On(message[2]))).unwrap();
            } else if message[0] == 177 {
                tx_input.send(TwisterMessage::Recording(control, message[2] > 0)).unwrap();
            }
        });

        thread::spawn(move || {
            let mut recorder = LoopRecorder::new();
            let mut last_pos = MidiTime::zero();
            let mut last_values: HashMap<Control, u8> = HashMap::new();
            let mut record_start_times = HashMap::new();
            let mut loops: HashMap<Control, Loop> = HashMap::new();
            let mut throttled_main_output = ThrottledOutput::new(main_output);
            // let mut throttled_main_output = ThrottledOutput::new(blofeld_output);

            let mut frozen = false;
            let mut frozen_values = None;
            let mut frozen_loops = None;

            let mut lfo_amounts = HashMap::new();

            let mut lfo = Lfo::new();

            for channel in 0..4 {
                last_values.insert(Control::ChannelControl(channel, 0), 100); // volume
                last_values.insert(Control::ChannelControl(channel, 1), 0); // reverb send
                last_values.insert(Control::ChannelControl(channel, 2), 0); // delay send
                last_values.insert(Control::ChannelControl(channel, 3), 64); // filter
                last_values.insert(Control::ChannelControl(channel, 4), 0);
                last_values.insert(Control::ChannelControl(channel, 5), 0);
            }

            last_values.insert(Control::LfoHold, lfo.hold);
            last_values.insert(Control::LfoOffset, lfo.offset);
            last_values.insert(Control::LfoSkew, lfo.skew);
            last_values.insert(Control::LfoSpeed, lfo.speed);

            last_values.insert(Control::KickDecay, 64);
            last_values.insert(Control::KickPitch, 64);
            last_values.insert(Control::KickDuck, 64);

            last_values.insert(Control::SamplePitch, 64);


            last_values.insert(Control::SynthPitch, 64);
            last_values.insert(Control::SynthEnv, 64);
            last_values.insert(Control::SynthPitchEnv, 64);
            last_values.insert(Control::SynthFilterEnv, 64);
            last_values.insert(Control::SynthCutoff, 64);
            last_values.insert(Control::SynthDuty, 127);
            last_values.insert(Control::SynthAdsr(2), 127);
            last_values.insert(Control::SynthAdsr(3), 64);

            last_values.insert(Control::BassPitch, 64);

            last_values.insert(Control::BassEnv, 64);
            last_values.insert(Control::BassPitchEnv, 64);
            last_values.insert(Control::BassFilterEnv, 80);
            last_values.insert(Control::BassCutoff, 40);
            last_values.insert(Control::BassSub, 127);
            last_values.insert(Control::BassDuty, 127);
            last_values.insert(Control::BassAdsr(1), 50);
            last_values.insert(Control::BassAdsr(2), 30);
            last_values.insert(Control::BassAdsr(3), 64);

            last_values.insert(Control::DelayTimeA, 90);
            last_values.insert(Control::DelayTimeB, 40);
            last_values.insert(Control::DelayFeedback, 50);

            last_values.insert(Control::ReverbTime, 20);

            last_values.insert(Control::Tempo, random_range(20, 80));
            last_values.insert(Control::Swing, random_range(64, 70));

            // update display and send all of the start values on load
            for control in control_ids.keys() {
                tx.send(TwisterMessage::Send(*control)).unwrap();
                tx.send(TwisterMessage::Refresh(*control)).unwrap();
            }

            for received in rx {
                match received {
                    TwisterMessage::ControlChange(control, value) => {
                        if let Some(id) = control_ids.get(&control) {
                            let allow = if loops.contains_key(&control) {
                                let item = loops.get(&control).unwrap();
                                (item.offset + item.length) < (last_pos - MidiTime::from_ticks(8))
                            } else {
                                true
                            };

                            if allow {
                                loops.remove(&control);
                                tx_feedback.send(TwisterMessage::Event(LoopEvent { 
                                    id: id.clone(), 
                                    value,
                                    pos: last_pos
                                })).unwrap();
                            }
                        }
                    },
                    TwisterMessage::Send(control) => {
                        let last_value = last_values.get(&control).unwrap_or(&0);
                        let value = if let Some(lfo_amount) = lfo_amounts.get(&control) {
                            let lfo_value = lfo.get_value_at(last_pos);
                            if *lfo_amount > 0.0 {
                                // bipolar modulation (CV style)
                                let polar = ((lfo_value * 2.0) - 1.0) * lfo_amount;
                                (*last_value as f64 + (polar * 64.0)).min(127.0).max(0.0) as u8
                            } else {
                                // treat current value as max and multiplier (subtract / sidechain style)
                                let offset: f64 = lfo_value * (*last_value as f64) * lfo_amount;
                                (*last_value as f64 + offset).min(127.0).max(0.0) as u8
                            }
                        } else {
                            *last_value
                        };
                        

                        match control {
                            Control::ChannelControl(channel, control) => {
                                throttled_main_output.send(&[176 + channel_map[channel as usize] + mixer_offset - 1, control as u8 + 1, value]);
                            },
                            Control::Tempo => {
                                clock_sender.send(ToClock::SetTempo(value as usize + 60)).unwrap();
                            },
                            Control::Swing => {
                                let mut params = params.lock().unwrap();
                                let linear_swing = (value as f64 - 64.0) / 64.0;
                                params.swing = if value == 63 || value == 64 {
                                    0.0
                                } else if linear_swing < 0.0 {
                                    -linear_swing.abs().powf(2.0)
                                } else {
                                    linear_swing.powf(2.0)
                                };
                            },
                            Control::ReverbTime => {
                                throttled_main_output.send(&[176 + reverb_channel - 1, 1, value]);
                            },
                            Control::ReverbPre => {
                                throttled_main_output.send(&[176 + reverb_channel - 1, 2, value]);
                            },
                            Control::DelayTimeA => {
                                // throttled_main_output.send(&[176 + zoia_fx_channel - 1, 21, value]);
                            },
                            Control::DelayTimeB => {
                                throttled_main_output.send(&[176 + delay_channel - 1, 1, value]);
                            },
                            Control::DelayFeedback => {
                                throttled_main_output.send(&[176 + delay_channel - 1, 2, value]);
                            },
                            Control::LfoSpeed => {
                                lfo.speed = value;
                            },
                            Control::LfoSkew => {
                                lfo.skew = value;
                            },
                            Control::LfoHold => {
                                lfo.hold = value;
                            },
                            Control::LfoOffset => {
                                lfo.offset = value;
                            },
                            Control::KickDecay => {
                                // slicer bit reduction
                                throttled_main_output.send(&[(176 - 1) + slicer_channel + mixer_offset, 5, value]);
                            },
                            Control::KickPitch => {
                                // slicer bit crush
                                throttled_main_output.send(&[(176 - 1) + slicer_channel + mixer_offset, 6, value]);
                            },
                            Control::KickDuck => {
                                // bass bit crush
                                throttled_main_output.send(&[(176 - 1) + bass_channel + mixer_offset, 6, value]);
                            },
                            Control::BassAdsr(param) => {
                                throttled_main_output.send(&[(176 - 1) + bass_channel, param as u8 + 1, value]);
                            },

                            Control::SynthAdsr(param) => {
                                throttled_main_output.send(&[(176 - 1) + synth_channel, param as u8 + 1, value]);
                            },

                            Control::BassPorta => {
                                throttled_main_output.send(&[(176 - 1) + bass_channel, 5, value]);
                            },

                            Control::SynthPorta => {
                                throttled_main_output.send(&[(176 - 1) + synth_channel, 5, value]);
                            },

                            Control::BassCutoff => {
                                throttled_main_output.send(&[(176 - 1) + bass_channel, 6, value]);
                            },

                            Control::SynthCutoff => {
                                throttled_main_output.send(&[(176 - 1) + synth_channel, 6, value]);
                            },

                            Control::BassRes => {
                                throttled_main_output.send(&[(176 - 1) + bass_channel, 7, value]);
                            },

                            Control::SynthRes => {
                                throttled_main_output.send(&[(176 - 1) + synth_channel, 7, value]);
                            },

                            Control::BassFilterEnv => {
                                throttled_main_output.send(&[(176 - 1) + bass_channel, 8, value]);
                            },

                            Control::SynthFilterEnv => {
                                throttled_main_output.send(&[(176 - 1) + synth_channel, 8, value]);
                            },

                            Control::BassWaveform => {
                                throttled_main_output.send(&[(176 - 1) + bass_channel, 9, value]);
                            },
                           
                            Control::SynthWaveform => {
                                throttled_main_output.send(&[(176 - 1) + synth_channel, 9, value]);
                            },

                            Control::BassSub => {
                                throttled_main_output.send(&[(176 - 1) + bass_channel, 10, value]);
                            },

                            Control::SynthSub => {
                                throttled_main_output.send(&[(176 - 1) + synth_channel, 10, value]);
                            },

                            Control::BassNoise => {
                                throttled_main_output.send(&[(176 - 1) + bass_channel, 11, value]);
                            },
                            
                            Control::SynthNoise => {
                                throttled_main_output.send(&[(176 - 1) + synth_channel, 11, value]);
                            },

                            Control::BassEnv => {
                                throttled_main_output.send(&[(176 - 1) + bass_channel, 12, value]);
                            },

                            Control::SynthEnv => {
                                throttled_main_output.send(&[(176 - 1) + synth_channel, 12, value]);
                            },

                            Control::BassDuty => {
                                throttled_main_output.send(&[(176 - 1) + bass_channel, 13, value]);
                            },

                            Control::SynthDuty => {
                                throttled_main_output.send(&[(176 - 1) + synth_channel, 13, value]);
                            },

                            Control::BassVibrato => {
                                throttled_main_output.send(&[(176 - 1) + bass_channel, 14, value]);
                            },

                            Control::SynthVibrato => {
                                throttled_main_output.send(&[(176 - 1) + synth_channel, 14, value]);
                            },

                            Control::SamplePitch => {
                                // hack around detent on mf twister
                                let value = if value == 63 {
                                    64
                                } else {
                                    value
                                };
                                throttled_main_output.send(&[(224 - 1) + slicer_channel, 0, value]);
                            },

                            Control::BassPitch => {
                                // hack around detent on mf twister
                                let value = if value == 63 {
                                    64
                                } else {
                                    value
                                };
                                throttled_main_output.send(&[(224 - 1) + bass_channel, 0, value]);
                            },

                            Control::SynthPitch => {
                                // hack around detent on mf twister
                                let value = if value == 63 {
                                    64
                                } else {
                                    value
                                };
                                throttled_main_output.send(&[(224 - 1) + synth_channel, 0, value]);
                            },

                            Control::BassPitchEnv => {
                                // hack around detent on mf twister
                                let value = if value < 64 {
                                    value + 1
                                } else {
                                    value
                                };
                                throttled_main_output.send(&[(176 - 1) + bass_channel, 16, value]);
                            },

                            Control::SynthPitchEnv => {
                                // hack around detent on mf twister
                                let value = if value < 64 {
                                    value + 1
                                } else {
                                    value
                                };
                                throttled_main_output.send(&[(176 - 1) + synth_channel, 16, value]);
                            },

                            Control::None => ()
                        }

                    },
                    TwisterMessage::Event(event) => {
                        let control = Control::from_id(event.id);
                        let value = event.value.value();

                        last_values.insert(control, value);

                        tx_feedback.send(TwisterMessage::Send(control)).unwrap();
                        tx_feedback.send(TwisterMessage::Refresh(control)).unwrap();

                        recorder.add(event);
                    },

                    TwisterMessage::Recording(control, recording) => {
                        if recording {
                            record_start_times.insert(control, last_pos);
                        } else {
                            if let Some(pos) = record_start_times.remove(&control) {
                                let loop_length = MidiTime::quantize_length(last_pos - pos);
                                if loop_length < MidiTime::from_ticks(16) {
                                    loops.remove(&control);
                                } else {
                                    loops.insert(control, Loop { 
                                        offset: last_pos - loop_length, 
                                        length: loop_length
                                    });
                                }
                            }
                        }
                    },

                    TwisterMessage::Refresh(control) => {
                        let value = *last_values.get(&control).unwrap_or(&0);
                        if let Some(id) = control_ids.get(&control) {
                            output.send(&[176, id.clone() as u8, value]).unwrap();

                            // MFT animation for currently looping (Channel 6)
                            if loops.contains_key(&control) {
                                output.send(&[181, id.clone() as u8, 13]).unwrap();
                            } else {
                                output.send(&[181, id.clone() as u8, 0]).unwrap();
                            }
                        }
                    },

                    TwisterMessage::Clock(msg) => {
                        match msg {
                            FromClock::Schedule { pos, length } => {
                                let mut params = params.lock().unwrap();
                                if params.reset_automation {
                                    // HACK: ack reset message from clear all
                                    params.reset_automation = false;
                                    loops.clear();

                                    for control in control_ids.keys() {
                                        tx.send(TwisterMessage::Refresh(*control)).unwrap();
                                    }
                                }

                                // emit beat tick for tap delay tempo
                                // if pos % MidiTime::from_beats(1) == MidiTime::zero() {
                                //     let value = if pos % MidiTime::from_beats(2) == MidiTime::zero() {
                                //         127 // rise
                                //     } else {
                                //         0 // fall
                                //     };
                                //     throttled_main_output.send(&[176 - 1 + zoia_fx_channel, 1, value])
                                // }

                                if params.frozen != frozen {
                                    frozen = params.frozen;

                                    if frozen {
                                        frozen_values = Some(last_values.clone());
                                        frozen_loops = Some(loops.clone());
                                    } else {
                                        if let Some(frozen_loops) = frozen_loops.take() {
                                            loops = frozen_loops;
                                        }
                                        if let Some(frozen_values) = frozen_values.take() {
                                            for (control, _) in &control_ids {
                                                if !loops.contains_key(control) && frozen_values.get(control) != last_values.get(control) {
                                                    // queue a value send for changed values on next message loop
                                                    tx.send(TwisterMessage::Send(*control)).unwrap();
                                                }
                                                tx.send(TwisterMessage::Refresh(*control)).unwrap();
                                            }

                                            last_values = frozen_values;
                                        }
                                    }
                                }

                                let mut scheduled = HashSet::new();
                                for (control, value) in &loops {
                                    let offset = value.offset % value.length;
                                    let playback_pos = value.offset + ((pos - offset) % value.length);

                                    if let Some(id) = control_ids.get(control) {
                                        if let Some(range) = recorder.get_range_for(id.clone(), playback_pos, playback_pos + length) {
                                            for event in range {
                                                tx_feedback.send(TwisterMessage::Event(event.clone())).unwrap();
                                                scheduled.insert(control);
                                            }
                                        }
                                    }
                                }
                                for (control, value) in &lfo_amounts {
                                    if value != &0.0 {
                                        tx_feedback.send(TwisterMessage::Send(*control)).unwrap();
                                    }
                                }
                                last_pos = pos;

                                throttled_main_output.flush();
                            },
                            FromClock::Tempo(value) => {
                                tx_feedback.send(TwisterMessage::Refresh(Control::Tempo)).unwrap();
                            },
                            FromClock::Jump => {

                            }
                        }
                    }
                }
            }
        });

        Twister {
            _midi_input: input
        }
    }
}

#[derive(Debug, Clone)]
enum TwisterMessage {
    ControlChange(Control, OutputValue),
    Event(LoopEvent),
    Send(Control),
    Refresh(Control),
    Recording(Control, bool),
    Clock(FromClock)
}

#[derive(Debug, Eq, PartialEq, Hash, Clone, Copy)]
enum Control {
    ChannelControl(u32, u32),

    LfoSkew,
    LfoHold,
    LfoSpeed,
    LfoOffset,

    KickDecay,
    KickPitch,
    KickDuck,

    Tempo,
    Swing,
    DelayTimeA,
    DelayTimeB,
    DelayFeedback,
    ReverbTime,
    ReverbPre,

    SamplePitch,

    BassAdsr(u32),
    BassPorta,
    BassCutoff,
    BassRes,
    BassFilterEnv,
    BassWaveform,
    BassSub,
    BassNoise,
    BassEnv,
    BassDuty,
    BassVibrato,
    BassPitch,
    BassPitchEnv,

    SynthAdsr(u32),
    SynthPorta,
    SynthCutoff,
    SynthRes,
    SynthFilterEnv,
    SynthWaveform,
    SynthSub,
    SynthNoise,
    SynthEnv,
    SynthDuty,
    SynthVibrato,
    SynthPitch,
    SynthPitchEnv,
    
    None
}

#[derive(Debug, Clone)]
struct Loop {
    offset: MidiTime,
    length: MidiTime
}

impl Control {
    fn from_id (id: u32) -> Control {
        let col = id % 4;
        let row = (id / 4) % 4;
        let page = id / 16;
        let coords = (page, row, col);

        match coords {
            // Bank A
            (0, row, col) => Control::ChannelControl(row, col),

            // Bank B

            (1, 0, 2) => Control::Swing,
            (1, 0, 3) => Control::Tempo,

            (1, 1, 2) => Control::SamplePitch,
            (1, 2, 2) => Control::ReverbTime,
            (1, 3, 2) => Control::ReverbPre,
  
            (1, 1, 3) => Control::DelayTimeA,
            (1, 2, 3) => Control::DelayTimeB,
            (1, 3, 3) => Control::DelayFeedback,

            (1, row, col) => Control::ChannelControl(row, col + 4),

            // Bank C
            (2, 0, col) => Control::BassAdsr(col),

            (2, 1, 0) => Control::BassPorta,
            (2, 1, 1) => Control::BassCutoff,
            (2, 1, 2) => Control::BassRes,
            (2, 1, 3) => Control::BassFilterEnv,

            (2, 2, 0) => Control::BassWaveform,
            (2, 2, 1) => Control::BassSub,
            (2, 2, 2) => Control::BassNoise,
            (2, 2, 3) => Control::BassEnv,

            (2, 3, 0) => Control::BassDuty,
            (2, 3, 1) => Control::BassVibrato,
            (2, 3, 2) => Control::BassPitch,
            (2, 3, 3) => Control::BassPitchEnv,

            // Bank D
            (3, 0, col) => Control::SynthAdsr(col),

            (3, 1, 0) => Control::SynthPorta,
            (3, 1, 1) => Control::SynthCutoff,
            (3, 1, 2) => Control::SynthRes,
            (3, 1, 3) => Control::SynthFilterEnv,

            (3, 2, 0) => Control::SynthWaveform,
            (3, 2, 1) => Control::SynthSub,
            (3, 2, 2) => Control::SynthNoise,
            (3, 2, 3) => Control::SynthEnv,

            (3, 3, 0) => Control::SynthDuty,
            (3, 3, 1) => Control::SynthVibrato,
            (3, 3, 2) => Control::SynthPitch,
            (3, 3, 3) => Control::SynthPitchEnv,

            _ => Control::None
        }
    }
}

fn get_control_ids () -> HashMap<Control, u32> {
    let mut result = HashMap::new();
    for id in 0..64 {
        let control = Control::from_id(id);
        if control != Control::None {
            result.insert(control, id);
        }
    }
    result
}

fn midi_to_polar (value: u8) -> f64 {
    if value < 63 {
        (value as f64 - 64.0) / 63.0
    } else if value > 64 {
        (value as f64 - 63.0) / 63.0
    } else {
        0.0
    }
} 

fn midi_to_float (value: u8) -> f64 {
     value as f64 / 127.0
}

fn float_to_midi (value: f64) -> u8 {
    (value * 127.0).max(0.0).min(127.0) as u8
}

pub fn u14_to_msb_lsb(input: u16) -> (u8, u8) {
    let msb = mask7((input >> 7) as u8);
    let lsb = mask7(input as u8);
    (msb, lsb)
}

/// 7 bit mask
#[inline(always)]
pub fn mask7(input: u8) -> u8 {
    input & 0b01111111
}

fn random_range (from: u8, to: u8) -> u8 {
    rand::thread_rng().gen_range(from, to)
}