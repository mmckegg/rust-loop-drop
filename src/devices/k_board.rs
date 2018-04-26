use ::midi_connection;
use std::collections::HashMap;
use std::time::{SystemTime, Duration};
use std::sync::{Arc, Mutex};
use ::output_value::OutputValue;
use std::thread;
use std::sync::mpsc;

pub use ::scale::{Scale, Offset};
pub use ::midi_connection::SharedMidiOutputConnection;

pub struct KBoard {
    kboard_input: midi_connection::MidiInputConnection<()>
}

impl KBoard {
    pub fn new (kboard_port_name: &str, midi_output: midi_connection::SharedMidiOutputConnection, channel: u8, scale: Arc<Mutex<Scale>>) -> Self {
        let (tx, rx) = mpsc::channel();

        let mut midi_output = midi_output;
        let tx_output = tx.clone();

        let kboard_input = midi_connection::get_input(kboard_port_name, move |stamp, message, _| {
            if message[0] == 144 {
                midi_output.send(&[144 + channel - 1, message[1], message[2]]);
                tx_output.send(KBoardMessage::RefreshNote(message[1] as i32));
            } else if message[0] == 128 {
                midi_output.send(&[128 + channel - 1, message[1], message[2]]);
                tx_output.send(KBoardMessage::RefreshScale);
            }
        }, ()).unwrap();

        let mut kboard_output = midi_connection::get_shared_output(kboard_port_name).unwrap();

        let scale_poll = Arc::clone(&scale);
        let tx_poll = tx.clone();

        // check for changes to scale and broadcast
        thread::spawn(move || {
            let mut last_scale = Scale {root: 0, scale: 0, sample_group_a: 0, sample_group_b: 0};
            loop {
                thread::sleep(Duration::from_millis(16));
                let current_scale = scale_poll.lock().unwrap();
                if last_scale != *current_scale {
                    last_scale = current_scale.clone();
                    tx_poll.send(KBoardMessage::RefreshScale).unwrap();
               }
            } 
        });

        let scale_loop = Arc::clone(&scale);

        thread::spawn(move || {
            let mut notes = scale_loop.lock().unwrap().get_notes();
            for msg in rx {
                match msg {
                    KBoardMessage::RefreshScale => {
                        notes = scale_loop.lock().unwrap().get_notes();
                        for id in 0..128 {
                            let value = if notes.contains(&id) {
                                127
                            } else {
                                0
                            };
                            kboard_output.send(&[144, id as u8, value]);
                        }
                    },
                    KBoardMessage::RefreshNote(id) => {
                        let value = if notes.contains(&id) {
                            127
                        } else {
                            0
                        };
                        kboard_output.send(&[144, id as u8, value]);
                    }
                }
            }
        });
        
        KBoard {
            kboard_input
        }
    }



    // pub fn midi_output (&self) -> &midi_connection::SharedMidiOutputConnection {
    //     &self.midi_output
    // }
}

enum KBoardMessage {
    RefreshScale,
    RefreshNote(i32)
}