use ::midi_connection;
use std::sync::mpsc;
use ::devices::BlofeldDrumParams;
use ::loop_recorder::{LoopRecorder, LoopEvent};
use ::clock_source::{RemoteClock, FromClock, ToClock, MidiTime};
use ::output_value::OutputValue;
use ::loop_grid_launchpad::{LoopGridParams, ChannelRepeat};
use ::audio_recorder::AudioRecorderEvent;

use std::thread;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

pub struct Twister {
    _midi_input: midi_connection::ThreadReference
}

impl Twister {
    pub fn new (port_name: &str, kmix_port_name: &str, main_output: midi_connection::SharedMidiOutputConnection, blofeld_output: midi_connection::SharedMidiOutputConnection, drum_params: Arc<Mutex<BlofeldDrumParams>>, params: Arc<Mutex<LoopGridParams>>, clock: RemoteClock, meta_tx: mpsc::Sender<AudioRecorderEvent>) -> Self {
        let (tx, rx) = mpsc::channel();
        let clock_sender = clock.sender.clone();
        let kmix_port_name = String::from(kmix_port_name);
        let control_ids = get_control_ids();

        let kmix_channel_map: [u8; 4] = [ 4, 2, 3, 1 ];
        let looper_return_channel = 5; // stereo pair
        let fx_return_channel = 7; // stereo pair

        let mut main_mix_kmix_channels = kmix_channel_map.to_vec();
        main_mix_kmix_channels.push(fx_return_channel);

        let delay_channel = 15;

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
            let mut control = Control::from_id(message[1] as u32);
            if message[0] == 176 {
                tx_input.send(TwisterMessage::ControlChange(control, OutputValue::On(message[2]))).unwrap();
            } else if message[0] == 177 {
                tx_input.send(TwisterMessage::Recording(control, message[2] > 0)).unwrap();
            }
        });

        for control in control_ids.keys() {
            tx.send(TwisterMessage::Refresh(*control)).unwrap();
        }

        thread::spawn(move || {
            let mut recorder = LoopRecorder::new();
            let mut last_pos = MidiTime::zero();
            let mut last_tempo = 120;
            let mut last_values = HashMap::new();
            let mut record_start_times = HashMap::new();
            let mut loops: HashMap<Control, Loop> = HashMap::new();
            let mut main_output = main_output;
            let mut blofeld_output = blofeld_output;
            let drum_params = drum_params;
            let mut kmix_output = midi_connection::get_shared_output(&kmix_port_name);
            let mut delay_time = 0;
            let mut feedback_time = 0;

            let mut looper_vocal_amount = 1.0;
            let mut looper_send_multiplier = 1.0;
            let mut looper_main_amount = 0.0;

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
            }

            tx_feedback.send(TwisterMessage::Event(LoopEvent { 
                id: *control_ids.get(&Control::LooperSend).unwrap(), 
                value: OutputValue::On(0),
                pos: last_pos
            })).unwrap();

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
                    TwisterMessage::Event(event) => {
                        let control = Control::from_id(event.id);
                        let value = event.value.value();

                        last_values.insert(control, value);

                        match control {
                            Control::ChannelVolume(channel) => {
                                let kmix_channel = kmix_channel_map[channel as usize % kmix_channel_map.len()];
                                meta_tx.send(AudioRecorderEvent::ChannelVolume(channel, value)).unwrap();
                                kmix_output.send(&[176 + kmix_channel - 1, 1, value]).unwrap();

                                if channel == 3 {
                                    tx_feedback.send(TwisterMessage::UpdateLooperSend).unwrap();
                                }
                            },
                            Control::ChannelReverb(channel) => {
                                let kmix_channel = kmix_channel_map[channel as usize % kmix_channel_map.len()];
                                kmix_output.send(&[176 + kmix_channel - 1, 23, value]).unwrap();

                                if channel == 3 {
                                    tx_feedback.send(TwisterMessage::UpdateLooperSend).unwrap();
                                }
                            },
                            Control::ChannelDelay(channel) => {
                                let kmix_channel = kmix_channel_map[channel as usize % kmix_channel_map.len()];
                                kmix_output.send(&[176 + kmix_channel - 1, 25, value]).unwrap();

                                if channel == 3 {
                                    tx_feedback.send(TwisterMessage::UpdateLooperSend).unwrap();
                                }
                            },
                            Control::ChannelMod(channel) => {
                                match channel {
                                    0 => {
                                        for i in 8..16 {
                                            blofeld_output.send(&[208 + i - 1, value]).unwrap();
                                        }
                                    },
                                    1 => {
                                        main_output.send(&[208, value]).unwrap();
                                    },
                                    2 => {
                                        blofeld_output.send(&[208, value]).unwrap();
                                    },
                                    _ => ()
                                }
                            },

                            Control::LooperSend => {
                                let shifted_value = value as f64 - 64.0;
                                let polar_value = if shifted_value < 0.0 {
                                    shifted_value / 64.0
                                } else {
                                    shifted_value / 63.0
                                };
                                looper_vocal_amount = polar_value.min(0.0) * -1.0;
                                looper_main_amount = polar_value.max(0.0);
                                looper_send_multiplier = 1.0 - looper_main_amount;
                                tx_feedback.send(TwisterMessage::UpdateLooperSend).unwrap();
                            },
                            Control::DrumVelocity(trigger) => {
                                let mut drum_params = drum_params.lock().unwrap();
                                let trigger_index = trigger as usize % drum_params.velocities.len();
                                drum_params.velocities[trigger_index] = value;
                            },
                            Control::DrumMod(trigger) => {
                                let mut drum_params = drum_params.lock().unwrap();
                                let trigger_index = trigger as usize % drum_params.mods.len();
                                drum_params.mods[trigger_index] = value;
                            },
                            Control::ChannelRepeat(channel) => {
                                let mut params = params.lock().unwrap();
                                params.channel_repeat.insert(channel, ChannelRepeat::from_midi(value));
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
                                delay_time = value;
                            },
                            Control::DelayFeedback => {
                                main_output.send(&[176 + delay_channel - 1, 5, value]).unwrap();
                                feedback_time = value;
                            },
                            Control::None => ()
                        }

                        tx_feedback.send(TwisterMessage::Refresh(control)).unwrap();

                        recorder.add(event);
                    },

                    TwisterMessage::UpdateLooperSend => {
                        let vocal_kmix_channel = kmix_channel_map[3];
                        
                        let vocal_volume = last_values.get(&Control::ChannelVolume(3)).unwrap_or(&100);
                        let vocal_reverb = last_values.get(&Control::ChannelReverb(3)).unwrap_or(&0);
                        let vocal_delay = last_values.get(&Control::ChannelDelay(3)).unwrap_or(&0);

                        let channel_volume = ((*vocal_volume as f64 * looper_send_multiplier) + (100.0 * (1.0 - looper_send_multiplier))).min(127.0) as u8;
                        let channel_reverb = (*vocal_reverb as f64 * looper_send_multiplier).min(127.0) as u8;
                        let channel_delay = (*vocal_delay as f64 * looper_send_multiplier).min(127.0) as u8;

                        // looper send amounts
                        for channel in &main_mix_kmix_channels {
                            let amount = if channel == &vocal_kmix_channel && looper_vocal_amount > 0.0 {
                                looper_vocal_amount
                            } else {
                                looper_main_amount
                            };
                            kmix_output.send(&[176 + channel - 1, 27, (amount * 127.0).min(127.0) as u8]).unwrap();
                        }

                        // return volume
                        kmix_output.send(&[176 + looper_return_channel - 1, 1, channel_volume]).unwrap();

                        // return reverb
                        kmix_output.send(&[176 + looper_return_channel - 1, 23, channel_reverb]).unwrap();

                        // return delay
                        kmix_output.send(&[176 + looper_return_channel - 1, 25, channel_delay]).unwrap();
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
                        let value = match control {
                            Control::Tempo => (last_tempo - 60) as u8,
                            Control::Swing => {
                                let params = params.lock().unwrap();
                                let exp_swing = if params.swing < 0.0 {
                                    -params.swing.abs().powf(1.0 / 2.0)
                                } else {
                                    params.swing.powf(1.0 / 2.0)
                                };
                                (exp_swing * 64.0 + 64.0) as u8
                            },
                            Control::DrumVelocity(trigger) => {
                                let drum_params = drum_params.lock().unwrap();
                                drum_params.velocities[trigger as usize % drum_params.velocities.len()]
                            },
                            Control::DrumMod(trigger) => {
                                let drum_params = drum_params.lock().unwrap();
                                drum_params.mods[trigger as usize % drum_params.mods.len()]                            
                            },
                            Control::ChannelRepeat(channel) => {
                                let params = params.lock().unwrap();
                                params.channel_repeat.get(&channel).unwrap_or(&ChannelRepeat::None).to_midi()
                            },
                            Control::DelayTime => delay_time,
                            Control::DelayFeedback => feedback_time,
                            _ => *last_values.get(&control).unwrap_or(&0)
                        };

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
                                for (control, value) in &loops {
                                    let offset = value.offset % value.length;
                                    let playback_pos = value.offset + ((pos - offset) % value.length);

                                    if let Some(id) = control_ids.get(control) {
                                        if let Some(range) = recorder.get_range_for(id.clone(), playback_pos, playback_pos + length) {
                                            for event in range {
                                                tx_feedback.send(TwisterMessage::Event(event.clone())).unwrap();
                                            }
                                        }
                                    }
                                    
                                }
                                last_pos = pos;
                            },
                            FromClock::Tempo(value) => {
                                last_tempo = value;
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

#[derive(Debug)]
enum TwisterMessage {
    ControlChange(Control, OutputValue),
    Event(LoopEvent),
    Refresh(Control),
    Recording(Control, bool),
    Clock(FromClock),
    UpdateLooperSend
}

#[derive(Debug, Eq, PartialEq, Hash, Clone, Copy)]
enum Control {
    ChannelVolume(u32),
    ChannelReverb(u32),
    ChannelDelay(u32),
    ChannelMod(u32),
    LooperSend,

    DrumVelocity(u32),
    DrumMod(u32),

    ChannelRepeat(u32),

    Tempo,
    Swing,
    DelayTime,
    DelayFeedback,

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
            (0, 3, 3)  => Control::LooperSend,
            (0, row, 0) => Control::ChannelVolume(row),
            (0, row, 1) => Control::ChannelReverb(row),
            (0, row, 2) => Control::ChannelDelay(row),
            (0, row, 3) => Control::ChannelMod(row),

            // Bank B
            (1, 0, col) => Control::DrumVelocity(col),
            (1, 1, col) => Control::DrumMod(col),
            (1, 2, col) => Control::DrumVelocity(col + 4),
            (1, 3, col) => Control::DrumMod(col + 4),

            // Bank C
            (2, row, 0) => Control::ChannelRepeat(row),
            (2, 0, 3) => Control::Tempo,
            (2, 1, 3) => Control::Swing,
            (2, 2, 3) => Control::DelayTime,
            (2, 3, 3) => Control::DelayFeedback,

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