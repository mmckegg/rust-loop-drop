use std::error::Error;
use std::io::{stdin};

mod midi_connection;
mod loop_grid_launchpad;

use loop_grid_launchpad::LoopGridLaunchpad;
use loop_grid_launchpad::LoopGridMessage;

fn main() {
    match run() {
        Ok(_) => (),
        Err(err) => println!("Error: {}", err.description())
    }
}

fn run() -> Result<(), Box<Error>> {
    let launchpad = LoopGridLaunchpad::new("Launchpad Mini");
    let launchpad_clock_channel = launchpad.get_channel();

    let mut ticks = 0;
    let clock_in = midi_connection::getInput("IAC Driver Bus 1", move |stamp, message, _| {
        if message[0] == 248 {
            ticks += 1;
            launchpad_clock_channel.send(LoopGridMessage::Schedule(ticks));
        }
    }, ());

    let mut input = String::new();
    stdin().read_line(&mut input)?; // wait for next enter key press
    println!("Closing connections");
    Ok(())
}