use ::midi_connection;
use std::sync::mpsc;
use ::devices::BlofeldDrumParams;
use ::loop_recorder::{LoopRecorder, LoopEvent};
use ::clock_source::{RemoteClock, FromClock, ToClock, MidiTime};
use ::output_value::OutputValue;
use ::loop_grid_launchpad::{LoopGridParams, ChannelRepeat};
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
    pub fn new (port_name: &str, kmix_port_name: &str, main_output: midi_connection::SharedMidiOutputConnection, blofeld_output: midi_connection::SharedMidiOutputConnection, drum_params: Arc<Mutex<BlofeldDrumParams>>, params: Arc<Mutex<LoopGridParams>>, clock: RemoteClock, meta_tx: mpsc::Sender<AudioRecorderEvent>) -> Self {
        let (tx, rx) = mpsc::channel();
        let clock_sender = clock.sender.clone();
        let kmix_port_name = String::from(kmix_port_name);
        let control_ids = get_control_ids();

        let ext_channel = 5; // stereo pair
        let kmix_channel_map: [u8; 5] = [ 4, 2, 3, 1, ext_channel ];
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
            let mut blofeld_output = blofeld_output;
            let drum_params = drum_params;
            let mut kmix_output = midi_connection::get_shared_output(&kmix_port_name);
            let mut throttled_kmix_output = ThrottledOutput::new(kmix_output);

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

                last_values.insert(Control::ChannelVolumeLfo(channel), 64);
                last_values.insert(Control::ChannelReverbLfo(channel), 64);
                last_values.insert(Control::ChannelDelayLfo(channel), 64);
                last_values.insert(Control::ChannelModLfo(channel), 64);
                last_values.insert(Control::Tempo, 64);

                // drum defaults
                tx_feedback.send(TwisterMessage::Event(LoopEvent { 
                    id: *control_ids.get(&Control::DrumModX(channel)).unwrap(), 
                    value: OutputValue::On(64),
                    pos: last_pos
                })).unwrap();
                tx_feedback.send(TwisterMessage::Event(LoopEvent { 
                    id: *control_ids.get(&Control::DrumModY(channel)).unwrap(), 
                    value: OutputValue::On(64),
                    pos: last_pos
                })).unwrap();
                tx_feedback.send(TwisterMessage::Event(LoopEvent { 
                    id: *control_ids.get(&Control::DrumModZ(channel)).unwrap(), 
                    value: OutputValue::On(0),
                    pos: last_pos
                })).unwrap();
            }

            last_values.insert(Control::LfoHold, lfo.hold);
            last_values.insert(Control::LfoOffset, lfo.offset);
            last_values.insert(Control::LfoSkew, lfo.skew);
            last_values.insert(Control::LfoSpeed, lfo.speed);
            last_values.insert(Control::ReturnVolume, 100);
            last_values.insert(Control::ReturnVolumeLfo, 64);

            tx_feedback.send(TwisterMessage::Event(LoopEvent { 
                id: *control_ids.get(&Control::ChannelSend(3)).unwrap(), 
                value: OutputValue::On(100),
                pos: last_pos
            })).unwrap();

            tx_feedback.send(TwisterMessage::Event(LoopEvent { 
                id: *control_ids.get(&Control::ChannelVolume(4)).unwrap(), 
                value: OutputValue::On(100),
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
                                    meta_tx.send(AudioRecorderEvent::ChannelVolume(channel, value));
                                    throttled_kmix_output.send(&[176 + kmix_channel - 1, 1, value]);
                                },
                                Control::ChannelReverb(channel) => {
                                    let kmix_channel = kmix_channel_map[channel as usize % kmix_channel_map.len()];
                                    throttled_kmix_output.send(&[176 + kmix_channel - 1, 23, value]);

                                    if channel == 0 { // ext shares delay and reverb send with drums
                                        throttled_kmix_output.send(&[176 + ext_channel - 1, 23, value]);
                                    }
                                },
                                Control::ChannelDelay(channel) => {
                                    let kmix_channel = kmix_channel_map[channel as usize % kmix_channel_map.len()];
                                    throttled_kmix_output.send(&[176 + kmix_channel - 1, 25, value]);

                                    if channel == 0 { // ext shares delay and reverb send with drums
                                        throttled_kmix_output.send(&[176 + ext_channel - 1, 25, value]);
                                    }
                                },
                                Control::ChannelSend(channel) => {
                                    let kmix_channel = kmix_channel_map[channel as usize % kmix_channel_map.len()];
                                    throttled_kmix_output.send(&[176 + kmix_channel - 1, 27, value]);
                                },
                                Control::ChannelMod(channel) => {
                                    match channel {
                                        1 => {
                                            main_output.send(&[208, value]).unwrap();
                                        },
                                        2 => {
                                            blofeld_output.send(&[208, value]).unwrap();
                                        },
                                        _ => ()
                                    }
                                },

                                Control::DrumVelocity(trigger) => {
                                    let mut drum_params = drum_params.lock().unwrap();
                                    let trigger_index = trigger as usize % drum_params.velocities.len();
                                    drum_params.velocities[trigger_index] = value;
                                },
                                Control::DrumModX(trigger) => {
                                    let mut drum_params = drum_params.lock().unwrap();
                                    let trigger_index = trigger as usize % drum_params.x.len();
                                    drum_params.x[trigger_index] = value;
                                },
                                Control::DrumModY(trigger) => {
                                    let mut drum_params = drum_params.lock().unwrap();
                                    let trigger_index = trigger as usize % drum_params.y.len();
                                    drum_params.y[trigger_index] = value;
                                },
                                Control::DrumModZ(trigger) => {
                                    blofeld_output.send(&[176 + 1 + trigger as u8, 94, value]).unwrap();
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
                                Control::ChannelVolumeLfo(channel) => {
                                    lfo_amounts.insert(Control::ChannelVolume(channel), midi_to_polar(value));
                                },
                                Control::ChannelReverbLfo(channel) => {
                                    lfo_amounts.insert(Control::ChannelReverb(channel), midi_to_polar(value));
                                },
                                Control::ChannelDelayLfo(channel) => {
                                    lfo_amounts.insert(Control::ChannelDelay(channel), midi_to_polar(value));
                                },
                                Control::ChannelModLfo(channel) => {
                                    lfo_amounts.insert(Control::ChannelMod(channel), midi_to_polar(value));
                                },
                                Control::ChannelSendLfo(channel) => {
                                    lfo_amounts.insert(Control::ChannelSend(channel), midi_to_polar(value));
                                },
                                Control::ReturnVolume => {
                                    meta_tx.send(AudioRecorderEvent::ChannelVolume(5, value)).unwrap();
                                    throttled_kmix_output.send(&[176 + fx_return_channel - 1, 1, value]);
                                },
                                Control::ReturnVolumeLfo => {
                                    lfo_amounts.insert(Control::ReturnVolume, midi_to_polar(value));
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
                        let value = match control {
                            Control::DrumVelocity(trigger) => {
                                let drum_params = drum_params.lock().unwrap();
                                drum_params.velocities[trigger as usize % drum_params.velocities.len()]
                            },
                            Control::DrumModX(trigger) => {
                                let drum_params = drum_params.lock().unwrap();
                                drum_params.x[trigger as usize % drum_params.x.len()]                            
                            },
                            Control::ChannelRepeat(channel) => {
                                let params = params.lock().unwrap();
                                params.channel_repeat.get(&channel).unwrap_or(&ChannelRepeat::None).to_midi()
                            },
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
                                let mut params = params.lock().unwrap();
                                if params.reset_automation {
                                    // HACK: ack reset message from clear all
                                    params.reset_automation = false;
                                    loops.clear();

                                    for control in control_ids.keys() {
                                        tx.send(TwisterMessage::Refresh(*control)).unwrap();
                                    }

                                    // reset LFO
                                    for channel in 0..5 {
                                        tx_feedback.send(TwisterMessage::ControlChange(Control::ChannelVolumeLfo(channel), OutputValue::On(64))).unwrap();
                                        tx_feedback.send(TwisterMessage::ControlChange(Control::ChannelReverbLfo(channel), OutputValue::On(64))).unwrap();
                                        tx_feedback.send(TwisterMessage::ControlChange(Control::ChannelDelayLfo(channel), OutputValue::On(64))).unwrap();
                                        tx_feedback.send(TwisterMessage::ControlChange(Control::ChannelModLfo(channel), OutputValue::On(64))).unwrap();
                                        tx_feedback.send(TwisterMessage::ControlChange(Control::ChannelSendLfo(channel), OutputValue::On(64))).unwrap();
                                    }

                                    tx_feedback.send(TwisterMessage::ControlChange(Control::ReturnVolumeLfo, OutputValue::On(64))).unwrap();
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
    Refresh(Control),
    Recording(Control, bool),
    Clock(FromClock)
}

#[derive(Debug, Eq, PartialEq, Hash, Clone, Copy)]
enum Control {
    ChannelVolume(u32),
    ChannelReverb(u32),
    ChannelDelay(u32),
    ChannelMod(u32),
    ChannelSend(u32),

    ChannelVolumeLfo(u32),
    ChannelReverbLfo(u32),
    ChannelDelayLfo(u32),
    ChannelModLfo(u32),
    ChannelSendLfo(u32),

    DrumVelocity(u32),
    DrumModX(u32),
    DrumModY(u32),
    DrumModZ(u32),

    ChannelRepeat(u32),

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
            (0, 0, 3)  => Control::ChannelVolume(4),
            (0, 3, 3)  => Control::ChannelSend(3),
            (0, row, 0) => Control::ChannelVolume(row),
            (0, row, 1) => Control::ChannelReverb(row),
            (0, row, 2) => Control::ChannelDelay(row),
            (0, row, 3) => Control::ChannelMod(row),

            // Bank B
            (1, 0, col) => Control::DrumVelocity(col),
            (1, 1, col) => Control::DrumModX(col),
            (1, 2, col) => Control::DrumModY(col),
            (1, 3, col) => Control::DrumModZ(col),

            // Bank C
            (2, row, 0) => Control::ChannelRepeat(row),

            (2, 0, 1) => Control::LfoSpeed,
            (2, 0, 2) => Control::LfoSkew,
            (2, 1, 1) => Control::LfoHold,
            (2, 1, 2) => Control::LfoOffset,

            (2, 3, 1) => Control::ReturnVolume,
            (2, 3, 2) => Control::ReturnVolumeLfo,

            (2, 1, 3) => Control::Swing,
            (2, 0, 3) => Control::Tempo,
            (2, 2, 3) => Control::DelayTime,
            (2, 3, 3) => Control::DelayFeedback,

            // Bank D
            (3, 0, 3)  => Control::ChannelVolumeLfo(4),
            (3, 3, 3)  => Control::ChannelSendLfo(3),
            (3, row, 0) => Control::ChannelVolumeLfo(row),
            (3, row, 1) => Control::ChannelReverbLfo(row),
            (3, row, 2) => Control::ChannelDelayLfo(row),
            (3, row, 3) => Control::ChannelModLfo(row),

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