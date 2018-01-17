#[macro_use] extern crate lazy_static;
use std::error::Error;
use std::io::{stdin};
use std::thread;
use std::time;

mod midi_connection;
mod loop_grid_launchpad;
mod loop_recorder;
mod loop_state;

use loop_grid_launchpad::LoopGridLaunchpad;
use loop_grid_launchpad::LoopGridMessage;

fn main() {
    println!("Midi Outputs: {:?}", midi_connection::get_outputs());
    println!("Midi Inputs: {:?}", midi_connection::get_inputs());

    let launchpad = LoopGridLaunchpad::new("Launchpad Mini", "UM-ONE");
    let launchpad_clock_channel = launchpad.get_channel();
    let mut ticks = 0;

    let clock_in = midi_connection::get_input("UM-ONE", move |stamp, message, _| {
        if message[0] == 248 {
            ticks += 1;
            launchpad_clock_channel.send(LoopGridMessage::Schedule(ticks));
        }
    }, ());

    // let tempo = 120;
    // thread::spawn(move || {
    //     loop {
    //         launchpad_clock_channel.send(LoopGridMessage::Schedule(ticks));
    //         ticks += 1;
    //         thread::sleep(time::Duration::from_millis(1000 / (tempo / 60) / 24));
    //     }
    // });

    loop {
        // keep app alive
        thread::sleep(time::Duration::from_millis(500));
    }
}
