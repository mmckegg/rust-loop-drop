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
use loop_grid_launchpad::{LoopGridLaunchpad, LoopGridParams, ChannelRepeat};
use chunk::{Shape, Coords, ChunkMap};
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
    
    let all_io_name = "RK006";
    let pulse_io_name = "RK006 PORT 4";
    let blofeld_io_name = "RK006 PORT 5";
    let blackbox_io_name = "RK006 PORT 2";
    let zoia_io_name = "RK006 PORT 3";
    let vt4_io_name = "VT-4";
    let keyboard_io_name = "K-Board";

    let launchpad_io_name = if cfg!(target_os = "linux") {
        "Launchpad Pro"
    } else {
        "Launchpad Pro Live Port"
    };

    // setup default repeat rates
    let mut channel_repeat = HashMap::new();
    channel_repeat.insert(0, ChannelRepeat::Global);
    channel_repeat.insert(1, ChannelRepeat::Global);
    channel_repeat.insert(2, ChannelRepeat::Global);
    channel_repeat.insert(3, ChannelRepeat::Global);

    let scale = Scale::new(64, 5);

    let params = Arc::new(Mutex::new(LoopGridParams { 
        swing: 0.0,
        bank: 0,
        frozen: false,
        channel_repeat,
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
    let vox_offset = Offset::new(-1, -4);
    let keys_offset = Offset::new(-1, -4);

    let pulse_output_port = midi_connection::get_shared_output(pulse_io_name);
    let blofeld_output_port = midi_connection::get_shared_output(blofeld_io_name);
    let blackbox_output_port = midi_connection::get_shared_output(blackbox_io_name);
    let zoia_output_port = midi_connection::get_shared_output(zoia_io_name);
    let vt4_output_port = midi_connection::get_shared_output(vt4_io_name);
    let all_output_port = midi_connection::get_shared_output(all_io_name);


    // let mut clock = ClockSource::new(all_io_name, vec![
    //     all_output_port.clone(),
    //     // zoia_output_port.clone(),
    //     vt4_output_port.clone(),
    //     midi_connection::get_shared_output(launchpad_io_name)
    // ]);

    // auto send clock start every 32 beats (for arp sync)
    // clock.sync_clock_start(blofeld_output_port.clone());

    let launchpad = LoopGridLaunchpad::new(launchpad_io_name, vec![
        ChunkMap::new(
            Box::new(devices::VelocityMap::new(Arc::clone(&drum_velocities))),
            Coords::new(0 + 8, 0),
            Shape::new(1, 8),
            12, // light yellow
            None
        ),

        // ChunkMap::new(
        //     Box::new(devices::VelocityMap::new(Arc::clone(&sp404_velocities))),
        //     Coords::new(1 + 8, 0),
        //     Shape::new(1, 8),
        //     126, // light orange
        //     None
        // ),

        ChunkMap::new(
            Box::new(devices::BlackboxSlicerModeChooser::new(Arc::clone(&slicer_mode))),
            Coords::new(1 + 8, 0),
            Shape::new(1, 3),
            126, // light orange
            None
        ),

        ChunkMap::new(
            Box::new(devices::BlackboxSlicerBankChooser::new(Arc::clone(&slicer_bank))),
            Coords::new(1 + 8, 3),
            Shape::new(1, 5),
            71, // dark grey
            None
        ),

        ChunkMap::new(
            Box::new(devices::MidiKeys::new(vec![vt4_output_port.clone(), blackbox_output_port.clone()], 8, Arc::clone(&scale), Arc::clone(&vox_offset))), 
            Coords::new(2 + 8, 0),
            Shape::new(1, 8),
            125, // gross
            Some(2)
        ),

        ChunkMap::new(
            Box::new(devices::OffsetChunk::new(Arc::clone(&bass_offset))),
            Coords::new(3 + 8, 0), 
            Shape::new(1, 8),
            55, // pink
            None
        ),

        ChunkMap::new(
            Box::new(devices::OffsetChunk::new(Arc::clone(&keys_offset))), 
            Coords::new(4 + 8, 0), 
            Shape::new(1, 8),
            43, // blue
            None
        ),

        ChunkMap::new(
            Box::new(devices::ScaleSelect::new(Arc::clone(&scale))), 
            Coords::new(5 + 8, 0), 
            Shape::new(1, 8),
            32, // blue
            None
        ),

        ChunkMap::new(
            Box::new(devices::RootSelect::new(Arc::clone(&scale))), 
            Coords::new(6 + 8, 0), 
            Shape::new(2, 8),
            35, // green
            None
        ),

        ChunkMap::new(
            Box::new(devices::BlackboxDrums::new(blackbox_output_port.clone(), 10, zoia_output_port.clone(), 16, Arc::clone(&drum_velocities))), 
            Coords::new(0, 0), 
            Shape::new(1, 8),
            15, // yellow
            Some(0)
        ),

        ChunkMap::new(
            Box::new(devices::BlackboxSlicer::new(blackbox_output_port.clone(), Arc::clone(&slicer_mode), Arc::clone(&slicer_bank))), 
            Coords::new(1, 0), 
            Shape::new(1, 8),
            9, // orange
            Some(1)
        ),

        ChunkMap::new(
            Box::new(devices::MidiKeys::new(vec![pulse_output_port.clone(), blackbox_output_port.clone()], 11, Arc::clone(&scale), Arc::clone(&bass_offset))), 
            Coords::new(2, 0), 
            Shape::new(3, 8),
            59, // pink
            Some(2)
        ),

        ChunkMap::new(
            Box::new(devices::MidiKeys::new(vec![blofeld_output_port.clone()], 12, Arc::clone(&scale), Arc::clone(&keys_offset))), 
            Coords::new(5, 0), 
            Shape::new(3, 8),
            43, // blue
            Some(3)
        )
    ], Arc::clone(&scale), Arc::clone(&params), blackbox_output_port.clone(), 10, 36);

    let _keyboard = devices::KBoard::new(keyboard_io_name, blofeld_output_port.clone(), 13, scale.clone());

    let twister = devices::Twister::new("Midi Fighter Twister",
        pulse_output_port.clone(),
        blofeld_output_port.clone(),
        blackbox_output_port.clone(),
        zoia_output_port.clone(),
        Arc::clone(&params)
    );

    let _pedal = devices::Umi3::new("Logidy UMI3", launchpad.remote_tx.clone());

    let mut vt4 = devices::VT4Key::new(vt4_output_port.clone(), 8, scale.clone());
    
    for schedule in Scheduler::start(all_io_name, Duration::from_micros(500)) {
        if schedule.from.ticks() < schedule.to.ticks() || schedule.from.frac() == 0 {
            let pos = MidiTime::from_ticks(schedule.to.ticks());
            let length = MidiTime::tick();
            launchpad.schedule(pos, length);
            twister.schedule(pos, length);
            vt4.schedule(pos, length);
        }
    }
}

