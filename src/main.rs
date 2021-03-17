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
use serde_json::json;


const APP_NAME: &str = "Loop Drop";

fn main() {
    
    let mut chunks = Vec::new();

    let myconfig = config::Config{
        chunks: vec![
            config::ChunkConfig {
                shape: Shape::new(1, 8),
                coords: Coords::new(0, 0),
                color: 22,
                channel: Some(1),
                repeat_mode: RepeatMode::Global,
                device: config::DeviceConfig::MidiKeys{
                    outputs: vec![config::MidiPortConfig{
                        name: String::from("RK006 PORT 2"),
                        channel: 2
                    }],
                    offset_id: String::from("SomeNameToUseItSomewhere"),
                    note_offset: 0,
                    octave_offset: -1
                }
            },
            config::ChunkConfig {
                shape: Shape::new(1, 8),
                coords: Coords::new(1, 0),
                color: 44,
                channel: Some(2),
                repeat_mode: RepeatMode::Global,
                device: config::DeviceConfig::BlackboxSample {
                    output: config::MidiPortConfig{
                        name: String::from("RK006 PORT 3"),
                        channel: 1
                    }
                }
            },
            config::ChunkConfig {
                shape: Shape::new(1, 8),
                coords: Coords::new(2, 0),
                color: 66,
                channel: Some(3),
                repeat_mode: RepeatMode::Global,
                device: config::DeviceConfig::OffsetChunk {
                    id: String::from("SomeNameToUseItSomewhere")
                }
            },
            config::ChunkConfig {
                shape: Shape::new(1, 8),
                coords: Coords::new(3, 0),
                color: 88,
                channel: Some(4),
                repeat_mode: RepeatMode::Global,
                device: config::DeviceConfig::RootSelect
            }
        ],
        clock_input_port_name: String::from("TR-6S"),
        clock_output_port_names: vec![String::from("clock_output_port_names")],
        resync_port_names: vec![String::from("resync_port_names")],
    };

    let myjson = json!(myconfig);

    println!("{}", myjson.to_string());


    Command::new("renice").args(&["-n", "-20", &format!("{}", process::id())]).output();
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

    // let bass_offset = Offset::new(-2, -4);
    // let keys_offset = Offset::new(0, -4);
    // let ext_offset = Offset::new(-1, -4);

    // let tr6s_output_port = midi_connection::get_shared_output("TR-6S");
    
    // let rk006_out_1_port = midi_connection::get_shared_output("RK006 PORT 2");
    // let rk006_out_2_port = midi_connection::get_shared_output("RK006 PORT 3");
    // let rk006_out_3_port = midi_connection::get_shared_output("RK006 PORT 4");
    // let rk006_out_4_port = midi_connection::get_shared_output("RK006 PORT 5");
    // let rk006_out_5_port = midi_connection::get_shared_output("RK006 PORT 6");

    // // PWM CV voltages
    // let cv1_port = midi_connection::get_shared_output("RK006 PORT 7");
    // let cv2_port = midi_connection::get_shared_output("RK006 PORT 9");
    
    // let vt4_output_port = midi_connection::get_shared_output("VT-4");
    // let streichfett_output_port = midi_connection::get_shared_output("Streichfett");
    // let nts1_output_port = midi_connection::get_shared_output("NTS-1 digital kit");

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

    // let mut launchpad = LoopGridLaunchpad::new(launchpad_io_name, vec![

    //     // EXT SYNTH
    //     // Send this to Geode, Blackbox (channel 1), Blackbox (channel 2 but as slicer rather than pitch), and RK-006 port 4 (TRS)
    //     ChunkMap::new(
    //         Box::new(devices::MultiChunk::new(vec![
    //             Box::new(devices::MidiKeys::new(vec![rk006_out_1_port.clone(), rk006_out_5_port.clone(), nts1_output_port.clone()], 1, Arc::clone(&scale), Arc::clone(&ext_offset))), 
    //             Box::new(devices::BlackboxSlicer::new(rk006_out_1_port.clone(), 2))
    //         ])),
    //         Coords::new(0 + 8, 0),
    //         Shape::new(3, 8),
    //         125, // gross
    //         Some(2),
    //         RepeatMode::Global
    //     ),

    //     // EXT SYNTH OFFSET 
    //     // (also sends pitch mod on channel 2 for slicer)
    //     ChunkMap::new(
    //         Box::new(devices::MultiChunk::new(vec![
    //             Box::new(devices::OffsetChunk::new(Arc::clone(&ext_offset))),
    //             Box::new(devices::PitchOffsetChunk::new(rk006_out_1_port.clone(), 2))
    //         ])),
    //         Coords::new(3 + 8, 0), 
    //         Shape::new(1, 8),
    //         12, // soft yellow
    //         None,
    //         RepeatMode::OnlyQuant
    //     ),

    //     // BASS OFFSET
    //     ChunkMap::new(
    //         Box::new(devices::OffsetChunk::new(Arc::clone(&bass_offset))),
    //         Coords::new(4 + 8, 0), 
    //         Shape::new(1, 8),
    //         43, // blue
    //         None,
    //         RepeatMode::OnlyQuant
    //     ),

    //     // SYNTH OFFSET
    //     ChunkMap::new(
    //         Box::new(devices::OffsetChunk::new(Arc::clone(&keys_offset))), 
    //         Coords::new(5 + 8, 0), 
    //         Shape::new(1, 8),
    //         55, // pink
    //         None,
    //         RepeatMode::OnlyQuant
    //     ),
        
    //     // ROOT NOTE SELECTOR
    //     ChunkMap::new(
    //         Box::new(devices::RootSelect::new(Arc::clone(&scale))), 
    //         Coords::new(6 + 8, 0), 
    //         Shape::new(2, 8),
    //         35, // soft green
    //         None,
    //         RepeatMode::OnlyQuant
    //     ),

    //     // SCALE MODE SELECTOR
    //     ChunkMap::new(
    //         Box::new(devices::ScaleSelect::new(Arc::clone(&scale))), 
    //         Coords::new(16, 0), 
    //         Shape::new(1, 8),
    //         0, // black
    //         None,
    //         RepeatMode::OnlyQuant
    //     ),

    //     // DRUMS
    //     ChunkMap::new(
    //         Box::new(devices::TR6s::new(tr6s_output_port.clone(), 10, rk006_out_2_port.clone(), 16, Arc::clone(&drum_velocities))), 
    //         Coords::new(0, 0), 
    //         Shape::new(1, 6),
    //         15, // yellow
    //         Some(0),
    //         RepeatMode::NoCycle
    //     ),

    //     // EXTRA PERC (to fill in for only 6 triggers on cycles)
    //     ChunkMap::new(
    //         Box::new(devices::BlackboxPerc::new(rk006_out_1_port.clone(), 10, Arc::clone(&drum_velocities))), 
    //         Coords::new(0, 6), 
    //         Shape::new(1, 2),
    //         9, // orange
    //         Some(1),
    //         RepeatMode::NoCycle
    //     ),

    //     // SAMPLER
    //     ChunkMap::new(
    //         Box::new(devices::BlackboxSample::new(rk006_out_1_port.clone(), 10)), 
    //         Coords::new(1, 0), 
    //         Shape::new(1, 8),
    //         9, // orange
    //         Some(1),
    //         RepeatMode::OnlyQuant
    //     ),

    //     // BASS
    //     ChunkMap::new(
    //         Box::new(devices::MidiKeys::new(vec![rk006_out_3_port.clone(), rk006_out_1_port.clone()], 11, Arc::clone(&scale), Arc::clone(&bass_offset))), 
    //         Coords::new(2, 0), 
    //         Shape::new(3, 8),
    //         43, // blue
    //         Some(2),
    //         RepeatMode::Global
    //     ),

    //     // SYNTH
    //     ChunkMap::new(
    //         Box::new(devices::MidiKeys::new(vec![streichfett_output_port.clone(), rk006_out_4_port.clone()], 1, Arc::clone(&scale), Arc::clone(&keys_offset))), 
    //         Coords::new(5, 0), 
    //         Shape::new(3, 8),
    //         59, // pink
    //         Some(3),
    //         RepeatMode::Global
    //     )
    // ], Arc::clone(&scale), Arc::clone(&params), rk006_out_1_port.clone(), 10, 36);

    // let _keyboard = devices::KBoard::new("K-Board", streichfett_output_port.clone(), 1, scale.clone());

    // let twister = devices::Twister::new("Midi Fighter Twister",
    //     rk006_out_3_port.clone(),
    //     streichfett_output_port.clone(),
    //     tr6s_output_port.clone(),
    //     rk006_out_1_port.clone(),
    //     nts1_output_port.clone(),
    //     rk006_out_2_port.clone(),
    //     cv1_port.clone(),
    //     cv2_port.clone(),
    //     Arc::clone(&params)
    // );

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