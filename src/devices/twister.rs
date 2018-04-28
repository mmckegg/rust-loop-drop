use ::midi_connection;
use std::sync::mpsc;
use ::loop_recorder::{LoopRecorder, LoopEvent};
use ::clock_source::{RemoteClock, FromClock, ToClock, MidiTime};
use ::output_value::OutputValue;

use std::thread;
use std::collections::HashMap;

pub struct Twister {
    _midi_input: midi_connection::MidiInputConnection<()>
}

impl Twister {
    pub fn new (port_name: &str, kmix_port_name: &str, aftertouch_targets: Vec<(midi_connection::SharedMidiOutputConnection, u8)>, clock: RemoteClock) -> Self {
        let (tx, rx) = mpsc::channel();
        let clock_sender = clock.sender.clone();
        let kmix_port_name = String::from(kmix_port_name);

        let tx_clock = tx.clone();
        let tx_feedback = tx.clone();

        // pipe clock in
        thread::spawn(move || {
            for msg in clock.receiver {
                tx_clock.send(TwisterMessage::Clock(msg)).unwrap();
            }
        });

        let mut output = midi_connection::get_output(port_name).unwrap();

        let input = midi_connection::get_input(port_name, move |_stamp, message, _| {
            let control = Control::from_id(message[1] as u32);
            if message[0] == 176 {
                tx.send(TwisterMessage::ControlChange(control, OutputValue::On(message[2]))).unwrap();
            } else if message[0] == 177 {
                if let Control::Tempo = control {
                    tx.send(TwisterMessage::TapTempo).unwrap();
                } else {
                    tx.send(TwisterMessage::Recording(control, message[2] > 0)).unwrap();
                }
            }
        }, ()).unwrap();

        thread::spawn(move || {
            let mut recorder = LoopRecorder::new();
            let mut last_pos = MidiTime::zero();
            let mut last_tempo = 120;
            let mut last_values = HashMap::new();
            let mut record_start_times = HashMap::new();
            let mut loops: HashMap<Control, Loop> = HashMap::new();
            let mut aftertouch_targets = aftertouch_targets;
            let mut kmix_output = midi_connection::get_output(&kmix_port_name);

            for received in rx {
                match received {
                    TwisterMessage::ControlChange(control, value) => {
                        let id = control.id();
                        // let ignore = match loops.entry(control) {
                        //     Occupied(mut entry) => {
                        //         if entry.get().to < (last_pos - MidiTime::from_ticks(8)) {
                        //             entry.remove();
                        //             false
                        //         } else {
                        //             true
                        //         }

                        //     },
                        //     Vacant(_) => false
                        // };

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

                            },
                            Control::Param(channel, control) => {
                                tx_feedback.send(TwisterMessage::ParamControl(channel, control, value)).unwrap();
                            }
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
                            Control::Swing => 64,
                            Control::Param(_channel, _index) => last_values.get(&control).unwrap_or(&OutputValue::Off).value()
                        };

                        output.send(&[176, control.id() as u8, value]).unwrap();

                        // MFT animation for currently looping (Channel 6)
                        if loops.contains_key(&control) {
                            output.send(&[181, control.id() as u8, 13]).unwrap();
                        } else {
                            output.send(&[181, control.id() as u8, 0]).unwrap();
                        }
                    },

                    TwisterMessage::ParamControl(channel, control, value) => {
                        if let &mut Ok(ref mut kmix_output) = &mut kmix_output {
                            let value = value.value();
                            let kmix_channel = match channel {
                                0 => 1,
                                1 => 2,
                                2 => 3,
                                _ => 5
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
                                    if let Some(&mut (ref mut port, channel)) = aftertouch_targets.get_mut(channel as usize) {
                                        port.send(&[208 + channel - 1, value]);
                                    }
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

                                    if let Some(range) = recorder.get_range_for(key.id(), playback_pos, playback_pos + length) {
                                        for event in range {
                                            tx_feedback.send(TwisterMessage::Event(event.clone())).unwrap();
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
    Tempo,
    Swing
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
    fn id (&self) -> u32 {
        match self {
            &Control::Tempo => get_index(3, 3),
            &Control::Swing => get_index(3, 3),
            &Control::Param(channel, param) => get_index(channel, param as u32) 
        }
    }

    fn from_id (id: u32) -> Control {
        let col = id % 4;
        let row = id / 4;

        if col == 3 && row == 3 {
            Control::Tempo
        } else {
            Control::Param(row, match col {
                0 => ParamControl::Kaoss,
                1 => ParamControl::Reverb,
                2 => ParamControl::Delay,
                _ => ParamControl::Aftertouch
            })
        }
    }
}

fn get_index(row: u32, col: u32) -> u32 {
    row * 4 + col
}