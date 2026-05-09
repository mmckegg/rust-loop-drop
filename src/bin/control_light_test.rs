#[macro_use]
extern crate lazy_static;
extern crate midir;
extern crate regex;

use midir::{MidiOutput, MidiOutputConnection};
use regex::Regex;
use std::thread;
use std::time::Duration;

const APP_NAME: &str = "Loop Drop Control Light Test";
const DEFAULT_PORT_MATCH: &str = "LOOP DROP ";
const CONTROL_NOTES: [u8; 8] = [1, 2, 3, 4, 5, 6, 7, 8];
const CONTROL_CHANNEL: u8 = 1;
const BANK_NOTES: [u8; 4] = [5, 6, 7, 8];
const BANK_CHANNEL: u8 = 1;

fn main() {
    let port_match = std::env::args()
        .nth(1)
        .unwrap_or_else(|| String::from(DEFAULT_PORT_MATCH));

    let output = MidiOutput::new(APP_NAME).expect("could not create midi output");
    let port_names = get_outputs(&output);

    let port_index = port_names
        .iter()
        .position(|name| name.contains(&port_match))
        .unwrap_or_else(|| {
            eprintln!("Could not find MIDI output matching {:?}", port_match);
            for (i, name) in port_names.iter().enumerate() {
                eprintln!("  {}: {}", i, name);
            }
            std::process::exit(1);
        });

    let port_name = output.port_name(port_index).unwrap_or_default();
    println!("Connecting to MIDI output: {}", port_name);
    let mut conn = output
        .connect(port_index, APP_NAME)
        .expect("could not connect midi output");

    all_off(&mut conn);
    sleep_ms(250);

    println!("Phase 1: original control buttons note test on channel {}", CONTROL_CHANNEL);
    let hues = [3, 13, 17, 43, 72, 94, 96, 120];
    for (note, hue) in CONTROL_NOTES.iter().zip(hues.iter()) {
        send_note(&mut conn, CONTROL_CHANNEL, *note, *hue);
    }
    sleep_ms(2000);

    println!("Phase 2: pulse through original control notes on channel {}", CONTROL_CHANNEL);
    for note in &CONTROL_NOTES {
        for other in &CONTROL_NOTES {
            send_note(&mut conn, CONTROL_CHANNEL, *other, 0);
        }
        send_note(&mut conn, CONTROL_CHANNEL, *note, 96);
        sleep_ms(300);
    }
    sleep_ms(500);

    println!("Phase 3: original bank buttons note test on channel {}", BANK_CHANNEL);
    let bank_hues = [13, 17, 94, 3];
    for (note, hue) in BANK_NOTES.iter().zip(bank_hues.iter()) {
        send_note(&mut conn, BANK_CHANNEL, *note, *hue);
    }
    sleep_ms(2000);

    println!("Phase 4: all off");
    all_off(&mut conn);
}

fn send_note(conn: &mut MidiOutputConnection, channel: u8, note: u8, value: u8) {
    let status = 0x90 + (channel - 1);
    let msg = [status, note, value];
    println!("send note ch={} note={} value={} bytes={:02X?}", channel, note, value, msg);
    conn.send(&msg).expect("failed to send midi");
}

fn all_off(conn: &mut MidiOutputConnection) {
    for note in &CONTROL_NOTES {
        send_note(conn, CONTROL_CHANNEL, *note, 0);
    }
    for note in &BANK_NOTES {
        send_note(conn, BANK_CHANNEL, *note, 0);
    }
}

fn sleep_ms(ms: u64) {
    thread::sleep(Duration::from_millis(ms));
}

fn get_outputs(output: &MidiOutput) -> Vec<String> {
    let mut result = Vec::new();

    for i in 0..output.port_count() {
        result.push(output.port_name(i).unwrap_or(String::from("")));
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
