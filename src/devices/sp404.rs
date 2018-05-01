use ::chunk::{Triggerable, OutputValue, SystemTime};
use ::midi_connection;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
pub use ::scale::Scale;
use std::sync::mpsc;
use std::thread;
use std::collections::HashSet;

use std::collections::HashMap;

pub struct SP404 {
    tx: mpsc::Sender<SP404Message>
}

impl SP404 {
    pub fn new (midi_port: midi_connection::SharedMidiOutputConnection, midi_channel: u8, offset: Arc<AtomicUsize>) -> Self {
        let (tx, rx) = mpsc::channel();

        let tx_feedback = tx.clone();

        thread::spawn(move || {
            let mut to_choke = HashSet::new();
            let mut output_values: HashMap<u32, (u8, u8, u8)> = HashMap::new();
            let mut midi_port = midi_port;

            for msg in rx {
                match msg {
                    SP404Message::Trigger(id, _) => {
                        let mut offset_value = offset.load(Ordering::Relaxed);

                        let mut channel = if offset_value < 5 {
                            midi_channel
                        } else {
                            midi_channel + 1
                        };

                        // choke
                        for (_, &(channel, note_id, _)) in &output_values {
                            // add to queue so that chokes are processed after note ons
                            to_choke.insert((channel, note_id));
                            tx_feedback.send(SP404Message::Choke(channel, note_id)).unwrap();                            
                        }

                        output_values.clear();

                        let note_id = (47 + ((offset_value % 5) * 12) + (id as usize)) as u8;
                        midi_port.send(&[144 - 1 + channel, note_id, 127]).unwrap();
                        output_values.insert(id, (channel, note_id, 127));

                        // since this note has been added, we don't need to choke it any more
                        to_choke.remove(&(channel, note_id));
                    },
                    SP404Message::Choke(channel, note_id) => {
                        // only trigger this choke if it is still valid
                        if to_choke.contains(&(channel, note_id)) {
                            midi_port.send(&[128 - 1 + channel, note_id, 0]).unwrap();
                        }

                        to_choke.remove(&(channel, note_id));
                    }
                }
            }
        });




        SP404 {
            tx
        }
    }
}

impl Triggerable for SP404 {
    fn trigger (&mut self, id: u32, value: OutputValue, _at: SystemTime) {
        match value {
            OutputValue::Off => {
                // if self.output_values.contains_key(&id) {
                //     let (channel, note_id, _) = *self.output_values.get(&id).unwrap();
                //     self.midi_port.send(&[128 - 1 + channel, note_id, 0]).unwrap();
                //     self.output_values.remove(&id);
                // }
            },
            OutputValue::On(velocity) => {
                self.tx.send(SP404Message::Trigger(id, velocity)).unwrap();
            }
        }
    }
}

enum SP404Message {
    Choke(u8, u8),
    Trigger(u32, u8)
}