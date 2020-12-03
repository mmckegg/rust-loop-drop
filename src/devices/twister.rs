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
    _midi_input: midi_connection::ThreadReference,
    _midi_input_ju06a: midi_connection::ThreadReference,
    _midi_input_sh01a: midi_connection::ThreadReference,
    tx: mpsc::Sender<TwisterMessage>
}

impl Twister {
    pub fn new (port_name: &str, sh01a_output: midi_connection::SharedMidiOutputConnection, streichfett_output: midi_connection::SharedMidiOutputConnection, drum_output: midi_connection::SharedMidiOutputConnection, blackbox_output: midi_connection::SharedMidiOutputConnection, zoia_output: midi_connection::SharedMidiOutputConnection, dfam_pwm_output: midi_connection::SharedMidiOutputConnection, params: Arc<Mutex<LoopGridParams>>) -> Self {
        let (tx, rx) = mpsc::channel();
        // let clock_sender = clock.sender.clone();
        let control_ids = get_control_ids();

        let sampler_channel = 10;
        let bass_channel = 11;
        let zoia_channel = 14;
        let digit_channel = 15;
        let dividers = vec![
            // 25.0,
            150.0,
            // 200.0
            300.0,
            // 400.0,
            450.0,
            600.0,
            // 800.0,
            900.0,
            1200.0,
            1600.0,
            1800.0,
            2400.0
        ];

        let channel_offsets = [10, 20, 30, 40];

        let tx_input = tx.clone();
        let tx_feedback = tx.clone();
        let tx_clock = tx.clone();

        let mut output = midi_connection::get_shared_output(port_name);

        let input = midi_connection::get_input(port_name, move |_stamp, message| {
            let control = Control::from_id(message[1] as u32);
            if message[0] == 176 {
                tx_input.send(TwisterMessage::ControlChange(control, OutputValue::On(message[2]))).unwrap();
            } else if message[0] == 177 {
                tx_input.send(TwisterMessage::Recording(control, message[2] > 0)).unwrap();
            } else if message[0] == 179 && message[1] < 4 && message[2] == 127 {
                tx_input.send(TwisterMessage::BankChange(message[1])).unwrap();
            } else if message[0] == 179 && (message[1] == 10 || message[1] == 16 || message[1] == 22 || message[1] == 28) {
                tx_input.send(TwisterMessage::LeftButton(message[2] > 0)).unwrap();
            } else if message[0] == 179 && (message[1] == 13 || message[1] == 19 || message[1] == 25 || message[1] == 31) {
                tx_input.send(TwisterMessage::RightButton(message[2] > 0)).unwrap();
            }
        });

        let tx_ju06a = tx.clone();
        let ju06a_input = midi_connection::get_input("JU-06A", move |_stamp, message| {
            if message[0] == 176 {
                if message[1] == 74 { 
                    tx_ju06a.send(TwisterMessage::ControlChange(Control::SynthFilter, OutputValue::On(message[2]))).unwrap();
                } else if message[1] == 71 {
                    tx_ju06a.send(TwisterMessage::ControlChange(Control::SynthRes, OutputValue::On(message[2]))).unwrap();
                } else if message[1] == 13 {
                    tx_ju06a.send(TwisterMessage::ControlChange(Control::SynthVibrato, OutputValue::On(message[2]))).unwrap();
                } else if message[1] == 19 {
                    tx_ju06a.send(TwisterMessage::ControlChange(Control::SynthNoise, OutputValue::On(message[2]))).unwrap();
                }
            }
        });

        let tx_sh01a = tx.clone();
        let sh01a_input = midi_connection::get_input("Boutique", move |_stamp, message| {
            if message[0] == 186 {
                if message[1] == 74 { 
                    tx_sh01a.send(TwisterMessage::ControlChange(Control::BassCutoff, OutputValue::On(message[2]))).unwrap();
                } else if message[1] == 71 {
                    tx_sh01a.send(TwisterMessage::ControlChange(Control::BassRes, OutputValue::On(message[2]))).unwrap();
                } else if message[1] == 5 {
                    tx_sh01a.send(TwisterMessage::ControlChange(Control::BassPorta, OutputValue::On(message[2]))).unwrap();
                } else if message[1] == 23 {
                    tx_sh01a.send(TwisterMessage::ControlChange(Control::BassNoise, OutputValue::On(message[2]))).unwrap();
                } else if message[1] == 76 {
                    tx_sh01a.send(TwisterMessage::ControlChange(Control::BassPitch, OutputValue::On(message[2]))).unwrap();
                }
            }
        });

        thread::spawn(move || {
            let mut recorder = LoopRecorder::new();
            let mut last_pos = MidiTime::zero();
            let mut last_values: HashMap<Control, u8> = HashMap::new();
            let mut record_start_times = HashMap::new();
            let mut loops: HashMap<Control, Loop> = HashMap::new();
            let mut sh01a_output = sh01a_output;
            let mut throttled_blackbox_output = ThrottledOutput::new(blackbox_output);
            let mut throttled_drum_output = ThrottledOutput::new(drum_output);
            let mut throttled_zoia_output = ThrottledOutput::new(zoia_output);
            let mut streichfett_output = streichfett_output;
            let mut dfam_pwm_output = dfam_pwm_output;

            let mut current_bank = 0;

            let mut frozen = false;
            let mut frozen_values = None;
            let mut frozen_loops = None;

            let mut lfo_amounts = HashMap::new();

            let mut lfo = Lfo::new();

            let mut resend_params: Vec<Control> = vec![
                Control::ReverbPre,
                Control::ReverbFeedback,
                Control::ReverbLow,
                Control::ReverbHigh,
                Control::DelayDivider,
                Control::DelayFeedback,
                Control::DelayLow,
                Control::DelayHigh
            ];

            for channel in 0..4 {
                last_values.insert(Control::ChannelVolume(channel), 100);
                last_values.insert(Control::ChannelReverb(channel), 0);
                last_values.insert(Control::ChannelDelay(channel), 0);
                last_values.insert(Control::ChannelFilter(channel), 0);

                resend_params.push(Control::ChannelReverb(channel));
                resend_params.push(Control::ChannelDelay(channel));
                resend_params.push(Control::ChannelFilter(channel));

                last_values.insert(Control::ChannelFilterFollower(channel), 64);
                last_values.insert(Control::ChannelCrush(channel), 0);
                last_values.insert(Control::ChannelRedux(channel), 0);
                last_values.insert(Control::ChannelDrive(channel), 0);
                last_values.insert(Control::ChannelDuck(channel), 64);
            }

            last_values.insert(Control::LfoRate, 64);
            last_values.insert(Control::LfoHold, 0);
            last_values.insert(Control::LfoSkew, 64);
            last_values.insert(Control::LfoOffset, 64);
            last_values.insert(Control::DuckRelease, 64);

            last_values.insert(Control::SamplerMod, 64);

            last_values.insert(Control::BassFilterLfoAmount, 64);
            last_values.insert(Control::SynthFilterLfoAmount, 64);

            last_values.insert(Control::SynthPitch, 64);
            last_values.insert(Control::SynthVibrato, 0);
            last_values.insert(Control::SynthFilter, 60);

            last_values.insert(Control::ReverbPre, 0);
            last_values.insert(Control::ReverbFeedback, 0);
            last_values.insert(Control::ReverbLow, 30);
            last_values.insert(Control::ReverbHigh, 50);

            last_values.insert(Control::DelayDivider, 115);
            last_values.insert(Control::DelayFeedback, 64);
            last_values.insert(Control::DelayLow, 50);
            last_values.insert(Control::DelayHigh, 30);

            
            last_values.insert(Control::SynthFilter, 60);

            last_values.insert(Control::BassPitch, 64);
            last_values.insert(Control::BassCutoff, 40);

            // a wee bit of swing
            last_values.insert(Control::Swing, 64);

            // update display and send all of the start values on load
            for control in control_ids.keys() {
                tx.send(TwisterMessage::Send(*control)).unwrap();
                tx.send(TwisterMessage::Refresh(*control)).unwrap();
            }

            // enable nemesis pedal!
            // throttled_zoia_output.send(&[176 + digit_channel - 1, 38, 127]);

            for received in rx {
                match received {
                    TwisterMessage::LeftButton(pressed) => {
                        let mut params = params.lock().unwrap();
                        params.frozen = pressed;
                    },
                    TwisterMessage::RightButton(pressed) => {
                        let mut params = params.lock().unwrap();
                        params.frozen = pressed;
                    },
                    TwisterMessage::BankChange(bank) => {
                        let mut params = params.lock().unwrap();
                        params.bank = bank;
                    },
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
                            Control::ChannelVolume(channel) => {
                                let cc = channel_offsets[channel as usize % channel_offsets.len()] + 0;
                                throttled_zoia_output.send(&[176 + zoia_channel - 1, cc as u8, value]);
                            },
                            Control::ChannelFilterFollower(channel) => {
                                let cc = channel_offsets[channel as usize % channel_offsets.len()] + 1;
                                throttled_zoia_output.send(&[176 + zoia_channel - 1, cc as u8, value]);
                            },
                            Control::ChannelCrush(channel) => {
                                let cc = channel_offsets[channel as usize % channel_offsets.len()] + 2;
                                throttled_zoia_output.send(&[176 + zoia_channel - 1, cc as u8, value]);
                            },
                            Control::ChannelRedux(channel) => {
                                let cc = channel_offsets[channel as usize % channel_offsets.len()] + 3;
                                throttled_zoia_output.send(&[176 + zoia_channel - 1, cc as u8, value]);
                            },
                            Control::ChannelDrive(channel) => {
                                let cc = channel_offsets[channel as usize % channel_offsets.len()] + 4;
                                throttled_zoia_output.send(&[176 + zoia_channel - 1, cc as u8, value]);
                            },

                            Control::ChannelDuck(channel) => {
                                let cc = channel_offsets[channel as usize % channel_offsets.len()] + 5;
                                throttled_zoia_output.send(&[176 + zoia_channel - 1, cc as u8, value]);
                            },

                            Control::ChannelReverb(channel) => {
                                let cc = channel_offsets[channel as usize % channel_offsets.len()] + 0;
                                throttled_zoia_output.send(&[176 + digit_channel - 1, cc as u8, midi_ease_out(value)]);
                            },
                            Control::ChannelDelay(channel) => {
                                let cc = channel_offsets[channel as usize % channel_offsets.len()] + 1;
                                throttled_zoia_output.send(&[176 + digit_channel - 1, cc as u8, midi_ease_out(value)]);
                            },
                            Control::ChannelFilter(channel) => {
                                let cc = channel_offsets[channel as usize % channel_offsets.len()] + 2;
                                throttled_zoia_output.send(&[176 + digit_channel - 1, cc as u8, value]);
                            },

                            Control::ReverbPre => {
                                throttled_zoia_output.send(&[176 + digit_channel - 1, 60, value]);
                            },

                            Control::ReverbFeedback => {
                                throttled_zoia_output.send(&[176 + digit_channel - 1, 61, value]);
                            },

                            Control::ReverbLow=> {
                                throttled_zoia_output.send(&[176 + digit_channel - 1, 62, value]);
                            },

                            Control::ReverbHigh => {
                                throttled_zoia_output.send(&[176 + digit_channel - 1, 63, value]);
                            },

                            Control::DelayDivider => {
                                let index = (midi_to_float(value) * (dividers.len() - 1) as f64 ) as usize;
                                let divider = dividers[index];
                                let value = if divider > 0.0 {
                                    float_to_midi(divider / 2400.0)
                                } else {
                                    0
                                };
                                throttled_zoia_output.send(&[176 + digit_channel - 1, 70, value]);
                            },

                            Control::DelayFeedback => {
                                throttled_zoia_output.send(&[176 + digit_channel - 1, 71, value]);
                            },

                            Control::DelayLow=> {
                                let value = if value == 63 {
                                    64
                                } else {
                                    value
                                };
                                throttled_zoia_output.send(&[176 + digit_channel - 1, 72, value]);
                            },

                            Control::DelayHigh => {
                                throttled_zoia_output.send(&[176 + digit_channel - 1, 73, value]);
                            },

                            Control::DrumMod => {
                                let value = if value == 63 {
                                    64
                                } else {
                                    value
                                };
                                throttled_blackbox_output.send(&[176 + sampler_channel - 1, 1, value]);
                                dfam_pwm_output.send(&[176, 1, value]);
                            },
                            
                            Control::SamplerMod => {
                                let value = if value == 63 {
                                    64
                                } else {
                                    value
                                };
                                throttled_blackbox_output.send(&[176 + sampler_channel - 1, 0, value]);
                            },

                            Control::BassFilterLfoAmount => {
                                lfo_amounts.insert(Control::BassCutoff, midi_to_polar(value));
                            },

                            Control::SynthFilterLfoAmount => {
                                lfo_amounts.insert(Control::SynthFilter, midi_to_polar(value));
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
                            Control::LfoRate => {
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
                            Control::DuckRelease => {
                                throttled_zoia_output.send(&[(176 - 1) + zoia_channel, 2, value]);
                            },

                            Control::BassPorta => {
                                sh01a_output.send(&[(176 - 1) + bass_channel, 5, value]).unwrap();
                            },

                            Control::BassCutoff => {
                                sh01a_output.send(&[(176 - 1) + bass_channel, 74, value]).unwrap();
                            },

                            Control::BassRes => {
                                sh01a_output.send(&[(176 - 1) + bass_channel, 71, value]).unwrap();
                            },

                            Control::BassNoise => {
                                sh01a_output.send(&[(176 - 1) + bass_channel, 23, value]).unwrap();
                            },

                            Control::BassPitch => {
                                // hack around detent on mf twister
                                let value = if value == 63 {
                                    64
                                } else {
                                    value
                                };
                                sh01a_output.send(&[(176 - 1) + bass_channel, 76, value]).unwrap();
                                throttled_blackbox_output.send(&[(224 - 1) + bass_channel, 0, value]);
                            },


                            Control::SynthFilter => {
                                // streichfett_output.send(&[176, 74, value]);
                            },

                            Control::SynthRes => {
                                // streichfett_output.send(&[176, 71, value]);
                            },


                            Control::SynthNoise => {
                                // streichfett_output.send(&[176, 19, value]);
                            },


                            Control::SynthVibrato => {
                                // streichfett_output.send(&[176, 13, value]);
                            },

                            Control::SynthPitch => {
                                // hack around detent on mf twister
                                let value = if value == 63 {
                                    64
                                } else {
                                    value
                                };
                                streichfett_output.send(&[224, 0, value]);
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

                    TwisterMessage::Schedule { pos, length } => {
                        let mut params = params.lock().unwrap();
                        if params.reset_automation {
                            // HACK: ack reset message from clear all
                            params.reset_automation = false;
                            loops.clear();

                            for control in control_ids.keys() {
                                tx.send(TwisterMessage::Refresh(*control)).unwrap();
                            }
                        }

                        // resend poly params every 32 beats (delayed by 1 tick to prevent holding up down beat scheduling)
                        if pos % MidiTime::from_beats(32) == MidiTime::from_ticks(1) {
                            for control in &resend_params {
                                tx.send(TwisterMessage::Send(*control)).unwrap();
                            }
                        }

                        if current_bank != params.bank {
                            output.send(&[179, params.bank, 127]).unwrap();
                            current_bank = params.bank;
                        }

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

                        throttled_blackbox_output.flush();
                        throttled_zoia_output.flush();
                    }
                }
            }
        });

        Twister {
            _midi_input: input,
            _midi_input_ju06a: ju06a_input,
            _midi_input_sh01a: sh01a_input,
            tx: tx_clock
        }
    }

    pub fn schedule (&self, pos: MidiTime, length: MidiTime) {
        self.tx.send(TwisterMessage::Schedule {pos, length}).unwrap();
    }
}

#[derive(Debug, Clone)]
enum TwisterMessage {
    ControlChange(Control, OutputValue),
    BankChange(u8),
    Event(LoopEvent),
    Send(Control),
    Refresh(Control),
    Recording(Control, bool),
    LeftButton(bool),
    RightButton(bool),
    Schedule { pos: MidiTime, length: MidiTime }
}

#[derive(Debug, Eq, PartialEq, Hash, Clone, Copy)]
enum Control {
    ChannelVolume(u32),
    ChannelReverb(u32),
    ChannelDelay(u32),
    ChannelFilter(u32),
    
    ChannelCrush(u32),
    ChannelRedux(u32),
    ChannelDrive(u32),
    ChannelFilterFollower(u32),
    ChannelDuck(u32),

    DuckRelease,

    ReverbPre,
    ReverbFeedback,
    ReverbLow,
    ReverbHigh,

    DelayDivider,
    DelayFeedback,
    DelayLow,
    DelayHigh,

    DrumMod,

    SamplerMod,
    
    Swing,

    LfoRate,
    LfoHold,
    LfoSkew,
    LfoOffset,
    
    BassCutoff,
    BassRes,
    BassFilterLfoAmount,
    BassPitch,
    BassPorta,
    BassNoise,
        
    SynthFilter,
    SynthRes,
    SynthFilterLfoAmount,
    SynthPitch,
    SynthVibrato,
    SynthNoise,

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
            (0, row, 0) => Control::ChannelVolume(row),
            (0, row, 1) => Control::ChannelReverb(row),
            (0, row, 2) => Control::ChannelDelay(row),
            (1, row, 3) => Control::ChannelFilter(row),

            // Bank B
            (1, row, 0) => Control::ChannelCrush(row),
            (1, row, 1) => Control::ChannelRedux(row),
            (1, row, 2) => Control::ChannelDrive(row),
            (0, row, 3) => Control::ChannelFilterFollower(row),

            // Bank C
            (2, 0, 0) => Control::BassCutoff,
            (2, 0, 1) => Control::BassRes,
            (2, 0, 2) => Control::BassFilterLfoAmount,

            (2, 1, 0) => Control::BassPitch,
            (2, 1, 1) => Control::BassPorta,
            (2, 1, 2) => Control::BassNoise,

            (2, 2, 0) => Control::SynthFilter,
            (2, 2, 1) => Control::SynthRes,
            (2, 2, 2) => Control::SynthFilterLfoAmount,

            (2, 3, 0) => Control::SynthPitch,
            (2, 3, 1) => Control::SynthVibrato,
            (2, 3, 2) => Control::SynthNoise,

            (2, 0, 3) => Control::LfoRate,
            (2, 1, 3) => Control::LfoHold,
            (2, 2, 3) => Control::LfoSkew,
            (2, 3, 3) => Control::LfoOffset,


            // Bank D
            (3, row, 0) => Control::ChannelDuck(row),

            (3, 0, 1) => Control::ReverbPre,
            (3, 0, 2) => Control::ReverbFeedback,
            (3, 1, 1) => Control::ReverbLow,
            (3, 1, 2) => Control::ReverbHigh,

            (3, 2, 1) => Control::DelayDivider,
            (3, 2, 2) => Control::DelayFeedback,
            (3, 3, 1) => Control::DelayLow,
            (3, 3, 2) => Control::DelayHigh,

            (3, 0, 3) => Control::Swing,
            (3, 1, 3) => Control::DuckRelease,
            (3, 2, 3) => Control::DrumMod,
            (3, 3, 3) => Control::SamplerMod,

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
        (value as f64 - 63.0) / 63.0
    } else if value > 64 {
        (value as f64 - 64.0) / 63.0
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

fn random_range (from: u8, to: u8) -> u8 {
    rand::thread_rng().gen_range(from, to)
}

fn midi_ease_out (value: u8) -> u8 {
    let f = midi_to_float(value);
    float_to_midi(f * (2.0 - f))
}