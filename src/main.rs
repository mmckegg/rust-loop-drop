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
    match run() {
        Ok(_) => (),
        Err(err) => println!("Error: {}", err.description())
    }
}

fn run() -> Result<(), Box<Error>> {

    println!("Midi Outputs: {:?}", midi_connection::get_outputs());
    println!("Midi Inputs: {:?}", midi_connection::get_inputs());

    let launchpad = LoopGridLaunchpad::new("Launchpad Mini");
    let launchpad_clock_channel = launchpad.get_channel();
    let tempo = 120;

    let mut ticks = 0;
    // let clock_in = midi_connection::get_input("IAC Driver Bus 1", move |stamp, message, _| {
    //     if message[0] == 248 {
    //         ticks += 1;
    //         launchpad_clock_channel.send(LoopGridMessage::Schedule(ticks));
    //     }
    // }, ());

    thread::spawn(move || {
        loop {
            launchpad_clock_channel.send(LoopGridMessage::Schedule(ticks));
            ticks += 1;
            thread::sleep(time::Duration::from_millis(1000 / (tempo / 60) / 24));
        }
    });


    let mut input = String::new();
    stdin().read_line(&mut input)?; // wait for next enter key press
    println!("Closing connections");
    Ok(())
}