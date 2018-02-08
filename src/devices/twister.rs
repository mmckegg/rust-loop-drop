use ::midi_connection;
use std::sync::mpsc;
use ::midi_keys::{Offset, Scale};
use ::clock_source::{RemoteClock, FromClock, ToClock};

use std::sync::{Arc, Mutex};
use std::thread;

pub struct Twister {
    port_name: String,
    midi_input: midi_connection::MidiInputConnection<()>
}

impl Twister {
    pub fn new (port_name: &str, offsets: Vec<Arc<Mutex<Offset>>>, scale: Arc<Mutex<Scale>>, clock: RemoteClock) -> Self {
        let (tx, rx) = mpsc::channel();
        let clock_sender = clock.sender.clone();

        let tx_clock = tx.clone();

        thread::spawn(move || {
            for msg in clock.receiver {
                match msg {
                    FromClock::Schedule {pos, length} => {

                    },
                    FromClock::Tempo(value) => {
                        let msg = UpdateMessage::Tempo(value);
                        tx_clock.send(TwisterMessage::Update(msg)).unwrap();
                    },
                    FromClock::Jump => ()
                }
            }
        });

        let mut output = midi_connection::get_output(port_name).unwrap();

        let input = midi_connection::get_input(port_name, move |stamp, message, _| {
            let col = (message[1] % 4) as usize;
            let row = message[1] / 4;


            if row < 4 { // bank 1
                if col < 3 {
                    if message[0] == 176 {
                        let result = match row {
                            0 => TwisterMessage::OctOffset(col, get_offset(message[2])),
                            1 => TwisterMessage::ThirdOffset(col, get_offset(message[2])),
                            2 => TwisterMessage::Offset(col, get_offset(message[2])),
                            _ => TwisterMessage::PitchOffset(col, get_offset(message[2]))
                        };

                        tx.send(result).unwrap();
                    } else if message[0] == 177 && message[2] == 0 {
                        let result = match row {
                            0 => TwisterMessage::ResetOctOffset(col),
                            1 => TwisterMessage::ResetThirdOffset(col),
                            2 => TwisterMessage::ResetOffset(col),
                            _ => TwisterMessage::ResetPitchOffset(col)
                        };

                        tx.send(result).unwrap();
                    }
                } else if col == 3 {
                    if message[0] == 176 {
                        let result = match row {
                            0 => TwisterMessage::Tempo(message[2]),
                            1 => TwisterMessage::Swing(message[2]),
                            2 => TwisterMessage::RootOffset(get_offset(message[2])),
                            _ => TwisterMessage::ScaleOffset(get_offset(message[2]))
                        };

                        tx.send(result).unwrap();
                    } else if message[0] == 177 && row == 0 && message[2] == 0 {
                        tx.send(TwisterMessage::TapTempo).unwrap();
                    }
                }
            }
        }, ()).unwrap();

        for (col, mutex) in offsets.iter().enumerate() {
            if col <= 3 {
                let offset = mutex.lock().unwrap();
                output.send(&[176, col as u8, get_midi_value(offset.oct)]);
                output.send(&[176, col as u8 + 4, get_midi_value(offset.third)]);
                output.send(&[176, col as u8 + 4 * 2, get_midi_value(offset.offset)]);
                output.send(&[176, col as u8 + 4 * 3, get_midi_value(offset.pitch)]);
            }
        }

        thread::spawn(move || {
            for received in rx {
                match received {
                    TwisterMessage::OctOffset(index, value) => {
                        if let Some(offset) = offsets.get(index) {
                            let mut current = offset.lock().unwrap();
                            current.oct = value;
                        }
                    },
                    TwisterMessage::ResetOctOffset(index) => {
                        if let Some(offset) = offsets.get(index) {
                            let mut current = offset.lock().unwrap();
                            current.oct = 0;
                            output.send(&[176, index as u8, get_midi_value(current.oct)]);   
                        }  
                    },
                    TwisterMessage::ThirdOffset(index, value) => {
                        if let Some(offset) = offsets.get(index) {
                            let mut current = offset.lock().unwrap();
                            current.third = value;
                        }
                    },
                    TwisterMessage::ResetThirdOffset(index) => {
                        if let Some(offset) = offsets.get(index) {
                            let mut current = offset.lock().unwrap();
                            current.third = 0;
                            output.send(&[176, index as u8 + 4, get_midi_value(current.third)]);   
                        }  
                    },
                    TwisterMessage::Offset(index, value) => {
                        if let Some(offset) = offsets.get(index) {
                            let mut current = offset.lock().unwrap();
                            current.offset = value;
                        }
                    },
                    TwisterMessage::ResetOffset(index) => {
                        if let Some(offset) = offsets.get(index) {
                            let mut current = offset.lock().unwrap();
                            current.offset = 0;
                            output.send(&[176, index as u8 + 4 * 2, get_midi_value(current.offset)]);   
                        }  
                    },
                    TwisterMessage::PitchOffset(index, value) => {
                        if let Some(offset) = offsets.get(index) {
                            let mut current = offset.lock().unwrap();
                            current.pitch = value;
                        }
                    },
                    TwisterMessage::ResetPitchOffset(index) => {
                        if let Some(offset) = offsets.get(index) {
                            let mut current = offset.lock().unwrap();
                            current.pitch = 0;
                            output.send(&[176, index as u8 + 4 * 3, get_midi_value(current.pitch)]);   
                        }  
                    },
                    TwisterMessage::RootOffset(value) => {
                        let mut current_scale = scale.lock().unwrap();
                        current_scale.root = 69 + value;
                    },
                    TwisterMessage::ScaleOffset(value) => {
                        let mut current_scale = scale.lock().unwrap();
                        current_scale.scale = value;
                    },
                    TwisterMessage::Tempo(value) => {
                        clock_sender.send(ToClock::SetTempo(value as usize + 60)).unwrap();
                    },
                    TwisterMessage::TapTempo => {
                        clock_sender.send(ToClock::TapTempo).unwrap();
                    },
                    TwisterMessage::Swing(value) => {

                    },
                    TwisterMessage::Update(value) => {
                        match value {
                            UpdateMessage::Tempo(value) => {
                                output.send(&[176, 3, (value - 60) as u8]);
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
    OctOffset(usize, i32),
    ThirdOffset(usize, i32),
    Offset(usize, i32),
    PitchOffset(usize, i32),
    RootOffset(i32),
    ScaleOffset(i32),
    ResetOctOffset(usize),
    ResetThirdOffset(usize),
    ResetOffset(usize),
    ResetPitchOffset(usize),
    Tempo(u8),
    Swing(u8),
    TapTempo,

    Update(UpdateMessage)
}

#[derive(Debug)]
enum UpdateMessage {
    Tempo(usize)
}

fn get_offset (midi_value: u8) -> i32 {
    let ival = midi_value as i32;
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