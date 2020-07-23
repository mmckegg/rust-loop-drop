#[macro_use] extern crate lazy_static;
extern crate rand;
use rand::Rng;

use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use std::process::Command;
use std::process;
use std::time::{Duration, Instant};

mod midi_connection;
mod loop_grid_launchpad;
mod loop_recorder;
mod loop_event;
mod loop_state;
mod loop_transform;
mod midi_time;
mod clock_source;
mod output_value;
mod chunk;
mod devices;
mod scale;
mod lfo;
mod throttled_output;
mod scheduler;

use scale::{Scale, Offset};
use clock_source::ClockSource;
use loop_grid_launchpad::{LoopGridLaunchpad, LoopGridParams};
use chunk::{Shape, Coords, ChunkMap, RepeatMode};
use std::sync::atomic::AtomicUsize;
use ::midi_time::MidiTime;
use scheduler::Scheduler;

const APP_NAME: &str = "Loop Drop";

fn main() {

    Command::new("renice").args(&["-n", "-20", &format!("{}", process::id())]).output();
    let output = midi_connection::MidiOutput::new(APP_NAME).unwrap();
    let input = midi_connection::MidiInput::new(APP_NAME).unwrap();

    println!("Midi Outputs: {:?}", midi_connection::get_outputs(&output));
    println!("Midi Inputs: {:?}", midi_connection::get_inputs(&input));
    
    let all_io_name = "RK006"; // All out
    let blackbox_io_name = "RK006 PORT 2"; // 1
    let zoia_io_name = "RK006 PORT 3"; // 2
    let dd_io_name = "RK006 PORT 4"; // 3
    let sh01a_io_name = "Boutique";
    let vt4_io_name = "VT-4";
    let keyboard_io_name = "K-Board";
    let ju06_io_name = "JU-06A";
    let geode_io_name = "USB MIDI";

    let launchpad_io_name = if cfg!(target_os = "linux") {
        "Launchpad Pro MK3"
    } else {
        "Launchpad Pro MK3 LPProMK3 MIDI"
    };

    let scale = Scale::new(60, 0);

    let params = Arc::new(Mutex::new(LoopGridParams { 
        swing: 0.0,
        bank: 0,
        frozen: false,
        align_offset: MidiTime::zero(),
        reset_automation: false
    }));
    
    let drum_velocities = Arc::new(Mutex::new(HashMap::new()));
    let slicer_mode = devices::BlackboxSlicerModeChooser::default_value();
    let slicer_bank = devices::BlackboxSlicerBankChooser::default_value();

    { // release lock
        let mut v = drum_velocities.lock().unwrap();
        v.insert(0, 127);
        v.insert(4, 127);
    }

    let bass_offset = Offset::new(-2, -4);
    let geode_offset = Offset::new(0, -4);
    let keys_offset = Offset::new(-1, -4);

    let dd_output_port = midi_connection::get_shared_output(dd_io_name);
    let sh01a_output_port = midi_connection::get_shared_output(sh01a_io_name);
    let blackbox_output_port = midi_connection::get_shared_output(blackbox_io_name);
    let zoia_output_port = midi_connection::get_shared_output(zoia_io_name);
    let all_output_port = midi_connection::get_shared_output(all_io_name);
    
    let vt4_output_port = midi_connection::get_shared_output(vt4_io_name);
    let ju06a_output_port = midi_connection::get_shared_output(ju06_io_name);
    let geode_output_port = midi_connection::get_shared_output(geode_io_name);


    // let mut clock = ClockSource::new(all_io_name, vec![
    //     all_output_port.clone(),
    //     // zoia_output_port.clone(),
    //     vt4_output_port.clone(),
    //     midi_connection::get_shared_output(launchpad_io_name)
    // ]);

    // auto send clock start every 32 beats (for arp sync)
    // clock.sync_clock_start(blofeld_output_port.clone());

    let mut launchpad = LoopGridLaunchpad::new(launchpad_io_name, vec![

        ChunkMap::new(
            Box::new(devices::MidiKeys::new(vec![geode_output_port.clone(), blackbox_output_port.clone()], 1, Arc::clone(&scale), Arc::clone(&geode_offset))), 
            Coords::new(0 + 8, 0),
            Shape::new(3, 8),
            125, // gross
            Some(2),
            RepeatMode::Global
        ),

        ChunkMap::new(
            Box::new(devices::OffsetChunk::new(Arc::clone(&geode_offset))),
            Coords::new(3 + 8, 0), 
            Shape::new(1, 8),
            12, // soft yellow
            None,
            RepeatMode::None
        ),

        ChunkMap::new(
            Box::new(devices::OffsetChunk::new(Arc::clone(&bass_offset))),
            Coords::new(4 + 8, 0), 
            Shape::new(1, 8),
            55, // pink
            None,
            RepeatMode::None
        ),

        ChunkMap::new(
            Box::new(devices::OffsetChunk::new(Arc::clone(&keys_offset))), 
            Coords::new(5 + 8, 0), 
            Shape::new(1, 8),
            43, // blue
            None,
            RepeatMode::None
        ),
        
        ChunkMap::new(
            Box::new(devices::RootOffsetChunk::new(Arc::clone(&scale))), 
            Coords::new(6 + 8, 0), 
            Shape::new(1, 8),
            35, // green
            None,
            RepeatMode::None
        ),

        ChunkMap::new(
            Box::new(devices::ScaleOffsetChunk::new(Arc::clone(&scale))), 
            Coords::new(7 + 8, 0), 
            Shape::new(1, 8),
            32, // blue
            None,
            RepeatMode::None
        ),

        ChunkMap::new(
            Box::new(devices::DoubleDrummer::new(dd_output_port.clone(), 1, zoia_output_port.clone(), 16, Arc::clone(&drum_velocities))), 
            Coords::new(0, 0), 
            Shape::new(1, 8),
            15, // yellow
            Some(0),
            RepeatMode::Global
        ),

        ChunkMap::new(
            Box::new(devices::BlackboxSample::new(blackbox_output_port.clone(), 10)), 
            Coords::new(1, 0), 
            Shape::new(1, 8),
            9, // orange
            Some(1),
            RepeatMode::Global
        ),

        ChunkMap::new(
            Box::new(devices::MidiKeys::new(vec![sh01a_output_port.clone(), blackbox_output_port.clone()], 11, Arc::clone(&scale), Arc::clone(&bass_offset))), 
            Coords::new(2, 0), 
            Shape::new(3, 8),
            59, // pink
            Some(2),
            RepeatMode::Global
        ),

        ChunkMap::new(
            Box::new(devices::MidiKeys::new(vec![ju06a_output_port.clone()], 1, Arc::clone(&scale), Arc::clone(&keys_offset))), 
            Coords::new(5, 0), 
            Shape::new(3, 8),
            43, // blue
            Some(3),
            RepeatMode::Global
        )
    ], Arc::clone(&scale), Arc::clone(&params), blackbox_output_port.clone(), 10, 36);

    let _keyboard = devices::KBoard::new(keyboard_io_name, ju06a_output_port.clone(), 1, scale.clone());

    let twister = devices::Twister::new("Midi Fighter Twister",
        sh01a_output_port.clone(),
        ju06a_output_port.clone(),
        blackbox_output_port.clone(),
        zoia_output_port.clone(),
        Arc::clone(&params)
    );

    let _pedal = devices::Umi3::new("Logidy UMI3", launchpad.remote_tx.clone());

    let mut vt4 = devices::VT4Key::new(vt4_output_port.clone(), 8, scale.clone());
    
    let mut ju06a_output_port_clock = ju06a_output_port.clone();
    let mut sh01a_output_port_clock = sh01a_output_port.clone();

    let _clock = midi_connection::get_input(all_io_name, move |_stamp, msg| {
        if msg[0] == 248 {
            ju06a_output_port_clock.send(msg).unwrap();
            sh01a_output_port_clock.send(msg).unwrap();
        }
    });

    for range in Scheduler::start(all_io_name) {
        launchpad.schedule(range);

        if range.ticked {
            let pos = MidiTime::from_ticks(range.from.ticks());
            let length = MidiTime::tick();
            twister.schedule(pos, length);
            vt4.schedule(pos, length)
        }
    }
}

