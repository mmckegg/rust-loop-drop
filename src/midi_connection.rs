extern crate midir;
extern crate regex;

use self::regex::Regex;
pub use self::midir::{MidiInput, MidiOutput, MidiInputConnection, MidiOutputConnection, ConnectError, ConnectErrorKind, PortInfoError, SendError};
use std::sync::mpsc;
use std::thread;

const APP_NAME: &str = "Loop Drop";

// lazy_static! {
//     static ref outputs: HashMap<String, mpsc::Sender<MidiMessage>> = HashMap::new();
// }

pub fn get_shared_output (port_name: &str) -> Result<SharedMidiOutputConnection, ConnectError<MidiOutput>> {
    let mut output = try!(get_output(port_name));
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        for msg in rx {
            (match msg {
                MidiMessage::One(a) => output.send(&[a]),
                MidiMessage::Two(a, b) => output.send(&[a, b]),
                MidiMessage::Three(a, b, c) => output.send(&[a, b, c]),
                MidiMessage::Sysex(data) => output.send(data.as_slice())
            }).unwrap();
        }
    });
    Ok(SharedMidiOutputConnection { tx })
}

pub fn get_output (port_name: &str) -> Result<MidiOutputConnection, ConnectError<MidiOutput>> {
    let output = MidiOutput::new(APP_NAME).unwrap();
    let port_number = match get_output_port_index(&output, port_name) {
        Err(_) => return Err(ConnectError::new(ConnectErrorKind::Other("No output port with specified name"), output)),
        Ok(value) => value
    };
    output.connect(port_number, port_name)
}

pub fn get_input<F, T: Send> (port_name: &str, callback: F, data: T) -> Result<MidiInputConnection<T>, ConnectError<MidiInput>> 
where F: FnMut(u64, &[u8], &mut T) + Send + 'static {
    let input = MidiInput::new(APP_NAME).unwrap();
    let port_number = match get_input_port_index(&input, port_name) {
        Err(_) => return Err(ConnectError::new(ConnectErrorKind::Other("No input port with specified name"), input)),
        Ok(value) => value
    };
    input.connect(port_number, port_name, callback, data)
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

fn get_input_port_index (input: &MidiInput, name: &str) -> Result<usize, PortInfoError> {
    let normalized_name = normalize_port_name(name);
    for i in 0..input.port_count() {
        if normalize_port_name(&input.port_name(i).unwrap()) == normalized_name {
            return Ok(i);
        }
    }
    return Err(PortInfoError::CannotRetrievePortName)
}

fn get_output_port_index (output: &MidiOutput, name: &str) -> Result<usize, PortInfoError> {
    let normalized_name = normalize_port_name(name);
    for i in 0..output.port_count() {
        if normalize_port_name(&output.port_name(i).unwrap()) == normalized_name {
            return Ok(i);
        }
    }
    return Err(PortInfoError::CannotRetrievePortName)
}

fn normalize_port_name (name: &str) -> String {
    lazy_static! {
        static ref RE: Regex = Regex::new(r"^([0-9]- )?(.+?)( [0-9]+:[0-9]+)?$").unwrap();
    }
    RE.replace(name, "${2}").into_owned()
}

#[derive(Debug, Clone)]
pub struct SharedMidiOutputConnection {
    tx: mpsc::Sender<MidiMessage>
}

impl SharedMidiOutputConnection {
    pub fn send(&mut self, message: &[u8]) -> Result<(), SendError> {
        let nbytes = message.len();
        if nbytes == 0 {
            return Err(SendError::InvalidData("message to be sent must not be empty"));
        }

        if message[0] == 0xF0 { // Sysex message
            // Allocate buffer for sysex data and copy message
            if let Err(_) = self.tx.send(MidiMessage::Sysex(message.to_vec())) {
                return Err(SendError::Other("could not send message, thread might be dead"));
            }
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

            if let Err(_) = self.tx.send(msg) {
                return Err(SendError::Other("could not send message, thread might be dead"));
            }
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