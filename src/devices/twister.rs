use ::midi_connection;
use std::sync::mpsc;
use ::loop_recorder::{LoopRecorder, LoopEvent};
use ::clock_source::{RemoteClock, FromClock, ToClock, MidiTime};
use ::output_value::OutputValue;
use ::loop_grid_launchpad::LoopGridParams;
use ::audio_recorder::AudioRecorderEvent;
use ::throttled_output::ThrottledOutput;
use std::time::{Duration, Instant};
use ::lfo::Lfo;

use std::thread;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

pub struct Twister {
    _midi_input: midi_connection::ThreadReference
}

impl Twister {
    pub fn new (port_name: &str, kmix_port_name: &str, main_output: midi_connection::SharedMidiOutputConnection, drum_output: midi_connection::SharedMidiOutputConnection, params: Arc<Mutex<LoopGridParams>>, clock: RemoteClock, meta_tx: mpsc::Sender<AudioRecorderEvent>) -> Self {
        let (tx, rx) = mpsc::channel();
        let clock_sender = clock.sender.clone();
        let kmix_port_name = String::from(kmix_port_name);
        let control_ids = get_control_ids();

        let ext_channel = 5; // stereo pair
        let kmix_channel_map: [u8; 4] = [ 1, ext_channel, 2, 3 ];
        let fx_return_channel = 7; // stereo pair

        let mut main_mix_kmix_channels = kmix_channel_map.to_vec();
        main_mix_kmix_channels.push(fx_return_channel);

        let delay_channel = 15;

        let tx_input = tx.clone();
        let tx_clock = tx.clone();
        let tx_feedback = tx.clone();
        let mut align_button_pressed_at = Instant::now();

        // pipe clock in
        thread::spawn(move || {
            for msg in clock.receiver {
                tx_clock.send(TwisterMessage::Clock(msg)).unwrap();
            }
        });

        let mut output = midi_connection::get_shared_output(port_name);

        let input = midi_connection::get_input(port_name, move |_stamp, message| {
            let mut control = Control::from_id(message[1] as u32);
            if message[0] == 176 {
                tx_input.send(TwisterMessage::ControlChange(control, OutputValue::On(message[2]))).unwrap();
            } else if message[0] == 177 {
                if control == Control::Tempo {
                    tx_input.send(TwisterMessage::AlignButton(message[2] > 0)).unwrap()
                } else {
                    tx_input.send(TwisterMessage::Recording(control, message[2] > 0)).unwrap();
                }
            }
        });

        for control in control_ids.keys() {
            tx.send(TwisterMessage::Refresh(*control)).unwrap();
        }

        thread::spawn(move || {
            let mut recorder = LoopRecorder::new();
            let mut last_pos = MidiTime::zero();
            let mut last_values: HashMap<Control, u8> = HashMap::new();
            let mut record_start_times = HashMap::new();
            let mut loops: HashMap<Control, Loop> = HashMap::new();
            let mut main_output = main_output;
            let mut kmix_output = midi_connection::get_shared_output(&kmix_port_name);
            let mut throttled_kmix_output = ThrottledOutput::new(kmix_output);
            let mut throttled_drum_output = ThrottledOutput::new(drum_output.clone());

            let mut synth_env = 0.0;
            let mut synth_attack = 0.0;
            let mut synth_decay = 0.0;
            let mut synth_sustain = 1.0;

            let mut lfo_amounts = HashMap::new();

            let mut lfo = Lfo::new();

            for channel in 0..4 {
                tx_feedback.send(TwisterMessage::Event(LoopEvent { 
                    id: *control_ids.get(&Control::ChannelVolume(channel)).unwrap(), 
                    value: OutputValue::On(100),
                    pos: last_pos
                })).unwrap();
                tx_feedback.send(TwisterMessage::Event(LoopEvent { 
                    id: *control_ids.get(&Control::ChannelReverb(channel)).unwrap(), 
                    value: OutputValue::On(0),
                    pos: last_pos
                })).unwrap();
                tx_feedback.send(TwisterMessage::Event(LoopEvent { 
                    id: *control_ids.get(&Control::ChannelDelay(channel)).unwrap(), 
                    value: OutputValue::On(0),
                    pos: last_pos
                })).unwrap();
                tx_feedback.send(TwisterMessage::Event(LoopEvent { 
                    id: *control_ids.get(&Control::ChannelFilter(channel)).unwrap(), 
                    value: OutputValue::On(64),
                    pos: last_pos
                })).unwrap();
            }

            last_values.insert(Control::LfoHold, lfo.hold);
            last_values.insert(Control::LfoOffset, lfo.offset);
            last_values.insert(Control::LfoSkew, lfo.skew);
            last_values.insert(Control::LfoSpeed, lfo.speed);
            last_values.insert(Control::ReturnVolume, 100);
            last_values.insert(Control::ReturnVolumeLfo, 64);

            last_values.insert(Control::SynthPitch, 64);
            last_values.insert(Control::SynthEnv, 64);
            last_values.insert(Control::SynthPitchEnv, 64);
            last_values.insert(Control::SynthFilterEnv, 64);
            last_values.insert(Control::SynthHighpass, 10);
            last_values.insert(Control::SynthLowpass, 64);
            last_values.insert(Control::SynthAdsr(2), 127);
            last_values.insert(Control::SynthAdsr(3), 64);

            last_values.insert(Control::BassPitch, 64);
            last_values.insert(Control::BassEnv, 64);
            last_values.insert(Control::BassPitchEnv, 64);
            last_values.insert(Control::BassFilterEnv, 64);
            last_values.insert(Control::BassCutoff, 64);
            last_values.insert(Control::BassAdsr(2), 127);

            tx_feedback.send(TwisterMessage::Event(LoopEvent { 
                id: *control_ids.get(&Control::BassAdsr(3)).unwrap(), 
                value: OutputValue::On(64),
                pos: last_pos
            })).unwrap();

            last_values.insert(Control::Tempo, 64);

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
                        if let Some(last_value) = last_values.get(&control) {
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
                                    let kmix_channel = kmix_channel_map[channel as usize % kmix_channel_map.len()];
                                    meta_tx.send(AudioRecorderEvent::ChannelVolume(channel, value)).unwrap();
                                    throttled_kmix_output.send(&[176 + kmix_channel - 1, 1, value]);
                                },
                                Control::ChannelReverb(channel) => {
                                    let kmix_channel = kmix_channel_map[channel as usize % kmix_channel_map.len()];
                                    throttled_kmix_output.send(&[176 + kmix_channel - 1, 23, value]);
                                },
                                Control::ChannelDelay(channel) => {
                                    let kmix_channel = kmix_channel_map[channel as usize % kmix_channel_map.len()];
                                    throttled_kmix_output.send(&[176 + kmix_channel - 1, 25, value]);
                                },
                                Control::ChannelFilter(channel) => {
                                    let kmix_channel = kmix_channel_map[channel as usize % kmix_channel_map.len()];
                                    let lowpass = if value < 64 {
                                        (value * 2)
                                    } else {
                                        127
                                    };
                                    let highpass = if value > 64 {
                                        (value - 64) * 2
                                    } else {
                                        0
                                    };

                                    throttled_kmix_output.send(&[176 + kmix_channel - 1, 5, lowpass]);
                                    throttled_kmix_output.send(&[176 + kmix_channel - 1, 10, highpass]);
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
                                Control::DelayTime => {
                                    main_output.send(&[176 + delay_channel - 1, 2, value]).unwrap();
                                },
                                Control::DelayFeedback => {
                                    main_output.send(&[176 + delay_channel - 1, 5, value]).unwrap();
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
                                Control::ReturnVolume => {
                                    meta_tx.send(AudioRecorderEvent::ChannelVolume(4, value)).unwrap();
                                    throttled_kmix_output.send(&[176 + fx_return_channel - 1, 1, value]);
                                },
                                Control::ReturnVolumeLfo => {
                                    lfo_amounts.insert(Control::ReturnVolume, midi_to_polar(value));
                                },

                                Control::SampleTriggerLength => {

                                },

                                Control::BassDrive => {
                                    main_output.send(&[(176 - 1) + 1, 76, value]).unwrap();
                                },

                                Control::BassChorus => {
                                    main_output.send(&[(208 - 1) + 1, value]).unwrap();
                                },

                                Control::BassAdsr(param) => {
                                    match param {
                                        0 => {
                                            main_output.send(&[(176 - 1) + 1, 14, value]).unwrap();
                                        },
                                        1 => {
                                            main_output.send(&[(176 - 1) + 1, 15, value]).unwrap();
                                        },
                                        2 => {
                                            main_output.send(&[(176 - 1) + 1, 16, value]).unwrap();
                                        },
                                        _ => {
                                            main_output.send(&[(176 - 1) + 1, 78, value]).unwrap();
                                            main_output.send(&[(176 - 1) + 1, 4, 127 - value]).unwrap();
                                        }
                                    }
                                },

                                Control::BassPorta => {
                                    main_output.send(&[(176 - 1) + 1, 5, value]).unwrap();
                                },

                                Control::BassCutoff => {
                                    main_output.send(&[(176 - 1) + 1, 50, value]).unwrap();
                                },

                                Control::BassRes => {
                                    main_output.send(&[(176 - 1) + 1, 56, value]).unwrap();
                                },

                                Control::BassFilterEnv => {
                                    main_output.send(&[(176 - 1) + 1, 52, value]).unwrap();
                                },

                                Control::BassWaveform => {
                                    main_output.send(&[(176 - 1) + 1, 45, 127 - value]).unwrap();
                                    main_output.send(&[(176 - 1) + 1, 46, value]).unwrap();
                                },

                                Control::BassSub => {
                                    main_output.send(&[(176 - 1) + 1, 47, value]).unwrap();
                                },

                                Control::BassNoise => {
                                    main_output.send(&[(176 - 1) + 1, 48, value]).unwrap();
                                },

                                Control::BassEnv => {
                                    let vca_volume = if value > 64 {
                                        100 - ((value - 64) as f32 / 64.0 * 100.0).max(0.0).min(100.0) as u8
                                    } else {
                                        100
                                    };
                                    main_output.send(&[(176 - 1) + 1, 118, value]).unwrap();
                                    main_output.send(&[(176 - 1) + 1, 57, vca_volume]).unwrap();
                                },

                                Control::BassDuty => {
                                    main_output.send(&[(176 - 1) + 1, 35, value]).unwrap();
                                },

                                Control::BassVibrato => {
                                    main_output.send(&[(176 - 1) + 1, 1, value]).unwrap();
                                },

                                Control::BassPitch => {
                                    // hack around detent on mf twister
                                    let value = if value == 63 {
                                        64
                                    } else {
                                        value
                                    };
                                    main_output.send(&[(224 - 1) + 1, 0, value]).unwrap();
                                },

                                Control::BassPitchEnv => {
                                    main_output.send(&[(176 - 1) + 1, 115, value]).unwrap();
                                },

                                Control::SynthDrive => {
                                    main_output.send(&[(176 - 1) + 2, 4, value]).unwrap();
                                },

                                Control::SynthChorus => {
                                    main_output.send(&[(176 - 1) + 2, 93, value]).unwrap();
                                },

                                Control::SynthAdsr(param) => {
                                    match param {
                                        0 => {
                                            main_output.send(&[(176 - 1) + 2, 95, value]).unwrap();
                                            synth_attack = midi_to_float(value);
                                            tx_feedback.send(TwisterMessage::SendSynthEnvelope(0)).unwrap();
                                        },
                                        1 => {
                                            main_output.send(&[(176 - 1) + 2, 96, value]).unwrap();
                                            synth_decay = midi_to_float(value);
                                            tx_feedback.send(TwisterMessage::SendSynthEnvelope(1)).unwrap();
                                        },
                                        2 => {
                                            main_output.send(&[(176 - 1) + 2, 97, value]).unwrap();
                                            synth_sustain = midi_to_float(value);
                                            tx_feedback.send(TwisterMessage::SendSynthEnvelope(2)).unwrap();                                        },
                                        _ => {
                                            main_output.send(&[(176 - 1) + 2, 100, value]).unwrap();
                                            main_output.send(&[(176 - 1) + 2, 106, value]).unwrap();
                                        }
                                    }
                                },

                                Control::SynthHighpass => {
                                    main_output.send(&[(176 - 1) + 2, 80, value]).unwrap();
                                },

                                Control::SynthLowpass => {
                                    main_output.send(&[(176 - 1) + 2, 69, value]).unwrap();
                                },

                                Control::SynthRes => {
                                    main_output.send(&[(176 - 1) + 2, 70, value]).unwrap();
                                },

                                Control::SynthFilterEnv => {
                                    main_output.send(&[(176 - 1) + 2, 73, value]).unwrap();
                                },

                                Control::SynthWaveform => {
                                    main_output.send(&[(176 - 1) + 2, 52, 127 - value]).unwrap();
                                    main_output.send(&[(176 - 1) + 2, 56, value]).unwrap();
                                },

                                Control::SynthSub => {
                                    main_output.send(&[(176 - 1) + 2, 58, value]).unwrap();
                                },

                                Control::SynthNoise => {
                                    main_output.send(&[(176 - 1) + 2, 60, value]).unwrap();
                                },

                                Control::SynthEnv => {
                                    synth_env = midi_to_polar(value);
                                    tx_feedback.send(TwisterMessage::SendSynthEnvelope(0)).unwrap();   
                                    tx_feedback.send(TwisterMessage::SendSynthEnvelope(1)).unwrap();   
                                    tx_feedback.send(TwisterMessage::SendSynthEnvelope(2)).unwrap();   
                                },

                                Control::SynthDuty => {
                                    main_output.send(&[(176 - 1) + 2, 33, value]).unwrap();
                                },

                                Control::SynthVibrato => {
                                    main_output.send(&[(176 - 1) + 2, 1, value]).unwrap();
                                },

                                Control::SynthPitch => {
                                    // hack around detent on mf twister
                                    let value = if value == 63 {
                                        64
                                    } else {
                                        value
                                    };
                                    main_output.send(&[(224 - 1) + 2, 0, value]).unwrap();
                                },

                                Control::SynthPitchEnv => {
                                    // hack around detent on mf twister
                                    let value = if value < 64 {
                                        value + 1
                                    } else {
                                        value
                                    };
                                    main_output.send(&[(176 - 1) + 2, 2, value]).unwrap();
                                },

                                Control::None => ()
                            }

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

                                throttled_kmix_output.flush();
                                throttled_drum_output.flush();

                            },
                            FromClock::Tempo(value) => {
                                tx_feedback.send(TwisterMessage::Refresh(Control::Tempo)).unwrap();
                            },
                            FromClock::Jump => {

                            }
                        }
                    },
                    TwisterMessage::AlignButton(pressed) => {
                        let mut params = params.lock().unwrap();
                        if let Some(id) = control_ids.get(&Control::Tempo) {
                            if pressed {
                                align_button_pressed_at = Instant::now();
                                params.align_offset = last_pos;
                                output.send(&[181, id.clone() as u8, 40]).unwrap();
                            } else {
                                output.send(&[181, id.clone() as u8, 0]).unwrap();
                                if align_button_pressed_at.elapsed() < Duration::from_millis(300) {
                                    // cancel if released immediately
                                    params.align_offset = MidiTime::zero();
                                }
                            }
                        }
                    },

                    TwisterMessage::SendSynthEnvelope(param) => {
                        match param {
                            0 => {
                                let value = float_to_midi(synth_attack * synth_env);
                                main_output.send(&[(176 - 1) + 2, 101, value]).unwrap();
                            },
                            1 => {
                                let value = float_to_midi(synth_decay * synth_env);
                                main_output.send(&[(176 - 1) + 2, 102, value]).unwrap();
                            },
                            _ => {
                                let value = float_to_midi((1.0 - synth_env) + synth_sustain * synth_env);
                                main_output.send(&[(176 - 1) + 2, 103, value]).unwrap();
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

#[derive(Debug)]
enum TwisterMessage {
    ControlChange(Control, OutputValue),
    AlignButton(bool),
    Event(LoopEvent),
    Send(Control),
    SendSynthEnvelope(u8),
    Refresh(Control),
    Recording(Control, bool),
    Clock(FromClock)
}

#[derive(Debug, Eq, PartialEq, Hash, Clone, Copy)]
enum Control {
    ChannelVolume(u32),
    ChannelReverb(u32),
    ChannelDelay(u32),
    ChannelFilter(u32),

    // ChannelVolumeLfo(u32),
    // ChannelReverbLfo(u32),
    // ChannelDelayLfo(u32),
    // ChannelModLfo(u32),

    LfoSkew,
    LfoHold,
    LfoSpeed,
    LfoOffset,

    Tempo,
    Swing,
    DelayTime,
    DelayFeedback,

    ReturnVolume,
    ReturnVolumeLfo,

    SampleTriggerLength,

    BassDrive,
    BassChorus,
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

    SynthDrive,
    SynthChorus,
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
    SynthPitchEnv,


    None
}

#[derive(Debug)]
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
            (0, row, 3) => Control::ChannelFilter(row),

            // Bank B
            (1, 0, 0) => Control::ReturnVolume,
            (1, 0, 1) => Control::ReturnVolumeLfo,
            (1, 0, 2) => Control::Swing,
            (1, 0, 3) => Control::Tempo,

            (1, 1, 1) => Control::SampleTriggerLength,

            (1, 2, 0) => Control::BassDrive,
            (1, 2, 1) => Control::BassChorus,

            (1, 3, 0) => Control::SynthDrive,
            (1, 3, 1) => Control::SynthChorus, 

            (1, 1, 2) => Control::LfoSpeed,
            (1, 1, 3) => Control::LfoSkew,
            (1, 2, 2) => Control::LfoHold,
            (1, 2, 3) => Control::LfoOffset,

            (1, 3, 2) => Control::DelayTime,
            (1, 3, 3) => Control::DelayFeedback,

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