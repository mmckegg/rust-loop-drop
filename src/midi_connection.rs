extern crate midir;
extern crate regex;

use self::regex::Regex;
pub use self::midir::{MidiInput, MidiOutput, MidiInputConnection, MidiOutputConnection, ConnectError, ConnectErrorKind, PortInfoError, SendError};
use std::sync::mpsc;
use std::thread;
use std::collections::HashMap;
use std::time::Duration;
use std::sync::Arc;
pub use std::time::SystemTime;

const APP_NAME: &str = "Loop Drop";

pub fn get_shared_output (port_name: &str) -> SharedMidiOutputConnection {
    let mut current_output: Option<MidiOutputConnection> = None;

    let (tx, rx) = mpsc::channel();
    let port_name_notify = String::from(port_name);
    let port_name_msg = String::from(port_name);
    let (queue_tx, queue) = mpsc::channel::<(MidiMessage, SystemTime)>();

    let tx_notify = tx.clone();

    // reconnect loop
    thread::spawn(move || {
        let mut last_port = None;
        loop {
            let output = MidiOutput::new(APP_NAME).unwrap();
            let current_port = get_output_port_index(&output, &port_name_notify);
            if last_port.is_some() != current_port.is_some() {
                tx_notify.send(SharedMidiOutputEvent::Changed).unwrap();
                last_port = current_port;
            }
            thread::sleep(Duration::from_secs(1));
        }
    });

    let tx_queue = tx.clone();

    // scheduled send queue
    thread::spawn(move || {
        for msg in queue {
            let now = SystemTime::now();
            if msg.1 > now {
                thread::sleep(msg.1.duration_since(now).unwrap());
            }
            tx_queue.send(SharedMidiOutputEvent::Send(msg.0)).unwrap();
        }
    });

    // event loop
    thread::spawn(move || {
        let mut current_values: HashMap<(u8, u8), u8> = HashMap::new();
        for msg in rx {
            match msg {
                SharedMidiOutputEvent::Send(midi_msg) => match midi_msg {
                    MidiMessage::One(a) => send_and_save(&mut current_output, &mut current_values, &[a]),
                    MidiMessage::Two(a, b) => send_and_save(&mut current_output, &mut current_values, &[a, b]),
                    MidiMessage::Three(a, b, c) => send_and_save(&mut current_output, &mut current_values, &[a, b, c]),
                    MidiMessage::Sysex(data) => send_and_save(&mut current_output, &mut current_values, data.as_slice()),
                },

                SharedMidiOutputEvent::SendAt(midi_msg, time) => {
                    queue_tx.send((midi_msg, time));
                },

                SharedMidiOutputEvent::Changed => {
                    if let Some(port) = current_output {
                        port.close();
                    }
                    current_output = get_output(&port_name_msg);
                    match current_output {
                        Some(ref mut port) => {
                            // send current values
                            for (&(msg, id), value) in &current_values {
                                if value > &0 {
                                    port.send(&[msg, id, *value]);
                                }
                            }
                        },
                        None => ()
                    }
                }
            };
        }
    });
    SharedMidiOutputConnection { tx }
}

pub fn get_input<F> (port_name: &str, callback: F) -> ThreadReference
where F: FnMut(u64, &[u8]) + Send + 'static {
    let mut current_output: Option<MidiOutputConnection> = None;
    let port_name_notify = String::from(port_name);
    let (tx, rx) = mpsc::channel::<MidiInputMessage>();

    thread::spawn(move || {
        let mut callback = callback;
        for msg in rx {
            callback(msg.stamp, &msg.data)
        }
    });

    thread::spawn(move || {
        let mut last_port = None;
        let mut current_input: Option<MidiInputConnection<()>> = None;
        loop {
            let input = MidiInput::new(APP_NAME).unwrap();
            let current_port = get_input_port_index(&input, &port_name_notify);
            if last_port.is_some() != current_port.is_some() {
                if let Some(current_input) = current_input {
                    current_input.close();
                }
                current_input = match current_port {
                    Some(current_port) => {
                        let tx_input = tx.clone();

                        input.connect(current_port, &port_name_notify, move |stamp, msg, _| {
                            tx_input.send(MidiInputMessage {stamp, data: Vec::from(msg)}).unwrap();
                        }, ()).ok()
                    },
                    None => None
                };
                last_port = current_port;
            }
            thread::sleep(Duration::from_secs(1));
        }
    });

    ThreadReference {}
}

pub fn get_output (port_name: &str) -> Option<MidiOutputConnection> {
    let output = MidiOutput::new(APP_NAME).unwrap();
    let port_number = match get_output_port_index(&output, port_name) {
        None => return None,
        Some(value) => value
    };
    output.connect(port_number, port_name).ok()
}

