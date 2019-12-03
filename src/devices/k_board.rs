use ::midi_connection;
use std::time::{SystemTime, Duration};
use std::thread;

use std::sync::{Arc, Mutex, MutexGuard};
use std::sync::mpsc;
use ::chunk::{Triggerable, OutputValue, TriggerModeChange};
use std::collections::HashSet;

pub use ::scale::{Scale, Offset};
pub use ::midi_connection::SharedMidiOutputConnection;

pub struct KBoard {
    tx: mpsc::Sender<KBoardMessage>,
    listeners: Arc<Mutex<Vec<Box<Fn(u32, OutputValue) + Send + 'static>>>>
}

impl KBoard {
    pub fn new (kboard_port_name: &str, midi_output: midi_connection::SharedMidiOutputConnection, channel: u8, scale: Arc<Mutex<Scale>>) -> Self {
        let (tx, rx) = mpsc::channel();

        let mut midi_output = midi_output;
        
        let kboard_port_name = String::from(kboard_port_name);

        let tx_output = tx.clone();
        let listeners = Arc::new(Mutex::new(Vec::new()));
        let listeners_loop = Arc::clone(&listeners);

        let mut kboard_output = midi_connection::get_shared_output(&kboard_port_name);

        let scale_poll = Arc::clone(&scale);
        let tx_poll = tx.clone();

        // check for changes to scale and broadcast
        thread::spawn(move || {
            let kboard_input = midi_connection::get_input(&kboard_port_name, move |stamp, message| {
                if message[0] == 144 {
                    tx_output.send(KBoardMessage::Input(message[1] as u32, message[2])).unwrap();
                } else if message[0] == 128 {
                    tx_output.send(KBoardMessage::Input(message[1] as u32, 0)).unwrap();
                }
            });

            let mut last_scale = Scale {root: 0, scale: 0, sample_group_a: 0, sample_group_b: 0};
            let mut tick = 0;
            loop {
                thread::sleep(Duration::from_millis(50));
                let current_scale = scale_poll.lock().unwrap();
                if last_scale != *current_scale {
                    last_scale = current_scale.clone();
                    tx_poll.send(KBoardMessage::RefreshScale).unwrap();
                }
                tx_poll.send(KBoardMessage::Tick(tick)).unwrap();
                tick += 1;
            } 
        });

        let scale_loop = Arc::clone(&scale);

        let tx_feedback = tx.clone();
        thread::spawn(move || {
            let mut notes = scale_loop.lock().unwrap().get_notes();
            let mut triggering = HashSet::new();
            let mut trigger_stack: Vec<(u32, u8)> = Vec::new();
            let mut active = HashSet::new();
            let mut selected = HashSet::new();

            let selecting_scale = true; 
            let mut last_refresh_scale = SystemTime::now();
            for msg in rx {
                match msg {
                    KBoardMessage::RefreshScale => {
                        if last_refresh_scale.elapsed().unwrap() > Duration::from_millis(1) {                            
                            if selecting_scale {
                                notes = scale_loop.lock().unwrap().get_notes();
                                for id in 0..25 {
                                    let value = if notes.contains(&id) {
                                        127
                                    } else {
                                        0
                                    };
                                    kboard_output.send(&[144, id as u8, value]);
                                }
                            }
                            last_refresh_scale = SystemTime::now();
                        }
                    },
                    KBoardMessage::RefreshActive => {
                        for id in 0..128 {
                            kboard_output.send(&[144, id as u8, 0]);
                        }
                        for id in &active {
                            kboard_output.send(&[144, *id as u8, 127]);
                        }
                    },
                    KBoardMessage::RefreshNote(id) => {
                        if !selecting_scale {
                            let value = if triggering.contains(&id) || active.contains(&id) {
                                127
                            } else {
                                0
                            };
                            kboard_output.send(&[144, id as u8, value]).unwrap();
                        }
                    },
                    KBoardMessage::Trigger(id, velocity) => {
                        if velocity > 0 {
                            triggering.insert(id);
                            midi_output.send(&[144 + channel - 1, id as u8, velocity]).unwrap();
                        } else {
                            triggering.remove(&id);
                            midi_output.send(&[128 + channel - 1, id as u8, 0]).unwrap();
                            tx_feedback.send(KBoardMessage::RefreshScale).unwrap();
                        }

                        tx_feedback.send(KBoardMessage::RefreshNote(id)).unwrap();
                    },
                    KBoardMessage::Input(id, velocity) => {
                        let output_value = if velocity > 0 {
                            OutputValue::On(velocity)
                        } else {
                            OutputValue::Off
                        };
                        let listeners: MutexGuard<Vec<Box<Fn(u32, OutputValue) + Send + 'static>>> = listeners_loop.lock().unwrap();
                        if listeners.len() > 0 {
                            for l in listeners.iter() {
                                l(id, output_value)
                            }
                        } else {
                            tx_feedback.send(KBoardMessage::Trigger(id, output_value.value())).unwrap();
                        }
                    },
                    KBoardMessage::TriggerMode(state) => {
                        match state {
                            TriggerModeChange::SelectingScale(value) => {
                                // selecting_scale = value;
                                // tx_feedback.send(KBoardMessage::RefreshScale);

                                // if !selecting_scale {
                                //     tx_feedback.send(KBoardMessage::RefreshActive);
                                // }
                            },
                            TriggerModeChange::Active(id, value) => {

                                let updated = if value {
                                    active.insert(id)
                                } else {
                                    active.remove(&id)
                                };

                                if updated {
                                    tx_feedback.send(KBoardMessage::RefreshNote(id)).unwrap();
                                }
                            },
                            TriggerModeChange::Selected(id, value) => {
                                let updated = if value {
                                    selected.insert(id)
                                } else {
                                    selected.remove(&id)
                                };

                                if updated {
                                    tx_feedback.send(KBoardMessage::RefreshNote(id)).unwrap();
                                    tx_feedback.send(KBoardMessage::RefreshScale).unwrap();
                                }
                            }
                        }
                    },
                    KBoardMessage::Tick(value) => {
                        let output_value = if value % 2 == 0 { 127 } else { 0 };
                        let output_value_selected = if value % 8 < 7 { 127 } else { 0 };

                        if selecting_scale {
                            let output_value_root = if value % 8 < 4 { 127 } else { 0 };
                            let scale = scale_loop.lock().unwrap();
                            kboard_output.send(&[144, scale.root as u8, output_value_root]).unwrap();
                            kboard_output.send(&[144, (scale.root + 12) as u8, output_value_root]).unwrap();
                        }

                        for id in &triggering {
                            kboard_output.send(&[144, *id as u8, output_value]).unwrap();
                        }

                        for id in &selected {
                            kboard_output.send(&[144, *id as u8, output_value_selected]).unwrap();
                        }
                    }
                }
            }
        });
        
        KBoard {
            listeners,
            tx
        }
    }
}


enum KBoardMessage {
    RefreshScale,
    RefreshActive,
    RefreshNote(u32),
    Trigger(u32, u8),
    Input(u32, u8),
    TriggerMode(TriggerModeChange),
    Tick(u64)
}