#[macro_use] extern crate lazy_static;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;

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

    let usb_io_name = "UM-ONE";
    
    // if cfg!(target_os = "linux") {
    //     "pisound"
    // } else {
    //     "UM-ONE"
    // }; 

    let main_io_name = "pisound";
    // if cfg!(target_os = "linux") {
    //     "Boutique"
    // } else {
    //     "TR-08"
    // };

    let scale = Scale::new(69, 0);
    let bass_offset = Offset::new(-2);
    let keys_offset = Offset::new(-1);

    let sp404a_offset = Arc::new(AtomicUsize::new(0));
    let sp404b_offset = Arc::new(AtomicUsize::new(0));

    // let parva_port = midi_connection::get_shared_output("Parva").unwrap();

    let main_output_port = midi_connection::get_shared_output(main_io_name).unwrap();
    let usb_output_port = midi_connection::get_shared_output(usb_io_name).unwrap();
    let mut clock = ClockSource::new(usb_io_name, vec![
        usb_output_port.clone(), 
        main_output_port.clone(),
        midi_connection::get_shared_output("Launchpad MK2").unwrap()
    ]);

    let _twister = devices::Twister::new("Midi Fighter Twister", "K-Mix", vec![
        (main_output_port.clone(), 1),
        (main_output_port.clone(), 2),
        (main_output_port.clone(), 3)
    ], clock.add_rx());

    let _kboard = devices::KBoard::new("K-Board", main_output_port.clone(), 3, Arc::clone(&scale));

    let _launchpad = LoopGridLaunchpad::new("Launchpad MK2", vec![
        // ChunkMap::new( 
        //     Box::new(devices::TR08::new(main_output_port.clone(), 11)), 
        //     Coords::new(0, 0), 
        //     Shape::new(4, 4)
        // ),

        // ChunkMap::new( 
        //     Box::new(devices::Mother32::new(main_output_port.clone(), 14, Arc::clone(&scale), Arc::clone(&mother_offset))), 
        //     Coords::new(0, 4), 
        //     Shape::new(4, 4)
        // ),

        ChunkMap::new(
            Box::new(devices::SP404Offset::new(Arc::clone(&sp404a_offset))), 
            Coords::new(0 + 8, 0), 
            Shape::new(3, 4)
        ),

        ChunkMap::new( 
            Box::new(devices::SP404Offset::new(Arc::clone(&sp404b_offset))),
            Coords::new(0 + 8, 4), 
            Shape::new(3, 4)
        ),

        ChunkMap::new( 
            Box::new(devices::OffsetChunk::new(Arc::clone(&bass_offset))), 
            Coords::new(3 + 8, 0), 
            Shape::new(1, 8)
        ),

        ChunkMap::new( 
            Box::new(devices::OffsetChunk::new(Arc::clone(&keys_offset))), 
            Coords::new(7 + 8, 0), 
            Shape::new(1, 8)
        ),

        ChunkMap::new(
            Box::new(devices::SP404::new(usb_output_port.clone(), 12, Arc::clone(&sp404a_offset))), 
            Coords::new(0, 0), 
            Shape::new(3, 4)
        ),

        ChunkMap::new( 
            Box::new(devices::SP404::new(usb_output_port.clone(), 12, Arc::clone(&sp404b_offset))), 
            Coords::new(0, 4), 
            Shape::new(3, 4)
        ),
        
        ChunkMap::new( 
            Box::new(devices::VolcaBass::new(main_output_port.clone(), 1, Arc::clone(&scale), Arc::clone(&bass_offset))), 
            Coords::new(3, 0), 
            Shape::new(1, 8)
        ),

        ChunkMap::new( 
            Box::new(devices::VolcaBass::new(main_output_port.clone(), 2, Arc::clone(&scale), Arc::clone(&keys_offset))), 
            Coords::new(4, 0), 
            Shape::new(4, 8)
        )
    ], Arc::clone(&scale), clock.add_rx());

    clock.start();
}
