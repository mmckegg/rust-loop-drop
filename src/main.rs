#[macro_use] extern crate lazy_static;
use std::sync::Arc;

mod midi_connection;
mod loop_grid_launchpad;
mod loop_recorder;
mod loop_event;
mod loop_state;
mod loop_transform;
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

    let midi_io_name = if cfg!(target_os = "linux") {
        "pisound"
    } else {
        "UM-ONE"
    }; 

    let tr08_port_name = if cfg!(target_os = "linux") {
        "Boutique"
    } else {
        "TR-08"
    };

    let scale = Scale::new(69, 0);
    let bass_offset = Offset::new(-3);
    let mother_offset = Offset::new(-2);
    let keys_offset = Offset::new(-1);

    let tr08_port = midi_connection::get_shared_output(tr08_port_name).unwrap();
    let output_port = midi_connection::get_shared_output(midi_io_name).unwrap();
    let mut clock = ClockSource::new(midi_io_name, vec![
        output_port.clone(), 
        tr08_port.clone(),
        midi_connection::get_shared_output("Launchpad MK2").unwrap()
    ]);

    let _twister = devices::Twister::new("Midi Fighter Twister", vec![
        Arc::clone(&bass_offset),
        Arc::clone(&keys_offset),
        Arc::clone(&mother_offset)
    ], Arc::clone(&scale), clock.add_rx());

    let _kboard = devices::KBoard::new("K-Board", output_port.clone(), 15, Arc::clone(&scale));

    let _launchpad = LoopGridLaunchpad::new("Launchpad MK2", vec![
        ChunkMap::new( 
            Box::new(devices::TR08::new(tr08_port.clone(), 11)), 
            Coords::new(0, 0), 
            Shape::new(4, 4)
        ),

        ChunkMap::new( 
            Box::new(devices::Mother32::new(tr08_port.clone(), 14, Arc::clone(&scale), Arc::clone(&mother_offset))), 
            Coords::new(0, 4), 
            Shape::new(4, 4)
        ),

        ChunkMap::new( 
            Box::new(devices::VolcaBass::new(output_port.clone(), 16, Arc::clone(&scale), Arc::clone(&bass_offset))), 
            Coords::new(4, 0), 
            Shape::new(1, 8)
        ),

        ChunkMap::new( 
            Box::new(devices::SP404::new(output_port.clone(), 12)), 
            Coords::new(5, 0), 
            Shape::new(3, 4)
        ),

        ChunkMap::new( 
            Box::new(devices::SP404::new(output_port.clone(), 13)), 
            Coords::new(5, 4), 
            Shape::new(3, 4)
        )
    ], Arc::clone(&scale), clock.add_rx());

    clock.start();
}
