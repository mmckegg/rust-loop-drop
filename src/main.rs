#[macro_use] extern crate lazy_static;
extern crate rand;
use rand::Rng;

use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use std::process::Command;
use std::process;
use std::time::{Duration, Instant};

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
mod scheduler;

use scale::{Scale, Offset};
use loop_grid_launchpad::{LoopGridLaunchpad, LoopGridParams};
use chunk::{Shape, Coords, ChunkMap, RepeatMode};
use std::sync::atomic::AtomicUsize;
use ::midi_time::{MidiTime, SUB_TICKS};
use scheduler::{Scheduler, ScheduleRange};
use std::sync::mpsc;

const APP_NAME: &str = "Loop Drop";

fn main() {

    Command::new("renice").args(&["-n", "-20", &format!("{}", process::id())]).output();
    let output = midi_connection::MidiOutput::new(APP_NAME).unwrap();
    let input = midi_connection::MidiInput::new(APP_NAME).unwrap();

    println!("Midi Outputs: {:?}", midi_connection::get_outputs(&output));
    println!("Midi Inputs: {:?}", midi_connection::get_inputs(&input));
    
    let all_io_name = "RK006"; // All out
    let rk006_input_2 = "RK006 PORT 3"; // 2

    let blackbox_io_name = "RK006 PORT 2"; // 1
    let zoia_io_name = "RK006 PORT 3"; // 2
    let typhon_io_name = "RK006 PORT 4"; // 3
    let tr06s_io_name = "TR-6S";
    let rk006_output_4 = "RK006 PORT 5"; // 4
    let dfam_pwm_name = "RK006 PORT 7"; // 6
    let vt4_io_name = "VT-4";
    let keyboard_io_name = "K-Board";
    let streichfett_io_name = "Streichfett";
    let geode_io_name = "USB MIDI";

    let launchpad_io_name = if cfg!(target_os = "linux") {
        "Launchpad Pro MK3"
    } else {
        "Launchpad Pro MK3 LPProMK3 MIDI"
    };

    let scale = Scale::new(60, 0);

    let params = Arc::new(Mutex::new(LoopGridParams { 
        swing: 0.0,
        bank: 0,
        frozen: false,
        align_offset: MidiTime::zero(),
        reset_automation: false
    }));
    
    let drum_velocities = Arc::new(Mutex::new(HashMap::new()));

    { // release lock
        let mut v = drum_velocities.lock().unwrap();
        v.insert(0, 127);
        v.insert(4, 127);
    }

    let bass_offset = Offset::new(-2, -4);
    let geode_offset = Offset::new(-1, -4);
    let keys_offset = Offset::new(0, -4);

    let tr6s_output_port = midi_connection::get_shared_output(tr06s_io_name);
    let typhon_output_port = midi_connection::get_shared_output(typhon_io_name);
    let blackbox_output_port = midi_connection::get_shared_output(blackbox_io_name);
    let zoia_output_port = midi_connection::get_shared_output(zoia_io_name);
    let rk006_output_4_port = midi_connection::get_shared_output(rk006_output_4);
    let dfam_pwm_name = midi_connection::get_shared_output(dfam_pwm_name);
    let all_output_port = midi_connection::get_shared_output(all_io_name);
    
    let vt4_output_port = midi_connection::get_shared_output(vt4_io_name);
    let streichfett_output_port = midi_connection::get_shared_output(streichfett_io_name);
    let geode_output_port = midi_connection::get_shared_output(geode_io_name);

    let mut launchpad = LoopGridLaunchpad::new(launchpad_io_name, vec![

        // EXT SYNTH
        // Send this to Geode, Blackbox (channel 1), Blackbox (channel 2 but as slicer rather than pitch), and RK-006 port 4 (TRS)
        ChunkMap::new(
            Box::new(devices::MultiChunk::new(vec![
                Box::new(devices::MidiKeys::new(vec![blackbox_output_port.clone(), rk006_output_4_port.clone()], 1, Arc::clone(&scale), Arc::clone(&geode_offset))), 
                Box::new(devices::MidiKeys::new(vec![blackbox_output_port.clone(), rk006_output_4_port.clone()], 2, Arc::clone(&scale), Arc::clone(&geode_offset))), 
                Box::new(devices::BlackboxSlicer::new(blackbox_output_port.clone(), 2))
            ])),
            Coords::new(0 + 8, 0),
            Shape::new(3, 8),
            125, // gross
            Some(2),
            RepeatMode::Global
        ),

        // EXT SYNTH OFFSET 
        // (also sends pitch mod on channel 2 for slicer)
        ChunkMap::new(
            Box::new(devices::MultiChunk::new(vec![
                Box::new(devices::OffsetChunk::new(Arc::clone(&geode_offset))),
                Box::new(devices::PitchOffsetChunk::new(blackbox_output_port.clone(), 2))
            ])),
            Coords::new(3 + 8, 0), 
            Shape::new(1, 8),
            12, // soft yellow
            None,
            RepeatMode::None
        ),

        // BASS OFFSET
        ChunkMap::new(
            Box::new(devices::OffsetChunk::new(Arc::clone(&bass_offset))),
            Coords::new(4 + 8, 0), 
            Shape::new(1, 8),
            55, // pink
            None,
            RepeatMode::None
        ),

        // SYNTH OFFSET
        ChunkMap::new(
            Box::new(devices::OffsetChunk::new(Arc::clone(&keys_offset))), 
            Coords::new(5 + 8, 0), 
            Shape::new(1, 8),
            43, // blue
            None,
            RepeatMode::None
        ),
        
        // ROOT NOTE SELECTOR
        ChunkMap::new(
            Box::new(devices::RootSelect::new(Arc::clone(&scale))), 
            Coords::new(6 + 8, 0), 
            Shape::new(2, 8),
            35, // soft green
            None,
            RepeatMode::None
        ),

        // SCALE MODE SELECTOR
        ChunkMap::new(
            Box::new(devices::ScaleSelect::new(Arc::clone(&scale))), 
            Coords::new(16, 0), 
            Shape::new(1, 8),
            0, // black
            None,
            RepeatMode::None
        ),

        // DRUMS
        ChunkMap::new(
            Box::new(devices::TR6s::new(tr6s_output_port.clone(), 10, zoia_output_port.clone(), 16, Arc::clone(&drum_velocities))), 
            Coords::new(0, 0), 
            Shape::new(1, 6),
            15, // yellow
            Some(0),
            RepeatMode::Global
        ),

        // EXTRA PERC (to fill in for only 6 triggers on cycles)
        ChunkMap::new(
            Box::new(devices::BlackboxPerc::new(blackbox_output_port.clone(), 10, Arc::clone(&drum_velocities))), 
            Coords::new(0, 6), 
            Shape::new(1, 2),
            9, // orange
            Some(1),
            RepeatMode::Global
        ),

        // SAMPLER
        ChunkMap::new(
            Box::new(devices::BlackboxSample::new(blackbox_output_port.clone(), 10)), 
            Coords::new(1, 0), 
            Shape::new(1, 8),
            9, // orange
            Some(1),
            RepeatMode::Global
        ),

        // BASS
        ChunkMap::new(
            Box::new(devices::MidiKeys::new(vec![typhon_output_port.clone(), blackbox_output_port.clone()], 11, Arc::clone(&scale), Arc::clone(&bass_offset))), 
            Coords::new(2, 0), 
            Shape::new(3, 8),
            59, // pink
            Some(2),
            RepeatMode::Global
        ),

        // SYNTH
        ChunkMap::new(
            Box::new(devices::MidiKeys::new(vec![streichfett_output_port.clone()], 1, Arc::clone(&scale), Arc::clone(&keys_offset))), 
            Coords::new(5, 0), 
            Shape::new(3, 8),
            43, // blue
            Some(3),
            RepeatMode::Global
        )
    ], Arc::clone(&scale), Arc::clone(&params), blackbox_output_port.clone(), 10, 36);

    let _keyboard = devices::KBoard::new(keyboard_io_name, streichfett_output_port.clone(), 1, scale.clone());

    let twister = devices::Twister::new("Midi Fighter Twister",
        typhon_output_port.clone(),
        streichfett_output_port.clone(),
        tr6s_output_port.clone(),
        blackbox_output_port.clone(),
        zoia_output_port.clone(),
        dfam_pwm_name.clone(),
        Arc::clone(&params)
    );

    let _pedal = devices::Umi3::new("Logidy UMI3", launchpad.remote_tx.clone());

    let mut vt4 = devices::VT4Key::new(vt4_output_port.clone(), 8, scale.clone());
    
    let mut clock_blackbox_output_port = blackbox_output_port.clone();
    let mut loopback_blackbox_output_port = blackbox_output_port.clone();
    let mut tr6s_clock_output_port = tr6s_output_port.clone();

    let _bbx_loopback = midi_connection::get_input(rk006_input_2, move |_stamp, msg| {
        // messages on channels 1 - 9 are forwarded back into blackbox
        if (msg[0] >= 128 && msg[0] < 128 + 9) || (msg[0] >= 144 && msg[0] < 144 + 9) {
            loopback_blackbox_output_port.send(msg).unwrap();
        }
    });

    for range in Scheduler::start(tr06s_io_name) {
        // sending clock is the highest priority, so lets do these first
        if range.ticked {
            if range.tick_pos % MidiTime::from_beats(32) == MidiTime::zero() {
                clock_blackbox_output_port.send(&[250]).unwrap();
            }
        }
        
        if range.ticked && range.from.ticks() != range.to.ticks() {
            // HACK: straighten out missing sub ticks into separate schedules
            let mut a = range.clone();
            a.to = MidiTime::new(a.to.ticks(), 0);
            a.tick_pos = MidiTime::new(a.from.ticks(), 0);
            a.ticked = false;
            let mut b = range.clone();
            b.from = MidiTime::new(b.to.ticks(), 0);
            launchpad.schedule(a);
            launchpad.schedule(b);
        } else {
            launchpad.schedule(range);
        }
        
        // now for the lower priority stuff
        if range.ticked {
            let length = MidiTime::tick();
            twister.schedule(range.tick_pos, length);
            vt4.schedule(range.tick_pos, length);

            // keep the tr6s midi input active (otherwise it stops responding to incoming midi immediately)
            tr6s_clock_output_port.send(&[254]).unwrap();
        }
    }
}

