#[macro_use] extern crate lazy_static;

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
mod audio_recorder;
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
    
    let main_io_name = "pisound";

    // setup default repeat rates
    let mut channel_repeat = HashMap::new();
    channel_repeat.insert(0, ChannelRepeat::Global);
    channel_repeat.insert(1, ChannelRepeat::Global);
    channel_repeat.insert(2, ChannelRepeat::Global);
    channel_repeat.insert(3, ChannelRepeat::None);

    let scale = Scale::new(69, 0);
    let params = Arc::new(Mutex::new(LoopGridParams { 
        swing: 0.0,
        channel_repeat,
        align_offset: MidiTime::zero(),
        reset_automation: false
    }));
    
    let drum_params = Arc::new(Mutex::new(devices::BlofeldDrumParams {
        x: [0, 0, 0, 0],
        y: [0, 0, 0, 0],
        velocities: [110, 110, 110, 110]
    }));
    
    let bass_offset = Offset::new(-2, -4);
    let keys_offset = Offset::new(-1, -4);
    let vox_offset = Offset::new(-2, -4);
    let sp404_offset = Arc::new(AtomicUsize::new(0));

    let main_output_port = midi_connection::get_shared_output(main_io_name);
    let vt3_output_port = midi_connection::get_shared_output("VT-3");
    let blofeld_port = midi_connection::get_shared_output("Blofeld");

    let mut clock = ClockSource::new(main_io_name, vec![
        main_output_port.clone(),
        vt3_output_port.clone(),
        blofeld_port.clone(),
        midi_connection::get_shared_output("Launchpad MK2")
    ]);

    let launchpad = LoopGridLaunchpad::new("Launchpad MK2", vec![
        ChunkMap::new(
            Box::new(devices::SP404Offset::new(Arc::clone(&sp404_offset))),
            Coords::new(8, 0), // top row, page 2
            Shape::new(1, 8),
            71, // grey
            None
        ),

        ChunkMap::new(
            Box::new(devices::OffsetChunk::new(Arc::clone(&bass_offset))),
            Coords::new(1 + 8, 0), 
            Shape::new(1, 8),
            44, // purple
            None
        ),

        ChunkMap::new(
            Box::new(devices::OffsetChunk::new(Arc::clone(&keys_offset))), 
            Coords::new(2 + 8, 0), 
            Shape::new(1, 8),
            60, // red
            None
        ),

        ChunkMap::new(
            Box::new(devices::ScaleSelect::new(Arc::clone(&scale))), 
            Coords::new(3 + 8, 0), 
            Shape::new(1, 8),
            39, // blue
            None
        ),

        ChunkMap::new(
            Box::new(devices::RootSelect::new(Arc::clone(&scale))), 
            Coords::new(4 + 8, 0), 
            Shape::new(2, 8),
            35, // green
            None
        ),

        ChunkMap::new(
            Box::new(devices::BlofeldDrums::new(blofeld_port.clone(), 2, Arc::clone(&drum_params))), 
            Coords::new(0, 0), 
            Shape::new(1, 4),
            15, // yellow
            Some(0)
        ),

        ChunkMap::new(
            Box::new(devices::SP404::new(main_output_port.clone(), 10, Arc::clone(&sp404_offset))), 
            Coords::new(0, 4), 
            Shape::new(1, 4),
            11, // orange
            Some(0)
        ),

        ChunkMap::new(
            Box::new(devices::MidiKeys::new(main_output_port.clone(), 1, Arc::clone(&scale), Arc::clone(&bass_offset))), 
            Coords::new(1, 0), 
            Shape::new(2, 8),
            59, // pink
            Some(1)
        ),

        ChunkMap::new(
            Box::new(devices::MidiKeys::new(blofeld_port.clone(), 1, Arc::clone(&scale), Arc::clone(&keys_offset))), 
            Coords::new(3, 0), 
            Shape::new(3, 8),
            71, // grey
            Some(2)
        ),

        ChunkMap::new(
            Box::new(devices::VT3::new(vt3_output_port.clone(), Arc::clone(&scale), Arc::clone(&vox_offset))), 
            Coords::new(6, 0), 
            Shape::new(2, 8),
            43, // blue
            Some(3)
        )
    ], Arc::clone(&scale), Arc::clone(&params), clock.add_rx());

    let _twister = devices::Twister::new("Midi Fighter Twister", "K-Mix",
        main_output_port.clone(),
        blofeld_port.clone(),
        Arc::clone(&drum_params),
        Arc::clone(&params),
        clock.add_rx(),
        launchpad.meta_tx.clone()
    );

    let _pedal = devices::Umi3::new("Logidy UMI3", launchpad.remote_tx.clone());

    clock.start();
}
