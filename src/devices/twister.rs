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
    pub fn new (port_name: &str, pulse_output: midi_connection::SharedMidiOutputConnection, blofeld_output: midi_connection::SharedMidiOutputConnection, blackbox_output: midi_connection::SharedMidiOutputConnection, zoia_output: midi_connection::SharedMidiOutputConnection, params: Arc<Mutex<LoopGridParams>>, clock: RemoteClock) -> Self {
        let (tx, rx) = mpsc::channel();
        let clock_sender = clock.sender.clone();
        let control_ids = get_control_ids();

        let drums_channel = 1;
        let slicer_channel = 2;
        let sampler_channel = 6;

        let bass_channel = 11;
        let synth_channel = 12;
        let ext_channel = 13;
        let zoia_channel = 14;
        let pedal_channel = 15;

        let channel_offsets = [10, 20, 30, 40];

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
            } else if message[0] == 179 && message[1] < 4 && message[2] == 127 {
                tx_input.send(TwisterMessage::BankChange(message[1])).unwrap();
            }
        });

        thread::spawn(move || {
            let mut recorder = LoopRecorder::new();
            let mut last_pos = MidiTime::zero();
            let mut last_values: HashMap<Control, u8> = HashMap::new();
            let mut record_start_times = HashMap::new();
            let mut loops: HashMap<Control, Loop> = HashMap::new();
            let mut throttled_pulse_output = ThrottledOutput::new(pulse_output);
            let mut throttled_blofeld_output = ThrottledOutput::new(blofeld_output);
            let mut throttled_blackbox_output = ThrottledOutput::new(blackbox_output);
            let mut throttled_zoia_output = ThrottledOutput::new(zoia_output);

            let mut synth_env = 0.0;
            let mut synth_attack = 0.0;
            let mut synth_decay = 0.0;
            let mut synth_sustain = 1.0;
            let mut last_delay_division = None;

            let mut current_bank = 0;

            let mut bass_env = 0.0;
            let mut bass_attack = 0.0;
            let mut bass_decay = 0.0;
            let mut bass_sustain = 1.0;
            let mut bass_volume = 127;
            let mut bass_volume_multiplier = 1.0;

            let mut frozen = false;
            let mut frozen_values = None;
            let mut frozen_loops = None;

            let mut lfo_amounts = HashMap::new();

            let mut lfo = Lfo::new();

            for channel in 0..4 {
                last_values.insert(Control::ChannelVolume(channel), 100);
                last_values.insert(Control::ChannelFilter(channel), 64);
                last_values.insert(Control::ChannelCrush(channel), 0);
                last_values.insert(Control::ChannelRedux(channel), 0);
                last_values.insert(Control::ChannelChorus(channel), 0);
            }

            last_values.insert(Control::DrumSend, 0);
            last_values.insert(Control::SlicerSend, 0);
            last_values.insert(Control::SamplerSend, 0);
            last_values.insert(Control::BassSend, 0);
            last_values.insert(Control::SynthSend, 0);
            last_values.insert(Control::ExtSend, 0);

            last_values.insert(Control::LfoShape, 0);
            last_values.insert(Control::LfoRate, 64);

            last_values.insert(Control::DrumMod, 64);
            last_values.insert(Control::ExtMod, 64);
            last_values.insert(Control::SlicerMod, 64);

            last_values.insert(Control::BassFilterLfoAmount, 64);
            last_values.insert(Control::SynthFilterLfoAmount, 64);

            last_values.insert(Control::SynthPitch, 64);
            last_values.insert(Control::SynthVibrato, 40);
            last_values.insert(Control::SynthEnv, 100);
            last_values.insert(Control::SynthPitchOffset, 64);
            last_values.insert(Control::SynthFilterEnv, 80);
            last_values.insert(Control::SynthHighpass, 50);
            last_values.insert(Control::SynthLowpass, 60);
            last_values.insert(Control::SynthDuty, 127);
            last_values.insert(Control::SynthAdsr(0), 64);
            last_values.insert(Control::SynthAdsr(1), 75);
            last_values.insert(Control::SynthAdsr(2), 110);
            last_values.insert(Control::SynthAdsr(3), 70);

            last_values.insert(Control::BassDrive, 0);
            last_values.insert(Control::KickDuckAmount, 64);

            last_values.insert(Control::BassPitch, 64);
            last_values.insert(Control::BassEnv, 64);
            last_values.insert(Control::BassPitchOffset, 64);
            last_values.insert(Control::BassFilterEnv, 80);
            last_values.insert(Control::BassCutoff, 40);
            last_values.insert(Control::BassSub, 127);
            last_values.insert(Control::BassDuty, 127);
            last_values.insert(Control::BassPitchOffset, 102);
            last_values.insert(Control::BassAdsr(1), 50);
            last_values.insert(Control::BassAdsr(2), 30);
            last_values.insert(Control::BassAdsr(3), 64);

            last_values.insert(Control::Tempo, random_range(20, 80));
            last_values.insert(Control::DelayDivision, 62);
            last_values.insert(Control::Swing, 64);

            // update display and send all of the start values on load
            for control in control_ids.keys() {
                tx.send(TwisterMessage::Send(*control)).unwrap();
                tx.send(TwisterMessage::Refresh(*control)).unwrap();
            }

            // enable nemesis pedal!
            throttled_zoia_output.send(&[176 + pedal_channel - 1, 38, 127]);

            for received in rx {
                match received {
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
                            Control::ChannelFilter(channel) => {
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

                            Control::ChannelChorus(channel) => {
                                let cc = channel_offsets[channel as usize % channel_offsets.len()] + 4;
                                throttled_zoia_output.send(&[176 + zoia_channel - 1, cc as u8, value]);
                            },

                            Control::DrumSend => {
                                throttled_blackbox_output.send(&[176 + drums_channel - 1, 10, value / 2]);
                            },
                            
                            Control::SlicerSend => {
                                throttled_blackbox_output.send(&[176 + slicer_channel - 1, 10, value / 2]);
                                throttled_blackbox_output.send(&[176 + slicer_channel - 1 + 1, 10, value / 2]);
                            },
                            Control::SamplerSend => {
                                throttled_blackbox_output.send(&[176 + sampler_channel - 1, 10, value / 2]);
                            },
                            Control::DrumMod => {
                                let value = if value == 63 {
                                    64
                                } else {
                                    value
                                };
                                throttled_blackbox_output.send(&[(224 - 1) + drums_channel, 0, value]);
                            },
                            Control::SlicerMod => {
                                throttled_blackbox_output.send(&[(176 - 1) + slicer_channel, 1, value]);
                            },
                            Control::BassSend => {
                                bass_volume_multiplier = 0.7 + (midi_to_float(value) * 0.3);
                                throttled_pulse_output.send(&[176 + bass_channel - 1, 10, value / 2]);
                                throttled_blackbox_output.send(&[176 + bass_channel - 1, 10, value / 2]);
                                throttled_pulse_output.send(&[(176 - 1) + bass_channel, 57, (bass_volume as f64 * bass_volume_multiplier) as u8]);
                            },
                            Control::SynthSend => {
                                throttled_blofeld_output.send(&[176 + synth_channel - 1, 77, value / 2]);
                                throttled_blofeld_output.send(&[176 + synth_channel - 1, 88, value / 2]);
                            },
                            Control::ExtSend => {
                                throttled_blofeld_output.send(&[176 + ext_channel - 1, 77, value / 2]);
                                throttled_blofeld_output.send(&[176 + ext_channel - 1, 88, value / 2]);
                            },
                            Control::ExtMod => {
                                throttled_blofeld_output.send(&[(208 - 1) + ext_channel, value]);
                            },

                            Control::BassFilterLfoAmount => {
                                lfo_amounts.insert(Control::BassCutoff, midi_to_polar(value));
                            },

                            Control::SynthFilterLfoAmount => {
                                lfo_amounts.insert(Control::SynthLowpass, midi_to_polar(value));
                            },

                            Control::Tempo => {
                                clock_sender.send(ToClock::SetTempo(value as usize + 60)).unwrap();
                            },
                            Control::DelayDivision => {
                                let value = midi_to_float(value);
                                let new_value = (value * 14.0) as u8;
                                if last_delay_division != Some(new_value) {
                                    println!("Set division {}", new_value);
                                    throttled_zoia_output.send(&[176 + pedal_channel - 1, 42, new_value]);
                                    last_delay_division = Some(new_value);
                                }
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
                            Control::LfoShape => {
                                lfo.skew = value;
                                lfo.hold = value;
                                lfo.offset = value;
                            },
                            Control::KickDuckAmount => {
                                throttled_zoia_output.send(&[(176 - 1) + zoia_channel, 2, value]);
                            },

                            Control::BassDrive => {
                                throttled_pulse_output.send(&[(176 - 1) + bass_channel, 76, value]);
                            },

                            Control::BassAdsr(param) => {
                                match param {
                                    0 => {
                                        throttled_pulse_output.send(&[(176 - 1) + bass_channel, 14, value]);
                                    },
                                    1 => {
                                        throttled_pulse_output.send(&[(176 - 1) + bass_channel, 15, value]);
                                    },
                                    2 => {
                                        throttled_pulse_output.send(&[(176 - 1) + bass_channel, 16, value]);
                                    },
                                    _ => {
                                        throttled_pulse_output.send(&[(176 - 1) + bass_channel, 78, value]);
                                        throttled_pulse_output.send(&[(176 - 1) + bass_channel, 4, 127 - value]);
                                    }
                                }
                            },

                            Control::BassPorta => {
                                throttled_pulse_output.send(&[(176 - 1) + bass_channel, 5, value]);
                            },

                            Control::BassCutoff => {
                                throttled_pulse_output.send(&[(176 - 1) + bass_channel, 50, value]);
                            },

                            Control::BassRes => {
                                throttled_pulse_output.send(&[(176 - 1) + bass_channel, 56, value]);
                            },

                            Control::BassFilterEnv => {
                                throttled_pulse_output.send(&[(176 - 1) + bass_channel, 52, value]);
                            },

                            Control::BassWaveform => {
                                throttled_pulse_output.send(&[(176 - 1) + bass_channel, 45, 127 - value]);
                                throttled_pulse_output.send(&[(176 - 1) + bass_channel, 46, value]);
                            },

                            Control::BassSub => {
                                throttled_pulse_output.send(&[(176 - 1) + bass_channel, 47, value]);
                            },

                            Control::BassNoise => {
                                throttled_pulse_output.send(&[(176 - 1) + bass_channel, 48, value]);
                            },

                            Control::BassEnv => {
                                bass_volume = if value > 64 {
                                    100 - ((value - 64) as f32 / 64.0 * 100.0).max(0.0).min(100.0) as u8
                                } else {
                                    100
                                };
                                throttled_pulse_output.send(&[(176 - 1) + bass_channel, 118, value]);
                                throttled_pulse_output.send(&[(176 - 1) + bass_channel, 57, (bass_volume as f64 * bass_volume_multiplier) as u8]);
                            },

                            Control::BassDuty => {
                                throttled_pulse_output.send(&[(176 - 1) + bass_channel, 35, value]);
                            },

                            Control::BassVibrato => {
                                throttled_pulse_output.send(&[(176 - 1) + bass_channel, 1, value]);
                            },

                            Control::BassPitch => {
                                // hack around detent on mf twister
                                let value = if value == 63 {
                                    64
                                } else {
                                    value
                                };
                                throttled_pulse_output.send(&[(224 - 1) + bass_channel, 0, value]);
                            },

                            Control::BassPitchOffset => {
                                // oscillator 2 semitone
                                let value = midi_to_polar(value) * 12.0;
                                throttled_pulse_output.send(&[(176 - 1) + bass_channel, 36, (value + 64.0) as u8 ]);
                            },

                            Control::SynthAdsr(param) => {
                                match param {
                                    0 => {
                                        throttled_blofeld_output.send(&[(176 - 1) + synth_channel, 95, value]);
                                        synth_attack = midi_to_float(value);
                                        tx_feedback.send(TwisterMessage::SendSynthEnvelope(0)).unwrap();
                                    },
                                    1 => {
                                        throttled_blofeld_output.send(&[(176 - 1) + synth_channel, 96, value]);
                                        synth_decay = midi_to_float(value);
                                        tx_feedback.send(TwisterMessage::SendSynthEnvelope(1)).unwrap();
                                    },
                                    2 => {
                                        throttled_blofeld_output.send(&[(176 - 1) + synth_channel, 97, value]);
                                        synth_sustain = midi_to_float(value);
                                        tx_feedback.send(TwisterMessage::SendSynthEnvelope(2)).unwrap();
                                    },
                                    _ => {
                                        throttled_blofeld_output.send(&[(176 - 1) + synth_channel, 100, value]);
                                        throttled_blofeld_output.send(&[(176 - 1) + synth_channel, 106, value]);
                                    }
                                }
                            },

                            Control::SynthHighpass => {
                                throttled_blofeld_output.send(&[(176 - 1) + synth_channel, 80, value]);
                            },

                            Control::SynthLowpass => {
                                throttled_blofeld_output.send(&[(176 - 1) + synth_channel, 69, value]);
                            },

                            Control::SynthRes => {
                                throttled_blofeld_output.send(&[(176 - 1) + synth_channel, 70, value]);
                            },

                            Control::SynthFilterEnv => {
                                throttled_blofeld_output.send(&[(176 - 1) + synth_channel, 73, value]);
                            },

                            Control::SynthWaveform => {
                                throttled_blofeld_output.send(&[(176 - 1) + synth_channel, 52, 127 - value]);
                                throttled_blofeld_output.send(&[(176 - 1) + synth_channel, 56, value]);
                            },

                            Control::SynthSub => {
                                throttled_blofeld_output.send(&[(176 - 1) + synth_channel, 58, value]);
                            },

                            Control::SynthNoise => {
                                throttled_blofeld_output.send(&[(176 - 1) + synth_channel, 60, value]);
                            },

                            Control::SynthEnv => {
                                synth_env = midi_to_polar(value);
                                tx_feedback.send(TwisterMessage::SendSynthEnvelope(0)).unwrap();   
                                tx_feedback.send(TwisterMessage::SendSynthEnvelope(1)).unwrap();   
                                tx_feedback.send(TwisterMessage::SendSynthEnvelope(2)).unwrap();   
                            },

                            Control::SynthDuty => {
                                throttled_blofeld_output.send(&[(176 - 1) + synth_channel, 33, value]);
                            },

                            Control::SynthVibrato => {
                                throttled_blofeld_output.send(&[(176 - 1) + synth_channel, 1, value]);
                            },

                            Control::SynthPitch => {
                                // hack around detent on mf twister
                                let value = if value == 63 {
                                    64
                                } else {
                                    value
                                };
                                throttled_blofeld_output.send(&[(224 - 1) + synth_channel, 0, value]);
                            },

                            Control::SynthPitchOffset => {
                                // controlling oscillator 2 semitone instead!
                                let value = midi_to_polar(value) * 12.0;
                                throttled_blofeld_output.send(&[(176 - 1) + synth_channel, 36, (value + 64.0) as u8]);
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

                                if current_bank != params.bank {
                                    output.send(&[179, params.bank, 127]).unwrap();
                                    current_bank = params.bank;
                                }

                                // emit beat tick for tap delay tempo
                                if pos % MidiTime::from_beats(1) == MidiTime::zero() {
                                    let value = if pos % MidiTime::from_beats(2) == MidiTime::zero() {
                                        127 // rise
                                    } else {
                                        0 // fall
                                    };
                                    throttled_zoia_output.send(&[176 - 1 + zoia_channel, 1, value])
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

                                throttled_pulse_output.flush();
                                throttled_blofeld_output.flush();
                                throttled_blackbox_output.flush();
                                throttled_zoia_output.flush();

                            },
                            FromClock::Tempo(value) => {
                                tx_feedback.send(TwisterMessage::Refresh(Control::Tempo)).unwrap();
                            },
                            FromClock::Jump => {

                            }
                        }
                    },

                    TwisterMessage::SendSynthEnvelope(param) => {
                        match param {
                            0 => {
                                let value = float_to_midi(synth_attack * synth_env);
                                throttled_blofeld_output.send(&[(176 - 1) + synth_channel, 101, value]);
                            },
                            1 => {
                                let value = float_to_midi(synth_decay * synth_env);
                                throttled_blofeld_output.send(&[(176 - 1) + synth_channel, 102, value]);
                            },
                            _ => {
                                let value = float_to_midi((1.0 - synth_env) + synth_sustain * synth_env);
                                throttled_blofeld_output.send(&[(176 - 1) + synth_channel, 103, value]);
                            }
                        }
                    },

                    TwisterMessage::SendBassEnvelope(param) => {
                        match param {
                            0 => {
                                let value = float_to_midi(bass_attack * bass_env);
                                throttled_pulse_output.send(&[(176 - 1) + bass_channel, 101, value]);
                            },
                            1 => {
                                let value = float_to_midi(bass_decay * bass_env);
                                throttled_pulse_output.send(&[(176 - 1) + bass_channel, 102, value]);
                            },
                            _ => {
                                let value = float_to_midi((1.0 - bass_env) + bass_sustain * bass_env);
                                throttled_pulse_output.send(&[(176 - 1) + bass_channel, 103, value]);
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
    BankChange(u8),
    Event(LoopEvent),
    Send(Control),
    SendSynthEnvelope(u8),
    SendBassEnvelope(u8),
    Refresh(Control),
    Recording(Control, bool),
    Clock(FromClock)
}

#[derive(Debug, Eq, PartialEq, Hash, Clone, Copy)]
enum Control {
    ChannelVolume(u32),
    ChannelFilter(u32),
    ChannelCrush(u32),
    ChannelRedux(u32),
    ChannelChorus(u32),

    DrumSend,
    SlicerSend,
    SamplerSend,
    BassSend,
    SynthSend,
    ExtSend,

    KickDuckAmount,
    BassDrive,

    DrumMod,
    ExtMod,
    SlicerMod,
    BassFilterLfoAmount,
    SynthFilterLfoAmount,

    Tempo,
    DelayDivision,
    Swing,
    LfoShape,
    LfoRate,


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
    BassPitchOffset,


    SynthAdsr(u32),
    SynthHighpass,
    SynthLowpass,
    SynthRes,
    SynthFilterEnv,
    SynthWaveform,
    SynthSub,
    SynthNoise,
    SynthEnv,
    SynthDuty,
    SynthPitch,
    SynthVibrato,
    SynthPitchOffset,

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
            (0, row, 3) => Control::ChannelFilter(row),

            (0, 0, 1) => Control::DrumSend,
            (0, 1, 1) => Control::SlicerSend,
            (0, 2, 1) => Control::BassSend,
            (0, 3, 1) => Control::SynthSend,

            (0, 0, 2) => Control::KickDuckAmount,
            (0, 1, 2) => Control::SamplerSend,
            (0, 2, 2) => Control::ExtMod,
            (0, 3, 2) => Control::ExtSend,

            // Bank B

            (1, 0, 3) => Control::DelayDivision,
            (1, 1, 3) => Control::Swing,

            (1, row, 0) => Control::ChannelCrush(row),
            (1, row, 1) => Control::ChannelRedux(row),

            (1, 0, 2) => Control::DrumMod,
            (1, 1, 2) => Control::SlicerMod,

            (1, 2, 2) => Control::BassFilterLfoAmount,
            (1, 3, 2) => Control::SynthFilterLfoAmount,

            (1, 2, 3) => Control::LfoRate,
            (1, 3, 3) => Control::LfoShape,

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
            (2, 3, 3) => Control::BassPitchOffset,

            // Bank D
            (3, 0, col) => Control::SynthAdsr(col),

            (3, 1, 0) => Control::SynthHighpass,
            (3, 1, 1) => Control::SynthLowpass,
            (3, 1, 2) => Control::SynthRes,
            (3, 1, 3) => Control::SynthFilterEnv,

            (3, 2, 0) => Control::SynthWaveform,
            (3, 2, 1) => Control::SynthSub,
            (3, 2, 2) => Control::SynthNoise,
            (3, 2, 3) => Control::SynthEnv,

            (3, 3, 0) => Control::SynthDuty,
            (3, 3, 1) => Control::SynthVibrato,
            (3, 3, 2) => Control::SynthPitch,
            (3, 3, 3) => Control::SynthPitchOffset,

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