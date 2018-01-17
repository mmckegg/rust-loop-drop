extern crate midir;

use self::midir::{MidiInput, MidiOutput, MidiInputConnection, MidiOutputConnection, ConnectError, ConnectErrorKind, PortInfoError};

const APP_NAME: &str = "Loop Drop";

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
        result.push(output.port_name(i).unwrap());
    }

    result
}

pub fn get_inputs () -> Vec<String> {
    let input = MidiInput::new(APP_NAME).unwrap();
    let mut result = Vec::new();

    for i in 0..input.port_count() {
        result.push(input.port_name(i).unwrap());
    }

    result
}

fn get_input_port_index (input: &MidiInput, name: &str) -> Result<usize, PortInfoError> {
    for i in 0..input.port_count() {
        if input.port_name(i).unwrap() == name {
            return Ok(i);
        }
    }
    return Err(PortInfoError::CannotRetrievePortName)
}

fn get_output_port_index (output: &MidiOutput, name: &str) -> Result<usize, PortInfoError> {
    for i in 0..output.port_count() {
        if output.port_name(i).unwrap() == name {
            return Ok(i);
        }
    }
    return Err(PortInfoError::CannotRetrievePortName)
}