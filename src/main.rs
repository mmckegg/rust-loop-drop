#[macro_use] extern crate lazy_static;
use std::thread;
use std::time;
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
use loop_grid_launchpad::LoopGridLaunchpad;
use loop_grid_launchpad::LoopGridMessage;
use chunk::{Shape, Coords, ChunkMap};

fn main() {
    println!("Midi Outputs: {:?}", midi_connection::get_outputs());
    println!("Midi Inputs: {:?}", midi_connection::get_inputs());

    let output_port_name = "UM-ONE";

    let scale = Scale::new(69, 0);
    let bass_offset = Offset::new(-3);
    let mother_offset = Offset::new(-2);
    let keys_offset = Offset::new(-1);

    let twister = devices::Twister::new("Midi Fighter Twister", vec![
        Arc::clone(&bass_offset),
        Arc::clone(&keys_offset),
        Arc::clone(&mother_offset)
    ], Arc::clone(&scale));

    let launchpad = LoopGridLaunchpad::new("Launchpad Mini", output_port_name, vec![
        ChunkMap::new( 
            Box::new(devices::TR08::new(output_port_name, 11)), 
            Coords::new(0, 0), 
            Shape::new(3, 4)
        ),

        ChunkMap::new( 
            Box::new(devices::SP404::new(output_port_name, 12)), 
            Coords::new(0, 4), 
            Shape::new(3, 4)
        ),

        ChunkMap::new( 
            Box::new(devices::VolcaBass::new(output_port_name, 16, Arc::clone(&scale), Arc::clone(&bass_offset))), 
            Coords::new(3, 0), 
            Shape::new(1, 8)
        ),

        ChunkMap::new( 
            Box::new(devices::VolcaKeys::new(output_port_name, 15, Arc::clone(&scale), Arc::clone(&keys_offset))), 
            Coords::new(4, 0), 
            Shape::new(4, 4)
        ),

        ChunkMap::new( 
            Box::new(devices::Mother32::new(output_port_name, 14, Arc::clone(&scale), Arc::clone(&mother_offset))), 
            Coords::new(4, 4), 
            Shape::new(4, 4)
        )
    ], Arc::clone(&scale));

    let launchpad_clock_channel = launchpad.get_channel();

    let _clock_in = midi_connection::get_input("UM-ONE", move |_stamp, message, _| {
        if message[0] == 248 {
            launchpad_clock_channel.send(LoopGridMessage::TickFromExternal);
        } else if message[0] == 250 {
            launchpad_clock_channel.send(LoopGridMessage::ResetBeat);
        }
    }, ());

    loop {
        // keep app alive
        thread::sleep(time::Duration::from_millis(500));
    }
}
