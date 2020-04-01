extern crate midir;
extern crate regex;

use self::regex::Regex;
pub use self::midir::{MidiInput, MidiOutput, MidiInputConnection, MidiOutputConnection, ConnectError, ConnectErrorKind, PortInfoError, SendError};
use std::sync::{Arc, Mutex};
use std::sync::mpsc;
use std::thread;
use std::collections::HashMap;
use std::time::Duration;
pub use std::time::SystemTime;
type Listener = Box<dyn Fn(&mut MidiOutputConnection) + Send + 'static>;

const APP_NAME: &str = "Loop Drop";

struct OutputState {
    port: Option<MidiOutputConnection>,
    listeners: Vec<Listener>,
    current_values: HashMap<(u8, u8), u8>
}

impl OutputState {
    fn notify_listeners (&mut self) {
        if let Some(ref mut port) = self.port {
            for listener in &self.listeners {
                listener(port)
            }
        }
    }

    fn resend (&mut self) {
        if let Some(ref mut port) = self.port {
            for ((msg, id), value) in self.current_values.clone() {
                // resend 0 for CCs, but not for anything else
                if (msg >= 176 && msg < 192) || value > 0 {
                    port.send(&[msg, id, value]).unwrap();
                }
            }
        }
    }
}

pub fn get_shared_output (port_name: &str) -> SharedMidiOutputConnection {
    let state = Arc::new(Mutex::new(OutputState {
        port: None,
        listeners: Vec::new(),
        current_values: HashMap::new()
    }));

    let state_l = state.clone();
    let port_name_notify = String::from(port_name);
    let port_name_msg = String::from(port_name);

    // reconnect loop
    thread::spawn(move || {
        let mut has_port = false;
        loop {
            let output = MidiOutput::new(APP_NAME).unwrap();
            let current_port_id = get_outputs(&output).iter().position(|item| item == &port_name_notify);
            if current_port_id.is_some() != has_port {
                let mut state = state_l.lock().unwrap();
                state.port = get_output(&port_name_msg);
                state.notify_listeners();
                state.resend();
                has_port = state.port.is_some();
            }
            thread::sleep(Duration::from_secs(1));
        }
    });

    SharedMidiOutputConnection { 
        state
    }
}

pub fn get_input<F> (port_name: &str, callback: F) -> ThreadReference
where F: FnMut(u64, &[u8]) + Send + 'static {
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
            let current_port = get_inputs(&input).iter().position(|item| item == &port_name_notify);
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
    let port_number = match get_outputs(&output).iter().position(|item| item == port_name) {
        None => return None,
        Some(value) => value
    };
    output.connect(port_number, port_name).ok()
}

pub fn get_outputs (output: &MidiOutput) -> Vec<String> {
    let mut result = Vec::new();

    for i in 0..output.port_count() {
        // for some reason, sometimes the port doesn't exist -- use empty string
        result.push(output.port_name(i).unwrap_or(String::from("")));
    }

    normalize_port_names(&result)
}

pub fn get_inputs (input: &MidiInput) -> Vec<String> {
    let mut result = Vec::new();

    for i in 0..input.port_count() {
        // for some reason, sometimes the port doesn't exist -- use empty string
        result.push(input.port_name(i).unwrap_or(String::from("")));
    }

    normalize_port_names(&result)
}

fn normalize_port_names (names: &Vec<String>) -> Vec<String> {
    lazy_static! {
        static ref RE: Regex = Regex::new(r"^([0-9]- )?(.+?)( [0-9]+:([0-9]+))?$").unwrap();
    }

    let mut result = Vec::new();

    for name in names {
        let base_device_name = RE.replace(name, "${2}").into_owned();
        let device_port_index = RE.replace(name, "${4}").parse::<u32>().unwrap_or(0);
        let mut device_index = 0;
        let mut device_name = build_name(&base_device_name, device_index, device_port_index);

        // find an available device name (deal with multiple devices with the same name)
        while result.contains(&device_name) {
            device_index += 1;
            device_name = build_name(&base_device_name, device_index, device_port_index);
        }

        result.push(device_name);
    }

    result
}

fn build_name (base: &str, device_id: u32, port_id: u32) -> String {
    let mut result = String::from(base);
    if device_id > 0 {
        result.push_str(&format!(" {}", device_id + 1))
    }
    if port_id > 0 {
        result.push_str(&format!(" PORT {}", port_id + 1))
    }
    result
}

#[derive(Clone)]
pub struct SharedMidiOutputConnection {
    state: Arc<Mutex<OutputState>>
}

impl SharedMidiOutputConnection {
    pub fn send(&mut self, message: &[u8]) -> Result<(), SendError> {
        let mut state = self.state.lock().unwrap();
        
        if message.len() == 3 {
            state.current_values.insert((message[0], message[1]), message[2]);
        }
        
        if let Some(ref mut port) = state.port {
            port.send(message)
        } else {
            Ok(())
        }
    }

    pub fn on_connect<F>(&mut self, callback: F) where F: Fn(&mut MidiOutputConnection) + Send + 'static {
        let mut state = self.state.lock().unwrap();
        state.listeners.push(Box::new(callback));
    }
}

#[derive(Debug, Clone)]
struct MidiInputMessage {
    stamp: u64,
    data: Vec<u8>
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