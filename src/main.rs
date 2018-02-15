#[macro_use] extern crate lazy_static;
use std::sync::Arc;

mod midi_connection;
mod loop_grid_launchpad;
mod loop_recorder;
mod loop_state;
mod midi_time;
mod midi_keys;
mod clock_source;
mod output_value;
mod chunk;
mod devices;
mod scale;

use scale::{Scale, Offset};
use clock_source::ClockSource;
use loop_grid_launchpad::LoopGridLaunchpad;
use chunk::{Shape, Coords, ChunkMap};

fn main() {
    println!("Midi Outputs: {:?}", midi_connection::get_outputs());
    println!("Midi Inputs: {:?}", midi_connection::get_inputs());

    let clock_port_name = "UM-ONE";
    let output_port_name = "UM-ONE";

    let scale = Scale::new(69, 0);
    let bass_offset = Offset::new(-3);
    let mother_offset = Offset::new(-4);
    let keys_offset = Offset::new(-1);

    let mut clock = ClockSource::new(clock_port_name);

    let output_port = midi_connection::get_shared_output(output_port_name).unwrap();

    // let _twister = devices::Twister::new("Midi Fighter Twister", vec![
    //     Arc::clone(&bass_offset),
    //     Arc::clone(&keys_offset),
    //     Arc::clone(&mother_offset)
    // ], Arc::clone(&scale), clock.add_rx());

    // let _launchpad = LoopGridLaunchpad::new("Launchpad Mini", vec![
    //     ChunkMap::new( 
    //         Box::new(devices::TR08::new(output_port.clone(), 11)), 
    //         Coords::new(0, 0), 
    //         Shape::new(3, 4)
    //     ),

    //     ChunkMap::new( 
    //         Box::new(devices::SP404::new(output_port.clone(), 12)), 
    //         Coords::new(0, 4), 
    //         Shape::new(3, 4)
    //     ),

    //     ChunkMap::new( 
    //         Box::new(devices::VolcaBass::new(output_port.clone(), 16, Arc::clone(&scale), Arc::clone(&bass_offset))), 
    //         Coords::new(3, 0), 
    //         Shape::new(1, 8)
    //     ),

    //     ChunkMap::new( 
    //         Box::new(devices::VolcaKeys::new(output_port.clone(), 15, Arc::clone(&scale), Arc::clone(&keys_offset))), 
    //         Coords::new(4, 0), 
    //         Shape::new(4, 4)
    //     ),

    //     ChunkMap::new( 
    //         Box::new(devices::Mother32::new(output_port.clone(), 14, Arc::clone(&scale), Arc::clone(&mother_offset))), 
    //         Coords::new(4, 4), 
    //         Shape::new(4, 4)
    //     )
    // ], Arc::clone(&scale), clock.add_rx());


    let _launchpad = LoopGridLaunchpad::new("Launchpad Mini", vec![
        ChunkMap::new( 
            Box::new(devices::VolcaSample::new(output_port.clone())), 
            Coords::new(0, 0), 
            Shape::new(2, 8)
        ),

        ChunkMap::new( 
            Box::new(devices::Mother32::new(output_port.clone(), 14, Arc::clone(&scale), Arc::clone(&mother_offset))), 
            Coords::new(2, 0), 
            Shape::new(5, 8)
        ),

        ChunkMap::new( 
            Box::new(devices::OffsetChunk::new(Arc::clone(&mother_offset))), 
            Coords::new(7, 0), 
            Shape::new(1, 8)
        )
    ], Arc::clone(&scale), clock.add_rx());

    clock.start();
}
