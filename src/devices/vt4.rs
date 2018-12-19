use ::midi_connection;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::thread;
use ::chunk::{Triggerable, OutputValue, SystemTime, ScheduleMode};
use ::devices::MidiKeys;
use std::time::Duration;
use std::sync::atomic::{AtomicBool, Ordering};

pub use ::scale::{Scale, Offset};
pub use ::midi_connection::SharedMidiOutputConnection;

const ROBOT_ADDRESS: [u8; 4] = [0x10, 0x00, 0x00, 0x00];

pub struct VT4 {
    midi_keys: MidiKeys,
    last_key: Option<u8>,
    in_robot_mode: Arc<AtomicBool>,
    _midi_input: midi_connection::ThreadReference
}

impl VT4 {
    pub fn new (midi_output: midi_connection::SharedMidiOutputConnection, scale: Arc<Mutex<Scale>>, offset: Arc<Mutex<Offset>>) -> Self {
        let in_robot_mode = Arc::new(AtomicBool::new(false));
        let in_robot_mode_input = in_robot_mode.clone();
        let mut midi_output_input = midi_output.clone();
        let input = midi_connection::get_input("VT-4", move |_stamp, message| {
            if message == &[176, 49, 0] {  
                // HANDLE ROBOT BUTTON PRESS
                // for some stupid reason, this CC only tells you if the button is pressed, 
                // not what the state of ROBOT is. So we have to use sysex to find out :(
                let req = VT4SysExMessage::new(VT4Command::Request, ROBOT_ADDRESS, sysex_length(1)).to_sysex().unwrap();
                midi_output_input.send(&req).unwrap();
            } else if VT4SysExMessage::is_sysex(message) { 
                // HANDLE ROBOT STATE RESPONSE
                let response = VT4SysExMessage::from(message).unwrap();
                match response.command {
                    VT4Command::Set => {
                        if response.address == ROBOT_ADDRESS {
                            in_robot_mode_input.store(response.data[0] > 0, Ordering::Relaxed)
                        }
                    },
                    _ => ()
                }
            }
        });

        VT4 {
            midi_keys: MidiKeys::new(midi_output, 1, scale, offset),
            last_key: None,
            in_robot_mode,
            _midi_input: input
        }
    }
}

impl Triggerable for VT4 {
    fn trigger (&mut self, id: u32, value: OutputValue, time: SystemTime) {
        self.in_robot_mode.store(true, Ordering::Relaxed);
        self.midi_keys.trigger(id, value, time)
    }

    fn on_tick (&mut self) {
        let in_robot_mode = self.in_robot_mode.load(Ordering::Relaxed);

        // only update key if not in robot mode (as it kills robot mode when it changes)
        if !in_robot_mode {
            let key;
            { // immutable borrow
                let scale = self.midi_keys.scale();
                let from_c = scale.root - 60;
                let base_key = modulo(from_c, 12);
                let offset = get_mode_offset(modulo(scale.scale, 7));
                key = modulo(base_key - offset, 12) as u8;
            }

            if Some(key) != self.last_key {
                self.midi_keys.midi_output.send(&[176, 48, key]).unwrap();
                self.last_key = Some(key);
                println!("Set Key {}", key);
            }
        }
    }

    fn schedule_mode (&self) -> ScheduleMode {
        ScheduleMode::Monophonic
    }
}

fn modulo (n: i32, m: i32) -> i32 {
    ((n % m) + m) % m
}

fn get_mode_offset (mode: i32) -> i32 {
    let mut offset = 0;
    let intervals = [2, 2, 1, 2, 2, 2, 1];

    for i in 0..6 {
        if (i as i32) >= mode {
            break
        }
        offset += intervals[i];
    }

    offset
}

#[derive(Debug, Eq, PartialEq, Copy, Clone, Hash)]
enum VT4Command {
    Request,
    Set,
    Unknown
}

impl VT4Command {
    fn from (value: u8) -> VT4Command {
        match value {
            0x11 => VT4Command::Request,
            0x12 => VT4Command::Set,
            _ => VT4Command::Unknown
        }
    }

    fn to_sysex (&self) -> Option<u8> {
        match self {
            VT4Command::Request => Some(0x11),
            VT4Command::Set => Some(0x12),
            _ => None
        }
    }
}

#[derive(Debug, Eq, PartialEq, Clone, Hash)]
struct VT4SysExMessage {
    command: VT4Command,
    address: [u8; 4],
    data: Vec<u8>
}


impl VT4SysExMessage {
    fn new (command: VT4Command, address: [u8; 4], data: Vec<u8>) -> Self {
        Self {
            command,
            address,
            data
        }
    }

    fn from (message: &[u8]) -> Option<Self> {
        if VT4SysExMessage::is_sysex(message) {
            let command = VT4Command::from(message[7]);
            let address: [u8; 4] = [message[8], message[9], message[10], message[11]];
            let data = message[12..(message.len() - 2)].to_vec();
            Some(Self { command, address, data })
        } else {
            None
        }
    }

    fn to_sysex (&self) -> Option<Vec<u8>> {
        if self.command.to_sysex().is_some() {
            let mut result = vec![0xF0, 0x41, 0x10, 0x00, 0x00, 0x00, 0x51, self.command.to_sysex().unwrap()];
            result.extend_from_slice(&self.address);
            result.extend_from_slice(&self.data);

            // generate roland checksum
            let mut sum: u8 = 0;
            for i in 8..result.len() {
                sum = (sum + result[i]) % 128;
            }
            result.push((128 - sum) % 128);

            // end of sysex
            result.push(0xF7);

            Some(result)
        } else {
            None
        }
    }

    fn is_sysex (message: &[u8]) -> bool {
        message.len() > 12 && &message[0..7] == &[240, 65, 16, 0, 0, 0, 81]
    }
}

fn sysex_length (length: u8) -> Vec<u8> {
    vec![0, 0, 0, length % 128]
}