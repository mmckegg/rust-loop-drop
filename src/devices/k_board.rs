use ::midi_connection;
use std::time::{SystemTime, Duration};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::sync::mpsc;
use ::chunk::{Triggerable, OutputValue, TriggerModeChange};
use std::collections::HashSet;

pub use ::scale::{Scale, Offset};
pub use ::midi_connection::SharedMidiOutputConnection;

pub struct KBoard {
    kboard_input: midi_connection::MidiInputConnection<()>,
    tx: mpsc::Sender<KBoardMessage>,
    listeners: Arc<Mutex<Vec<Box<Fn(u32, OutputValue) + Send + 'static>>>>
}

impl KBoard {
    pub fn new (kboard_port_name: &str, midi_output: midi_connection::SharedMidiOutputConnection, channel: u8, scale: Arc<Mutex<Scale>>) -> Self {
        let (tx, rx) = mpsc::channel();

        let mut midi_output = midi_output;

        let tx_output = tx.clone();
        let listeners = Arc::new(Mutex::new(Vec::new()));
        let listeners_loop = Arc::clone(&listeners);

        let kboard_input = midi_connection::get_input(kboard_port_name, move |stamp, message, _| {
            if message[0] == 144 {
                tx_output.send(KBoardMessage::Input(message[1] as u32, message[2])).unwrap();
            } else if message[0] == 128 {
                tx_output.send(KBoardMessage::Input(message[1] as u32, 0)).unwrap();
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

        let tx_feedback = tx.clone();
        thread::spawn(move || {
            let mut notes = scale_loop.lock().unwrap().get_notes();
            let mut triggering = HashSet::new();
            let mut selecting_scale = false;
            for msg in rx {
                match msg {
                    KBoardMessage::RefreshScale => {
                        if selecting_scale {
                            notes = scale_loop.lock().unwrap().get_notes();
                            for id in 0..128 {
                                let value = if notes.contains(&id) {
                                    127
                                } else {
                                    0
                                };
                                kboard_output.send(&[144, id as u8, value]);
                            }
                        }
                    },
                    KBoardMessage::RefreshNote(id) => {
                        if !selecting_scale {
                            let value = if triggering.contains(&id) {
                                127
                            } else {
                                0
                            };
                            kboard_output.send(&[144, id as u8, value]);
                        }
                    },
                    KBoardMessage::Trigger(id, velocity) => {
                        if velocity > 0 {
                            midi_output.send(&[144 + channel - 1, id as u8, velocity]);
                            triggering.insert(id);
                        } else {
                            midi_output.send(&[128 + channel - 1, id as u8, 0]);
                            triggering.remove(&id);
                            tx_feedback.send(KBoardMessage::RefreshScale);
                        };
                        tx_feedback.send(KBoardMessage::RefreshNote(id));
                    },
                    KBoardMessage::Input(id, velocity) => {
                        let output_value = if velocity > 0 {
                            OutputValue::On(velocity)
                        } else {
                            OutputValue::Off
                        };
                        let listeners: MutexGuard<Vec<Box<Fn(u32, OutputValue) + Send + 'static>>> = listeners_loop.lock().unwrap();
                        for l in listeners.iter() {
                            l(id, output_value)
                        }
                    },
                    KBoardMessage::TriggerMode(state) => {
                        match state {
                            TriggerModeChange::SelectingScale(value) => {
                                selecting_scale = value;
                                tx_feedback.send(KBoardMessage::RefreshScale);

                                if !selecting_scale {
                                    for i in 0..128 {
                                        tx_feedback.send(KBoardMessage::RefreshNote(i));
                                    }
                                }
                            },
                            _ => ()
                        }
                    }
                }
            }
        });
        
        KBoard {
            kboard_input,
            listeners,
            tx
        }
    }



    // pub fn midi_output (&self) -> &midi_connection::SharedMidiOutputConnection {
    //     &self.midi_output
    // }
}

impl Triggerable for KBoard {
    fn trigger (&mut self, id: u32, value: OutputValue, _at: SystemTime) {
        self.tx.send(KBoardMessage::Trigger(id, value.value())).unwrap();
    }

    fn listen (&mut self, listener: Box<Fn(u32, OutputValue) + 'static + Send>) {
        let mut listeners = self.listeners.lock().unwrap();
        listeners.push(listener);
    }

    fn onTriggerModeChanged (&self, mode_change: TriggerModeChange) {
        self.tx.send(KBoardMessage::TriggerMode(mode_change)).unwrap();
    }

}

enum KBoardMessage {
    RefreshScale,
    RefreshNote(u32),
    Trigger(u32, u8),
    Input(u32, u8),
    TriggerMode(TriggerModeChange)
}