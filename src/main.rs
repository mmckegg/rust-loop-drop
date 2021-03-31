#[macro_use] extern crate lazy_static;
extern crate rand;
extern crate indexmap;
extern crate serde;
extern crate serde_json;

use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use std::process::Command;
use std::process;
use std::time::{Duration, Instant};
use std::path::Path;

mod config;
mod midi_connection;
mod loop_grid_launchpad;
mod loop_recorder;
mod loop_event;
mod loop_state;
mod loop_transform;
mod midi_time;
mod output_value;
mod chunk;
mod devices;
mod scale;
mod lfo;
mod throttled_output;
mod scheduler;

use scale::{Scale, Offset};
use loop_grid_launchpad::{LoopGridLaunchpad, LoopGridParams};
use chunk::{Shape, Coords, ChunkMap, RepeatMode, Triggerable};
use std::sync::atomic::AtomicUsize;
use ::midi_time::{MidiTime, SUB_TICKS};
use scheduler::{Scheduler, ScheduleRange};
use std::sync::mpsc;


const APP_NAME: &str = "Loop Drop";
const CONFIG_FILEPATH: &str = "./loopdrop-config.json";

fn main() {
    
    let mut chunks = Vec::new();
    let mut myconfig = config::Config::default();

    if Path::new(CONFIG_FILEPATH).exists() {
        myconfig = config::Config::read(CONFIG_FILEPATH).unwrap();
        println!("Read config from {}", CONFIG_FILEPATH);
    } else {
        myconfig.write(CONFIG_FILEPATH).unwrap();
        println!("Wrote config to {}", CONFIG_FILEPATH);
    }

    // Command::new("renice").args(&["-n", "-20", &format!("{}", process::id())]).output();
    let output = midi_connection::MidiOutput::new(APP_NAME).unwrap();
    let input = midi_connection::MidiInput::new(APP_NAME).unwrap();

    println!("Midi Outputs: {:?}", midi_connection::get_outputs(&output));
    println!("Midi Inputs: {:?}", midi_connection::get_inputs(&input));

    // NAME OF CLOCK INPUT MIDI PORT *****
    let clock_input_name = &myconfig.clock_input_port_name;
    // ***********************************

    let scale = Scale::new(60, 0);

    let params = Arc::new(Mutex::new(LoopGridParams { 
        swing: 0.0,
        bank: 0,
        frozen: false,
        align_offset: MidiTime::zero(),
        reset_automation: false
    }));
    
    let drum_velocities = Arc::new(Mutex::new(HashMap::new()));

    { // release lock
        let mut v = drum_velocities.lock().unwrap();
        v.insert(0, 127);
        v.insert(4, 127);
    }

    let launchpad_io_name = if cfg!(target_os = "linux") {
        "Launchpad Pro MK3"
    } else {
        "Launchpad Pro MK3 LPProMK3 MIDI"
    };

    let mut output_ports = HashMap::new();
    let mut offset_lookup = HashMap::new();

    for chunk in myconfig.chunks {

        let device: Box<Triggerable + Send> = match chunk.device{
            config::DeviceConfig::MidiKeys{outputs, offset_id, note_offset, octave_offset} => {
                let device_ports = outputs.iter().map(|port| (get_port(&mut output_ports, &port.name), port.channel)).collect();
                let offset = get_offset(&mut offset_lookup, &offset_id);
                set_offset(offset.clone(), &note_offset, &octave_offset);

                Box::new(devices::MidiKeys::new(device_ports, scale.clone(), offset))
            },
            config::DeviceConfig::OffsetChunk{id} => {
                Box::new(devices::OffsetChunk::new(get_offset(&mut offset_lookup, &id)))
            },
            config::DeviceConfig::RootSelect => {
                Box::new(devices::RootSelect::new(scale.clone()))
            },
            config::DeviceConfig::BlackboxSample{output} => {
                let device_port = get_port(&mut output_ports, &output.name);

                Box::new(devices::BlackboxSample::new(device_port, output.channel))
            }
        };

        chunks.push(
            ChunkMap::new(
                device,
                chunk.coords, 
                chunk.shape,
                chunk.color,
                chunk.channel,
                chunk.repeat_mode
            )
        )
    }

    let mut launchpad = LoopGridLaunchpad::new(launchpad_io_name, chunks, Arc::clone(&scale), Arc::clone(&params));
    let mut _twister = None;

    if let Some(port) = myconfig.twister_main_output_port {
        _twister = Some(devices::Twister::new("Midi Fighter Twister",
            get_port(&mut output_ports, &port.name),
            port.channel,
            Arc::clone(&params)
        ));
    } 

    let _pedal = devices::Umi3::new("Logidy UMI3", launchpad.remote_tx.clone());

    // let mut vt4 = devices::VT4Key::new(vt4_output_port.clone(), 8, scale.clone());
    
    // let mut clock_blackbox_output_port = rk006_out_1_port.clone();
    // let mut tr6s_clock_output_port = tr6s_output_port.clone();
    // let mut nts1_clock_output_port = nts1_output_port.clone();


    for range in Scheduler::start(clock_input_name) {
        // sending clock is the highest priority, so lets do these first
        if range.ticked {
            if range.tick_pos % MidiTime::from_beats(32) == MidiTime::zero() {
                // clock_blackbox_output_port.send(&[250]).unwrap();
            }
        }
        // nts1_clock_output_port.send(&[248]).unwrap();
        
        if range.ticked && range.from.ticks() != range.to.ticks() {
            // HACK: straighten out missing sub ticks into separate schedules
            let mut a = range.clone();
            a.to = MidiTime::new(a.to.ticks(), 0);
            a.tick_pos = MidiTime::new(a.from.ticks(), 0);
            a.ticked = false;
            let mut b = range.clone();
            b.from = MidiTime::new(b.to.ticks(), 0);
            launchpad.schedule(a);
            launchpad.schedule(b);
        } else {
            launchpad.schedule(range);
        }
        
        // now for the lower priority stuff
        if range.ticked {
            let length = MidiTime::tick();
            // twister.schedule(range.tick_pos, length);
            // vt4.schedule(range.tick_pos, length);

            // keep the tr6s midi input active (otherwise it stops responding to incoming midi immediately)
            // tr6s_clock_output_port.send(&[254]).unwrap();
        }
    }
}



// Helper functions

fn get_port(ports_lookup: &mut HashMap<String, midi_connection::SharedMidiOutputConnection>, port_name: &str) -> midi_connection::SharedMidiOutputConnection {
    if !ports_lookup.contains_key(port_name) {
        ports_lookup.insert(String::from(port_name), midi_connection::get_shared_output(port_name));
    }

    ports_lookup.get(port_name).unwrap().clone()
}

fn get_offset(offset_lookup: &mut HashMap<String, Arc<Mutex<Offset>> >, id: &str) -> Arc<Mutex<Offset>> {
    if !offset_lookup.contains_key(id) {
        offset_lookup.insert(String::from(id), Offset::new(0, 0));
    }

    offset_lookup.get(id).unwrap().clone()
}

fn set_offset(offset: Arc<Mutex<Offset>>, note_offset: &i32, octave_offset: &i32) {
    let mut value = offset.lock().unwrap();

    value.oct = *octave_offset;
    value.base = *note_offset;
}