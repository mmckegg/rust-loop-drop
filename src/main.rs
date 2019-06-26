#[macro_use] extern crate lazy_static;
extern crate rand;
use rand::Rng;

use std::sync::{Arc, Mutex};
use std::collections::HashMap;

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

use scale::{Scale, Offset};
use clock_source::ClockSource;
use loop_grid_launchpad::{LoopGridLaunchpad, LoopGridParams, ChannelRepeat};
use chunk::{Shape, Coords, ChunkMap};
use std::sync::atomic::AtomicUsize;
use ::midi_time::MidiTime;

fn main() {
    println!("Midi Outputs: {:?}", midi_connection::get_outputs());
    println!("Midi Inputs: {:?}", midi_connection::get_inputs());
    
    let output_name = "IAC Driver LD Output";

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

    let scale = Scale::new(rand::thread_rng().gen_range(64, 75), rand::thread_rng().gen_range(0, 6));

    let params = Arc::new(Mutex::new(LoopGridParams { 
        swing: 0.0,
        frozen: false,
        channel_repeat,
        align_offset: MidiTime::zero(),
        reset_automation: false
    }));
    
    let drum_velocities = Arc::new(Mutex::new(HashMap::new()));
    let sp404_velocities = Arc::new(Mutex::new(HashMap::new()));

    { // release lock
        let mut v = drum_velocities.lock().unwrap();
        v.insert(0, 127);
        v.insert(4, 127);
    }

    let bass_offset = Offset::new(-2, -4);
    let keys_offset = Offset::new(-1, -4);
    let sp404_offset = Arc::new(AtomicUsize::new(0));

    let main_output_port = midi_connection::get_shared_output(output_name);

    let mut clock = ClockSource::new("IAC Driver LD Cue", vec![
        main_output_port.clone(),
        midi_connection::get_shared_output(launchpad_io_name)
    ]);

    // auto send clock start every 32 beats (for arp sync) [breaks volca sample]
    // clock.sync_clock_start(main_output_port.clone());

    let launchpad = LoopGridLaunchpad::new(launchpad_io_name, vec![
        ChunkMap::new(
            Box::new(devices::VelocityMap::new(Arc::clone(&drum_velocities))),
            Coords::new(0 + 8, 0),
            Shape::new(1, 8),
            12, // light yellow
            None
        ),

        ChunkMap::new(
            Box::new(devices::SP404Offset::new(Arc::clone(&sp404_offset))),
            Coords::new(1 + 8, 0),
            Shape::new(2, 8),
            71, // dark grey
            None
        ),

        ChunkMap::new(
            Box::new(devices::BlofeldDrums::new(main_output_port.clone(), 1, main_output_port.clone(), 1, Arc::clone(&drum_velocities))), 
            Coords::new(0, 0), 
            Shape::new(1, 8),
            15, // yellow
            Some(0)
        ),

        ChunkMap::new(
            Box::new(devices::SP404::new(main_output_port.clone(), 4, 4, Arc::clone(&sp404_offset), Arc::clone(&sp404_velocities))), 
            Coords::new(1, 0), 
            Shape::new(1, 8),
            11, // orange
            Some(1)
        ),

        ChunkMap::new(
            Box::new(devices::MidiKeys::new(main_output_port.clone(), 2, Arc::clone(&scale), Arc::clone(&bass_offset))), 
            Coords::new(2, 0), 
            Shape::new(3, 8),
            59, // pink
            Some(2)
        ),

        ChunkMap::new(
            Box::new(devices::MidiKeys::new(main_output_port.clone(), 3, Arc::clone(&scale), Arc::clone(&keys_offset))), 
            Coords::new(5, 0), 
            Shape::new(3, 8),
            43, // blue
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
        )
    ], Arc::clone(&scale), Arc::clone(&params), clock.add_rx());

    let _twister = devices::Twister::new("Midi Fighter Twister",
        main_output_port.clone(),
        Arc::clone(&params),
        clock.add_rx()
    );

    let _pedal = devices::Umi3::new("Logidy UMI3", launchpad.remote_tx.clone());

    clock.start();
}