pub fn get_outputs () -> Vec<String> {
    let output = MidiOutput::new(APP_NAME).unwrap();
    let mut result = Vec::new();

    for i in 0..output.port_count() {
        result.push(normalize_port_name(&output.port_name(i).unwrap()));
    }

    result
}

pub fn get_inputs () -> Vec<String> {
    let input = MidiInput::new(APP_NAME).unwrap();
    let mut result = Vec::new();

    for i in 0..input.port_count() {
        result.push(normalize_port_name(&input.port_name(i).unwrap()));
    }

    result
}

fn get_input_port_index (input: &MidiInput, name: &str) -> Option<usize> {
    let normalized_name = normalize_port_name(name);
    for i in 0..input.port_count() {
        if let &Ok(ref name) = &input.port_name(i) {
            if normalize_port_name(&name) == normalized_name {
                return Some(i);
            }
        }
    }
    None
}

fn get_output_port_index (output: &MidiOutput, name: &str) -> Option<usize> {
    let normalized_name = normalize_port_name(name);
    for i in 0..output.port_count() {
        if let &Ok(ref name) = &output.port_name(i) {
            if normalize_port_name(&name) == normalized_name {
                return Some(i);
            }
        }
    }
    None
}

fn normalize_port_name (name: &str) -> String {
    lazy_static! {
        static ref RE: Regex = Regex::new(r"^([0-9]- )?(.+?)( [0-9]+:[0-9]+)?$").unwrap();
    }
    RE.replace(name, "${2}").into_owned()
}

#[derive(Debug, Clone)]
pub struct SharedMidiOutputConnection {
    tx: mpsc::Sender<SharedMidiOutputEvent>
}

impl SharedMidiOutputConnection {
    pub fn send_at(&mut self, message: &[u8], time: SystemTime) -> Result<(), SendError> {
        let now = SystemTime::now();

        // send straight away if time is in the past or no more than 0.9ms in future
        if time.duration_since(now).unwrap_or(Duration::from_millis(0)) < Duration::from_micros(900) {
            self.send(message)
        } else {
            let msg = try!(format_midi_message(message));
            if let Err(_) = self.tx.send(SharedMidiOutputEvent::SendAt(msg, time)) {
                return Err(SendError::Other("could not send message, thread might be dead"));
            }

            Ok(())
        }
    }

    pub fn send(&mut self, message: &[u8]) -> Result<(), SendError> {
        let msg = try!(format_midi_message(message));

        if let Err(_) = self.tx.send(SharedMidiOutputEvent::Send(msg)) {
            return Err(SendError::Other("could not send message, thread might be dead"));
        }

        Ok(())
    }
}

#[derive(Debug, Clone)]
enum MidiMessage {
    One(u8),
    Two(u8, u8),
    Three(u8, u8, u8),
    Sysex(Vec<u8>)
}

enum SharedMidiOutputEvent {
    Send(MidiMessage),
    SendAt(MidiMessage, SystemTime),
    Changed
}

#[derive(Debug, Clone)]
struct MidiInputMessage {
    stamp: u64,
    data: Vec<u8>
}

pub fn send_and_save (output: &mut Option<MidiOutputConnection>, save_dest: &mut HashMap<(u8, u8), u8>, message: &[u8]) {
    match output {
        &mut Some(ref mut port) => {
            port.send(message).unwrap();
        },
        &mut None => ()
    }
    if message.len() == 3 {
        save_dest.insert((message[0], message[1]), message[2]);
    }
}

pub struct ThreadReference {
    //tx: mpsc::Sender<()>
}

impl Drop for ThreadReference {
    fn drop(&mut self) {
        println!("DROP NOT IMPLEMENTED")
        //self.tx.send(()).unwrap();
    }
}

fn format_midi_message(message: &[u8]) -> Result<MidiMessage, SendError> {
    let nbytes = message.len();
    if nbytes == 0 {
        return Err(SendError::InvalidData("message to be sent must not be empty"));
    }

    if message[0] == 0xF0 { // Sysex message
        // Allocate buffer for sysex data and copy message
        Ok(MidiMessage::Sysex(message.to_vec()))
    } else { // Channel or system message.
        // Make sure the message size isn't too big.
        if nbytes > 3 {
            return Err(SendError::InvalidData("non-sysex message must not be longer than 3 bytes"));
        } 
        
        let msg = if nbytes == 3 {
            MidiMessage::Three(message[0], message[1], message[2])
        } else if nbytes == 2 {
            MidiMessage::Two(message[0], message[1])
        } else {
            MidiMessage::One(message[0])
        };

        Ok(msg)
    }
}