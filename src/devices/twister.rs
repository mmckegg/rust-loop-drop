use ::midi_connection;
use std::sync::mpsc;
use ::loop_recorder::{LoopRecorder, LoopEvent};
use ::midi_keys::{Offset, Scale};
use ::clock_source::{RemoteClock, FromClock, ToClock, MidiTime};
use ::output_value::OutputValue;

use std::sync::{Arc, Mutex};
use std::thread;
use std::collections::HashMap;
use std::collections::hash_map::Entry::{Occupied, Vacant};

pub struct Twister {
    port_name: String,
    midi_input: midi_connection::MidiInputConnection<()>
}

impl Twister {
    pub fn new (port_name: &str, offsets: Vec<Arc<Mutex<Offset>>>, scale: Arc<Mutex<Scale>>, clock: RemoteClock) -> Self {
        let (tx, rx) = mpsc::channel();
        let clock_sender = clock.sender.clone();

        let tx_clock = tx.clone();
        let tx_feedback = tx.clone();

        // pipe clock in
        thread::spawn(move || {
            for msg in clock.receiver {
                tx_clock.send(TwisterMessage::Clock(msg)).unwrap();
            }
        });

        let mut output = midi_connection::get_output(port_name).unwrap();

        let input = midi_connection::get_input(port_name, move |stamp, message, _| {
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

        // Refresh Display
        tx_feedback.send(TwisterMessage::Refresh(Control::Tempo));
        tx_feedback.send(TwisterMessage::Refresh(Control::Swing));
        tx_feedback.send(TwisterMessage::Refresh(Control::ScaleOffset));
        tx_feedback.send(TwisterMessage::Refresh(Control::RootOffset));
        for row in 0..4 {
            for col in 0..3 {
                tx_feedback.send(TwisterMessage::Refresh(Control::ScaleParam(col, row)));
            }
            for col in 0..4 {
                tx_feedback.send(TwisterMessage::Refresh(Control::Param(col, row)));
            }
        }

        thread::spawn(move || {
            let mut recorder = LoopRecorder::new();
            let mut last_pos = MidiTime::zero();
            let mut last_tempo = 120;
            let mut record_start_times = HashMap::new();
            let mut loops: HashMap<Control, Loop> = HashMap::new();

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

                        match control {
                            Control::ScaleParam(channel, index) => {
                                if let Some(offset) = offsets.get(channel as usize) {
                                    let mut current = offset.lock().unwrap();
                                    let offset = get_offset(value);
                                    match index {
                                        0 => current.oct = offset,
                                        1 => current.third = offset,
                                        2 => current.offset = offset,
                                        _ => current.pitch = offset
                                    };
                                }
                            },
                            Control::Tempo => {
                                clock_sender.send(ToClock::SetTempo(value.value() as usize + 60)).unwrap();
                            },
                            Control::Swing => {

                            },
                            Control::ScaleOffset => {
                                let mut current_scale = scale.lock().unwrap();
                                current_scale.scale = get_offset(value);
                            },
                            Control::RootOffset => {
                                let mut current_scale = scale.lock().unwrap();
                                current_scale.root = 69 + get_offset(value);
                            },
                            Control::Param(channel, index) => {

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
                            Control::RootOffset => get_midi_value(scale.lock().unwrap().root),
                            Control::ScaleOffset => get_midi_value(scale.lock().unwrap().scale),
                            Control::ScaleParam(channel, index) => {
                                if let Some(offset) = offsets.get(channel as usize) {
                                    let mut current = offset.lock().unwrap();
                                    get_midi_value(match index {
                                        0 => current.oct,
                                        1 => current.third,
                                        2 => current.offset,
                                        _ => current.pitch
                                    })
                                } else {
                                    0
                                }
                            },
                            Control::Param(channel, index) => 0
                        };

                        output.send(&[176, control.id() as u8, value]);
                    },

                    TwisterMessage::Clock(msg) => {
                        match msg {
                            FromClock::Schedule { pos, length } => {
                                for (key, value) in &loops {
                                    let offset = value.offset % value.length;
                                    let playback_pos = value.offset + ((pos - offset) % value.length);
                                    
                                    if playback_pos == value.offset {

                                    }

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
            port_name: String::from(port_name),
            midi_input: input
        }
    }
}

#[derive(Debug)]
enum TwisterMessage {
    ControlChange(Control, OutputValue),
    Event(LoopEvent),
    Refresh(Control),
    Recording(Control, bool),
    TapTempo,
    Clock(FromClock)
}

#[derive(Debug, Eq, PartialEq, Hash, Clone)]
enum Control {
    ScaleParam(u32, u32),
    Param(u32, u32),
    RootOffset,
    ScaleOffset,
    Tempo,
    Swing
}

#[derive(Debug)]
struct Loop {
    offset: MidiTime,
    length: MidiTime
}

impl Control {
    fn id (&self) -> u32 {
        match self {
            &Control::Tempo => get_index(0, 3),
            &Control::Swing => get_index(1, 3),
            &Control::RootOffset => get_index(2, 3),
            &Control::ScaleOffset => get_index(3, 3),
            &Control::ScaleParam(channel, index) => get_index(index, channel),
            &Control::Param(channel, index) => get_index(index + 4, channel) 
        }
    }

    fn from_id (id: u32) -> Control {
        let col = id % 4;
        let row = id / 4;

        if row < 4 { // bank 1
            if col < 3 {
                Control::ScaleParam(col, row)
            } else {
                match row {
                    0 => Control::Tempo,
                    1 => Control::Swing,
                    2 => Control::RootOffset,
                    _ => Control::ScaleOffset
                }
            }
        } else { // bank 2+
            Control::Param(col, row + 4)
        }
    }
}

fn get_index(row: u32, col: u32) -> u32 {
    row * 4 + col
}

fn get_offset (midi_value: OutputValue) -> i32 {
    let ival = midi_value.value() as i32;
    if ival < 2 {
        -5
    } else if ival > 126 {
        5
    } else if ival < 63 {
        -4 + ival / 16
    }  else if ival > 64 {
        -3 + (ival + 1) / 16
    } else {
        0
    }
}

fn get_midi_value (offset: i32) -> u8 {
    *[0, 7, 20, 40, 50, 64, 70, 85, 100, 120, 127].get((offset + 5) as usize).unwrap_or(&64)
}