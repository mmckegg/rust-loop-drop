use ::midi_connection;
use std::sync::mpsc;
use ::devices::SP404VelocityMap;
use ::loop_recorder::{LoopRecorder, LoopEvent};
use ::clock_source::{RemoteClock, FromClock, ToClock, MidiTime};
use ::output_value::OutputValue;
use ::loop_grid_launchpad::LoopGridParams;

use std::thread;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

pub struct Twister {
    _midi_input: midi_connection::ThreadReference
}

impl Twister {
    pub fn new (port_name: &str, kmix_port_name: &str, aftertouch_targets: Vec<(midi_connection::SharedMidiOutputConnection, u8)>, velocity_maps: Vec<Arc<Mutex<SP404VelocityMap>>>, params: Arc<Mutex<LoopGridParams>>, clock: RemoteClock) -> Self {
        let (tx, rx) = mpsc::channel();
        let clock_sender = clock.sender.clone();
        let kmix_port_name = String::from(kmix_port_name);

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

        for i in 0..12 {
            tx.send(TwisterMessage::Refresh(Control::VelocityMap(0, i))).unwrap();
        }

        tx.send(TwisterMessage::Refresh(Control::Swing)).unwrap();
        tx.send(TwisterMessage::Refresh(Control::VelocityMaster(0))).unwrap();
        tx.send(TwisterMessage::Refresh(Control::VelocityMaster(1))).unwrap();
        tx.send(TwisterMessage::Refresh(Control::DelayFeedback)).unwrap();
        tx.send(TwisterMessage::Refresh(Control::DelayTime)).unwrap();

        thread::spawn(move || {
            let mut recorder = LoopRecorder::new();
            let mut last_pos = MidiTime::zero();
            let mut last_tempo = 120;
            let mut last_values = HashMap::new();
            let mut record_start_times = HashMap::new();
            let mut loops: HashMap<Control, Loop> = HashMap::new();
            let mut aftertouch_targets = aftertouch_targets;
            let mut velocity_maps = velocity_maps;
            let mut kmix_output = midi_connection::get_shared_output(&kmix_port_name);

            for received in rx {
                match received {
                    TwisterMessage::ControlChange(control, value) => {
                        if let Some(id) = control.id() {
                            let allow = if loops.contains_key(&control) {
                                let item = loops.get(&control).unwrap();
                                (item.offset + item.length) < (last_pos - MidiTime::from_ticks(8))
                            } else {
                                true
                            };

                            if allow {
                                loops.remove(&control);
                                tx_feedback.send(TwisterMessage::Event(LoopEvent { 
                                    id, 
                                    value,
                                    pos: last_pos
                                })).unwrap();
                            }
                        }
                    },
                    TwisterMessage::Event(event) => {
                        let control = Control::from_id(event.id);
                        let value = event.value;

                        last_values.insert(control, value);

                        match control {
                            Control::Tempo => {
                                clock_sender.send(ToClock::SetTempo(value.value() as usize + 60)).unwrap();
                            },
                            Control::Swing => {
                                let mut params = params.lock().unwrap();
                                let val = value.value();
                                params.swing = (val as f64 - 64.0) / 64.0;
                            },
                            Control::Param(channel, control) => {
                                tx_feedback.send(TwisterMessage::ParamControl(channel, control, value)).unwrap();
                            },
                            Control::VelocityMap(channel, trigger) => {
                                if let Some(velocity_map) = velocity_maps.get(channel) {
                                    let mut velocity_map = velocity_map.lock().unwrap();
                                    let trigger_index = trigger % velocity_map.triggers.len();
                                    velocity_map.triggers[trigger_index] = value.value();
                                }
                            },
                            Control::VelocityMaster(channel) => {
                                if let Some(velocity_map) = velocity_maps.get(channel) {
                                    let mut velocity_map = velocity_map.lock().unwrap();
                                    velocity_map.master = value.value();
                                }
                            },
                            Control::DelayTime => {

                            },
                            Control::DelayFeedback => {

                            },
                            Control::None => ()
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

                    TwisterMessage::TapTempo => {
                        clock_sender.send(ToClock::TapTempo).unwrap();
                    },

                    TwisterMessage::Refresh(control) => {
                        let value = match control {
                            Control::Tempo => (last_tempo - 60) as u8,
                            Control::Swing => {
                                let params = params.lock().unwrap();
                                (params.swing * 64.0 + 64.0) as u8
                            },
                            Control::Param(_channel, _index) => last_values.get(&control).unwrap_or(&OutputValue::Off).value(),
                            Control::VelocityMap(channel, trigger) => {
                                if let Some(velocity_map) = velocity_maps.get(channel) {
                                    let velocity_map = velocity_map.lock().unwrap();
                                    velocity_map.triggers[trigger % velocity_map.triggers.len()]
                                } else {
                                    0
                                }
                            },
                            Control::VelocityMaster(channel) => {
                                if let Some(velocity_map) = velocity_maps.get(channel) {
                                    let velocity_map = velocity_map.lock().unwrap();
                                    velocity_map.master
                                } else {
                                    0
                                }                                
                            },
                            Control::DelayTime => 0,
                            Control::DelayFeedback => 0,
                            Control::None => 0
                        };

                        if let Some(id) = control.id() {
                            output.send(&[176, id as u8, value]).unwrap();

                            // MFT animation for currently looping (Channel 6)
                            if loops.contains_key(&control) {
                                output.send(&[181, id as u8, 13]).unwrap();
                            } else {
                                output.send(&[181, id as u8, 0]).unwrap();
                            }
                        }

                        
                    },

                    TwisterMessage::ParamControl(channel, control, value) => {
                        let value = value.value();
                        let kmix_channel = match channel {
                            0 => 5,
                            1 => 2,
                            2 => 3,
                            _ => 1
                        };

                        match control {
                            ParamControl::Kaoss => {
                                let value_f = ((value as f64) - 64.0) / 64.0;
                                let output_values = if value_f > 0.0 {
                                    ((100.0 - value_f * 100.0), value_f * 100.0)
                                } else {
                                    ((100.0 + value_f * 100.0), 0.0)
                                };
                                
                                kmix_output.send(&[176 + kmix_channel - 1, 1, output_values.0 as u8]);
                                kmix_output.send(&[176 + kmix_channel - 1, 27, output_values.1 as u8]);
                            },
                            ParamControl::Reverb => {
                                kmix_output.send(&[176 + kmix_channel - 1, 23, value]);
                            },
                            ParamControl::Delay => {
                                kmix_output.send(&[176 + kmix_channel - 1, 25, value]);
                            },
                            ParamControl::Aftertouch => {
                                if let Some(&mut (ref mut port, channel)) = aftertouch_targets.get_mut((channel - 1) as usize) {
                                    port.send(&[208 + channel - 1, value]);
                                }
                            }
                        }
                    },

                    TwisterMessage::Clock(msg) => {
                        match msg {
                            FromClock::Schedule { pos, length } => {
                                for (key, value) in &loops {
                                    let offset = value.offset % value.length;
                                    let playback_pos = value.offset + ((pos - offset) % value.length);

                                    if let Some(id) = key.id() {
                                        if let Some(range) = recorder.get_range_for(id, playback_pos, playback_pos + length) {
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
    ParamControl(u32, ParamControl, OutputValue),
    TapTempo,
    Clock(FromClock)
}

#[derive(Debug, Eq, PartialEq, Hash, Clone, Copy)]
enum Control {
    Param(u32, ParamControl),
    VelocityMap(usize, usize),
    VelocityMaster(usize),
    DelayTime,
    DelayFeedback,
    Tempo,
    Swing,
    None
}

#[derive(Debug, Eq, PartialEq, Hash, Clone, Copy)]
enum ParamControl {
    Kaoss = 0,
    Reverb = 1,
    Delay = 2,
    Aftertouch = 3
}

#[derive(Debug)]
struct Loop {
    offset: MidiTime,
    length: MidiTime
}

impl Control {
    fn id (&self) -> Option<u32> {
        match self {
            &Control::Tempo => Some(get_index(8, 3)),
            &Control::Swing => Some(get_index(0, 3)),
            &Control::Param(channel, param) => Some(get_index(channel, param as u32)),
            &Control::VelocityMap(channel, trigger) => {
                match channel {
                    0 => Some(get_index(4, 0) + (trigger as u32)),
                    _ => None
                }
            },
            &Control::VelocityMaster(channel) => {
                match channel {
                    0 => Some(get_index(7, 0)),
                    1 => Some(get_index(7, 1)),
                    _ => None
                }
            },
            &Control::DelayTime => Some(get_index(7, 2)),
            &Control::DelayFeedback => Some(get_index(7, 3)),
            &Control::None => None
        }
    }

    fn from_id (id: u32) -> Control {
        let col = id % 4;
        let row = id / 4;
        let knob_id = id % 16;
        let page_id = id / 16;

        if col == 3 && row == 0 {
            Control::Swing
        } else if col == 3 && row == 8 {
            Control::Tempo
        } else if page_id == 0 {
            Control::Param(row, match col {
                0 => ParamControl::Kaoss,
                1 => ParamControl::Reverb,
                2 => ParamControl::Delay,
                _ => ParamControl::Aftertouch
            })
        } else if page_id == 1 && knob_id < 12  {
            Control::VelocityMap(0, knob_id as usize)
        } else if page_id == 1 && knob_id == 12 {
            Control::VelocityMaster(0)
        } else if page_id == 1 && knob_id == 13 {
            Control::VelocityMaster(1)
        } else if page_id == 1 && knob_id == 14 {
            Control::DelayTime
        } else if page_id == 1 && knob_id == 15 {
            Control::DelayFeedback
        } else {
            Control::None
        }
    }
}

fn get_index(row: u32, col: u32) -> u32 {
    row * 4 + col
}