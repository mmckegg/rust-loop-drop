use ::loop_grid_launchpad::LoopGridRemoteEvent;
use ::midi_connection;

use std::sync::mpsc;

pub struct Umi3 {
    _midi_input: midi_connection::ThreadReference
}

impl Umi3 {
    pub fn new (port_name: &str, remote_tx: mpsc::Sender<LoopGridRemoteEvent>) -> Self {
        let input = midi_connection::get_input(port_name, move |_stamp, message| {
            match message {
                [144, 60, velocity] => {
                    remote_tx.send(LoopGridRemoteEvent::LoopButton(velocity > &0)).unwrap();
                },
                [144, 62, velocity] => {
                    remote_tx.send(LoopGridRemoteEvent::DoubleButton(velocity > &0)).unwrap();
                },
                [144, 64, velocity] => {
                    remote_tx.send(LoopGridRemoteEvent::SustainButton(velocity > &0)).unwrap();
                },
                _ => ()
            }
        });

        Umi3 {
            _midi_input: input
        }
    }
}