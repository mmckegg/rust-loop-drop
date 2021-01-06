use ::midi_connection;
use ::MidiTime;
use std::sync::mpsc;
use ::loop_recorder::{LoopRecorder, LoopEvent};
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
    _midi_input_streichfett: midi_connection::ThreadReference,
    _midi_input_tr6s: midi_connection::ThreadReference,
    _midi_input_nts1: midi_connection::ThreadReference,
    tx: mpsc::Sender<TwisterMessage>
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum EventSource {
    Internal,
    External
}

impl Twister {
    pub fn new (port_name: &str, typhon_output: midi_connection::SharedMidiOutputConnection, streichfett_output: midi_connection::SharedMidiOutputConnection, drum_output: midi_connection::SharedMidiOutputConnection, blackbox_output: midi_connection::SharedMidiOutputConnection, nts1_output: midi_connection::SharedMidiOutputConnection, zoia_output: midi_connection::SharedMidiOutputConnection, cv1_output: midi_connection::SharedMidiOutputConnection, cv2_output: midi_connection::SharedMidiOutputConnection, params: Arc<Mutex<LoopGridParams>>) -> Self {
        let (tx, rx) = mpsc::channel();
        // let clock_sender = clock.sender.clone();
        let control_ids = get_control_ids();

        let drum_channel = 10;
        let sampler_channel = 10;

        // no triggers on this channel, only modulation
        let sampler_mod_channel = 3;
        
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
            // 1600.0,
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
                tx_input.send(TwisterMessage::ControlChange(control, OutputValue::On(message[2]), EventSource::Internal)).unwrap();
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

        let tx_sf = tx.clone();
        let streichfett_input = midi_connection::get_input("Streichfett", move |_stamp, message| {
            if message[0] == 176 {
                if message[1] == 76 { 
                    tx_sf.send(TwisterMessage::ControlChange(Control::SynthTone, OutputValue::On(message[2]), EventSource::External)).unwrap();
                } else if message[1] == 70 { 
                    tx_sf.send(TwisterMessage::ControlChange(Control::SynthRegistration, OutputValue::On(message[2]), EventSource::External)).unwrap();
                } else if message[1] == 1 { 
                    tx_sf.send(TwisterMessage::ControlChange(Control::SynthMod, OutputValue::On(message[2]), EventSource::External)).unwrap();
                }
            }
        });

        let tx_tr6s = tx.clone();
        let tr6s_input = midi_connection::get_input("TR-6S", move |_stamp, message| {
            if message[0] == 176 - 1 + drum_channel {
                if message[1] == 20 { 
                    tx_tr6s.send(TwisterMessage::ControlChange(Control::KickPitch, OutputValue::On(message[2]), EventSource::External)).unwrap();
                } else if message[1] == 23 {
                    tx_tr6s.send(TwisterMessage::ControlChange(Control::KickDecay, OutputValue::On(message[2]), EventSource::External)).unwrap();
                }
            }
        });

        let tx_nts1 = tx.clone();
        let nts1_input = midi_connection::get_input("NTS-1 digital kit", move |_stamp, message| {
            if message[0] == 176 {
                if message[1] == 43 { 
                    tx_nts1.send(TwisterMessage::ControlChange(Control::ExtFilter, OutputValue::On(message[2]), EventSource::External)).unwrap();
                }
            }
        });

        thread::spawn(move || {
            let mut recorder = LoopRecorder::new();
            let mut last_pos = MidiTime::zero();
            let mut last_values: HashMap<Control, u8> = HashMap::new();
            let mut record_start_times = HashMap::new();
            let mut loops: HashMap<Control, Loop> = HashMap::new();
            let mut typhon_output = typhon_output;
            let mut drum_output = drum_output;
            let mut nts1_output = nts1_output;
            let mut throttled_blackbox_output = ThrottledOutput::new(blackbox_output);
            let mut throttled_zoia_output = ThrottledOutput::new(zoia_output);
            let mut streichfett_output = streichfett_output;
            let mut cv1_output = cv1_output;
            let mut cv2_output = cv2_output;

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
                last_values.insert(Control::ChannelFilterLfoAmount(channel), 64);

                resend_params.push(Control::ChannelReverb(channel));
                resend_params.push(Control::ChannelDelay(channel));
                resend_params.push(Control::ChannelFilter(channel));

                last_values.insert(Control::ChannelFilter(channel), 64);
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

            last_values.insert(Control::SamplerPitch, 64);

            last_values.insert(Control::Cv1, 64);
            last_values.insert(Control::Cv2, 0);
            last_values.insert(Control::BassFilterLfoAmount, 64);
            last_values.insert(Control::ExtFilterLfoAmount, 64);
            last_values.insert(Control::Cv2LfoAmount, 64);

            last_values.insert(Control::SynthPitch, 64);
            last_values.insert(Control::SynthTone, 60);

            last_values.insert(Control::ReverbPre, 0);
            last_values.insert(Control::ReverbFeedback, 0);
            last_values.insert(Control::ReverbLow, 30);
            last_values.insert(Control::ReverbHigh, 50);

            last_values.insert(Control::DelayDivider, 115);
            last_values.insert(Control::DelayFeedback, 64);
            last_values.insert(Control::DelayLow, 64);
            last_values.insert(Control::DelayHigh, 64);

            
            last_values.insert(Control::ChannelFilterFollower(3), 64);

            last_values.insert(Control::KickPitch, 64);
            last_values.insert(Control::KickDecay, 64);
            last_values.insert(Control::BassPitch, 64);
            last_values.insert(Control::BassCutoff, 40);
            
            last_values.insert(Control::ExtPitch, 64);
            last_values.insert(Control::ExtFilter, 60);

            // a wee bit of swing
            last_values.insert(Control::Swing, 64);

            // update display and send all of the start values on load
            for control in control_ids.keys() {
                tx.send(TwisterMessage::Send(*control)).unwrap();
                tx.send(TwisterMessage::Refresh(*control)).unwrap();
                if let Some(control_id) = control_ids.get(control) {
                    recorder.allocate(*control_id, 50000);
                }
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
                    TwisterMessage::ControlChange(control, value, source) => {
                        if let Some(id) = control_ids.get(&control) {
                            let allow = if loops.contains_key(&control) {
                                let item = loops.get(&control).unwrap();
                                (item.offset + item.length) < (last_pos - MidiTime::from_ticks(8))
                            } else {
                                true
                            };

                            if allow {
                                loops.remove(&control);
                                let event = LoopEvent { 
                                    id: id.clone(), 
                                    value,
                                    pos: last_pos
                                };
         
                                tx_feedback.send(TwisterMessage::Event(event, source)).unwrap();
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
                            Control::ChannelDrive(channel) => {
                                let cc = channel_offsets[channel as usize % channel_offsets.len()] + 4;
                                throttled_zoia_output.send(&[176 + zoia_channel - 1, cc as u8, value]);
                            },

                            Control::ChannelDuck(channel) => {
                                let cc = channel_offsets[channel as usize % channel_offsets.len()] + 5;
                                throttled_zoia_output.send(&[176 + zoia_channel - 1, cc as u8, value]);
                            },

                            Control::ChannelFilterFollower(channel) => {
                                let cc = channel_offsets[channel as usize % channel_offsets.len()] + 6;
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
                                let value = if value > 20 {
                                    float_to_midi(divider / 2400.0)
                                } else {
                                    value
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

                            Control::KickPitch => {
                                let value = if value == 63 {
                                    64
                                } else {
                                    value
                                };
                                drum_output.send(&[176 + drum_channel - 1, 20, value]).unwrap();
                            },

                            Control::KickDecay => {
                                drum_output.send(&[176 + drum_channel - 1, 23, value]).unwrap();
                            },
                            
                            Control::SamplerPitch => {
                                let value = float_to_msb_lsb(midi_to_polar(value));
                                throttled_blackbox_output.send(&[(224 - 1) + sampler_mod_channel, value.0, value.1]);
                            },

                            Control::SamplerMod => {    
                                throttled_blackbox_output.send(&[176 + sampler_channel - 1, 1, value]);
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

                            Control::Cv1 => {
                                let value = if value == 63 {
                                    64
                                } else {
                                    value
                                };
                                cv1_output.send(&[176, 1, value]).unwrap();
                            },

                            Control::Cv2 => {
                                cv2_output.send(&[176, 1, value]).unwrap();
                            },

                            Control::Cv2LfoAmount => {
                                lfo_amounts.insert(Control::Cv2, midi_to_polar(value));
                            },

                            Control::BassPitch => {
                                let value = float_to_msb_lsb(midi_to_polar(value));
                                typhon_output.send(&[(224 - 1) + bass_channel, value.0, value.1]).unwrap();
                                throttled_blackbox_output.send(&[(224 - 1) + bass_channel, value.0, value.1]);
                            },

                            Control::BassCutoff => {
                                typhon_output.send(&[(176 - 1) + bass_channel, 4, value]).unwrap();
                            },

                            Control::BassWave => {
                                typhon_output.send(&[(176 - 1) + bass_channel, 5, value]).unwrap();
                            },

                            Control::BassTune => {
                                typhon_output.send(&[(176 - 1) + bass_channel, 6, value]).unwrap();
                            },

                            Control::BassFilterLfoAmount => {
                                lfo_amounts.insert(Control::BassCutoff, midi_to_polar(value));
                            },
                            
                            Control::SynthPitch => {
                                // hack around detent on mf twister
                                let value = float_to_msb_lsb(midi_to_polar(value));
                                streichfett_output.send(&[224, value.0, value.1]).unwrap();
                            },

                            Control::SynthTone => {
                                streichfett_output.send(&[176, 76, value]).unwrap();
                            },
                            Control::SynthRegistration => {
                                streichfett_output.send(&[176, 70, value]).unwrap();
                            },
                            Control::SynthMod => {
                                streichfett_output.send(&[176, 1, value]).unwrap();
                            },

                            Control::ChannelFilterLfoAmount(channel) => {
                                lfo_amounts.insert(Control::ChannelFilter(channel), midi_to_polar(value));
                            },

                            Control::ExtPitch => {
                                // hack around detent on mf twister
                                let value = float_to_msb_lsb(midi_to_polar(value));
                                nts1_output.send(&[224, value.0, value.1]);
                            },

                            Control::ExtFilter => {
                                nts1_output.send(&[176, 43, value]).unwrap();
                            },

                            Control::ExtFilterLfoAmount => {
                                lfo_amounts.insert(Control::ExtFilter, midi_to_polar(value));
                            },

                            Control::None => ()
                        }

                    },
                    TwisterMessage::Event(event, source) => {
                        let control = Control::from_id(event.id);
                        let value = event.value.value();

                        last_values.insert(control, value);

                        if source == EventSource::Internal {
                            tx_feedback.send(TwisterMessage::Send(control)).unwrap();
                        }

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
                                        tx_feedback.send(TwisterMessage::Event(event.clone(), EventSource::Internal)).unwrap();
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
            _midi_input_streichfett: streichfett_input,
            _midi_input_tr6s: tr6s_input,
            _midi_input_nts1: nts1_input,
            tx: tx_clock
        }
    }

    pub fn schedule (&self, pos: MidiTime, length: MidiTime) {
        self.tx.send(TwisterMessage::Schedule {pos, length}).unwrap();
    }
}

#[derive(Debug, Clone)]
enum TwisterMessage {
    ControlChange(Control, OutputValue, EventSource),
    BankChange(u8),
    Event(LoopEvent, EventSource),
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
    ChannelFilterLfoAmount(u32),
    
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

    KickPitch,
    KickDecay,
    SamplerPitch,
    SamplerMod,
    
    Swing,

    LfoRate,
    LfoHold,
    LfoSkew,
    LfoOffset,

    Cv1,
    Cv2,
    Cv2LfoAmount,
    
    BassPitch,
    BassCutoff,
    BassWave,
    BassTune,
    BassFilterLfoAmount,
        
    SynthPitch,
    SynthTone,
    SynthRegistration,
    SynthMod,

    ExtPitch,
    ExtFilter,
    ExtFilterLfoAmount,

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
            (0, row, 3) => Control::ChannelFilter(row),

            // Bank B
            (1, row, 0) => Control::ChannelCrush(row),
            (1, row, 1) => Control::ChannelRedux(row),
            (1, row, 2) => Control::ChannelFilterLfoAmount(row),

            (1, 0, 3) => Control::LfoRate,
            (1, 1, 3) => Control::LfoHold,
            (1, 2, 3) => Control::LfoSkew,
            (1, 3, 3) => Control::LfoOffset,


            // Bank C
            (2, 0, 0) => Control::KickPitch,
            (2, 0, 1) => Control::KickDecay,
            (2, 0, 2) => Control::Cv1,
            (2, 0, 3) => Control::Cv2,

            (2, 1, 0) => Control::SamplerPitch,
            (2, 1, 1) => Control::SamplerMod,
            (2, 1, 2) => Control::ExtPitch,
            (2, 1, 3) => Control::ExtFilter,

            (2, 2, 0) => Control::BassPitch,
            (2, 2, 1) => Control::BassCutoff,
            (2, 2, 2) => Control::BassWave,
            (2, 2, 3) => Control::BassTune,

            (2, 3, 0) => Control::SynthPitch,
            (2, 3, 1) => Control::SynthTone,
            (2, 3, 2) => Control::SynthRegistration,
            (2, 3, 3) => Control::SynthMod,



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

pub fn float_to_msb_lsb(input: f64) -> (u8, u8) {
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