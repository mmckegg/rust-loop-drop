#[macro_use]
extern crate lazy_static;
extern crate midir;
extern crate regex;

use midir::{MidiInput, MidiInputConnection, MidiOutput};
use regex::Regex;
use std::collections::VecDeque;
use std::io::{self, Read};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const APP_NAME: &str = "Loop Drop MIDI Feedback Echo";
const DEFAULT_PORT_MATCH: &str = "LOOP DROP ";
const SELF_ECHO_WINDOW_MS: u64 = 50;

fn main() {
    let port_match = std::env::args()
        .nth(1)
        .unwrap_or_else(|| String::from(DEFAULT_PORT_MATCH));

    let input = MidiInput::new(APP_NAME).expect("could not create midi input");
    let output = MidiOutput::new(APP_NAME).expect("could not create midi output");

    let inputs = get_inputs(&input);
    let outputs = get_outputs(&output);

    let input_index = inputs
        .iter()
        .position(|name| name.contains(&port_match))
        .unwrap_or_else(|| {
            eprintln!("Could not find MIDI input matching {:?}", port_match);
            for (i, name) in inputs.iter().enumerate() {
                eprintln!("  IN {}: {}", i, name);
            }
            std::process::exit(1);
        });

    let output_index = outputs
        .iter()
        .position(|name| name.contains(&port_match))
        .unwrap_or_else(|| {
            eprintln!("Could not find MIDI output matching {:?}", port_match);
            for (i, name) in outputs.iter().enumerate() {
                eprintln!("  OUT {}: {}", i, name);
            }
            std::process::exit(1);
        });

    println!("Input : {}", inputs[input_index]);
    println!("Output: {}", outputs[output_index]);
    println!("Echoing MIDI back to controller. Press Ctrl+C to quit.");
    println!("Self-echo suppression window: {}ms", SELF_ECHO_WINDOW_MS);

    let output_conn = output
        .connect(output_index, APP_NAME)
        .expect("could not connect midi output");
    let output_conn = Arc::new(Mutex::new(output_conn));
    let recently_sent = Arc::new(Mutex::new(VecDeque::<(Instant, Vec<u8>)>::new()));

    let output_conn_cb = Arc::clone(&output_conn);
    let recently_sent_cb = Arc::clone(&recently_sent);

    let _input_conn: MidiInputConnection<()> = input
        .connect(
            input_index,
            APP_NAME,
            move |_stamp, message, _| {
                let bytes = Vec::from(message);
                let now = Instant::now();

                if should_ignore_self_echo(&recently_sent_cb, &bytes, now) {
                    println!("ignored self-echo {:02X?}", bytes);
                    return;
                }

                println!("in  {:02X?}", bytes);
                if let Ok(mut out) = output_conn_cb.lock() {
                    out.send(message).expect("failed to echo midi");
                    println!("out {:02X?}", bytes);
                }
                remember_sent(&recently_sent_cb, bytes, now);
            },
            (),
        )
        .expect("could not connect midi input");

    // Keep process alive.
    let mut buf = [0u8; 1];
    loop {
        let _ = io::stdin().read(&mut buf);
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn should_ignore_self_echo(
    recently_sent: &Arc<Mutex<VecDeque<(Instant, Vec<u8>)>>>,
    message: &[u8],
    now: Instant,
) -> bool {
    let mut queue = recently_sent.lock().unwrap();
    prune_old(&mut queue, now);
    queue.iter().any(|(_, sent)| sent.as_slice() == message)
}

fn remember_sent(
    recently_sent: &Arc<Mutex<VecDeque<(Instant, Vec<u8>)>>>,
    message: Vec<u8>,
    now: Instant,
) {
    let mut queue = recently_sent.lock().unwrap();
    prune_old(&mut queue, now);
    queue.push_back((now, message));
}

fn prune_old(queue: &mut VecDeque<(Instant, Vec<u8>)>, now: Instant) {
    let window = Duration::from_millis(SELF_ECHO_WINDOW_MS);
    while let Some((when, _)) = queue.front() {
        if now.duration_since(*when) > window {
            queue.pop_front();
        } else {
            break;
        }
    }
}

fn get_outputs(output: &MidiOutput) -> Vec<String> {
    let mut result = Vec::new();

    for i in 0..output.port_count() {
        result.push(output.port_name(i).unwrap_or(String::from("")));
    }

    normalize_port_names(&result)
}

fn get_inputs(input: &MidiInput) -> Vec<String> {
    let mut result = Vec::new();

    for i in 0..input.port_count() {
        result.push(input.port_name(i).unwrap_or(String::from("")));
    }

    normalize_port_names(&result)
}

fn normalize_port_names(names: &Vec<String>) -> Vec<String> {
    lazy_static! {
        static ref RE: Regex = Regex::new(r"^([0-9]- )?(.+?)( [0-9]+:([0-9]+))?$").unwrap();
    }

    let mut result = Vec::new();

    for name in names {
        let base_device_name = RE.replace(name, "${2}").into_owned();
        let device_port_index = RE.replace(name, "${4}").parse::<u32>().unwrap_or(0);
        let mut device_index = 0;
        let mut device_name = build_name(&base_device_name, device_index, device_port_index);

        while result.contains(&device_name) {
            device_index += 1;
            device_name = build_name(&base_device_name, device_index, device_port_index);
        }

        result.push(device_name);
    }

    result
}

fn build_name(base: &str, device_id: u32, port_id: u32) -> String {
    let mut result = String::from(base);
    if device_id > 0 {
        result.push_str(&format!(" {}", device_id + 1))
    }
    if port_id > 0 {
        result.push_str(&format!(" PORT {}", port_id + 1))
    }
    result
}
