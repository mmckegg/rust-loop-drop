#[macro_use]
extern crate lazy_static;
extern crate indexmap;
extern crate rand;
extern crate serde;
extern crate serde_json;

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

mod chunk;
mod config;
mod controllers;
mod devices;
mod lfo;
mod loop_event;
mod loop_grid_launchpad;
mod loop_recorder;
mod loop_state;
mod loop_transform;
mod midi_connection;
mod midi_time;
mod output_value;
mod scale;
mod scheduler;
mod throttled_output;

use chunk::{ChunkMap, Triggerable};
use loop_grid_launchpad::{LoopGridLaunchpad, LoopGridParams};
use midi_time::MidiTime;
use scale::{Offset, Scale};
use scheduler::Scheduler;

const APP_NAME: &str = "Loop Drop";
const CONFIG_FILEPATH: &str = "./loopdrop-config.json";

type PortLookup = HashMap<String, midi_connection::SharedMidiOutputConnection>;
type OffsetLookup = HashMap<String, Arc<Mutex<Offset>>>;

fn main() {
    let mut chunks = Vec::new();
    let mut myconfig = config::Config::default();

    // TODO: enable config persistence when loaded with filepath
    // if Path::new(CONFIG_FILEPATH).exists() {
    //     myconfig = config::Config::read(CONFIG_FILEPATH).unwrap();
    //     println!("Read config from {}", CONFIG_FILEPATH);
    // } else {
    //     myconfig.write(CONFIG_FILEPATH).unwrap();
    //     println!("Wrote config to {}", CONFIG_FILEPATH);
    // }

    let output = midi_connection::MidiOutput::new(APP_NAME).unwrap();
    let input = midi_connection::MidiInput::new(APP_NAME).unwrap();

    println!("Midi Outputs: {:?}", midi_connection::get_outputs(&output));
    println!("Midi Inputs: {:?}", midi_connection::get_inputs(&input));

    let clock_input_name = &myconfig.clock_input_port_name;

    let scale = Scale::new(60, 0);

    let params = Arc::new(Mutex::new(LoopGridParams {
        swing: 0.0,
        bank: 0,
        frozen: false,
        cueing: false,
        channel_triggered: HashSet::new(),
        align_offset: MidiTime::zero(),
        reset_automation: false,
    }));

    let launchpad_io_name = if cfg!(target_os = "linux") {
        "Launchpad Pro MK3"
    } else {
        "Launchpad Pro MK3 LPProMK3 MIDI"
    };

    let mut output_ports = HashMap::new();
    let mut offset_lookup = HashMap::new();

    for chunk in myconfig.chunks {
        chunks.push(ChunkMap::new(
            make_device(chunk.device, &mut output_ports, &mut offset_lookup, &scale),
            chunk.coords,
            chunk.shape,
            chunk.color,
            chunk.channel,
            chunk.repeat_mode,
        ))
    }

    let mut launchpad = LoopGridLaunchpad::new(launchpad_io_name, chunks, Arc::clone(&params));

    let mut controller_references: Vec<Box<dyn controllers::Schedulable>> = Vec::new();

    for controller in myconfig.controllers {
        controller_references.push(match controller {
            config::ControllerConfig::Twister {
                port_name,
                mixer_port,
                modulators,
            } => Box::new(controllers::Twister::new(
                &port_name,
                get_port(&mut output_ports, &mixer_port.name),
                mixer_port.channel,
                resolve_modulators(&mut output_ports, &modulators),
                Arc::clone(&params),
            )),
            config::ControllerConfig::Umi3 { port_name } => Box::new(controllers::Umi3::new(
                &port_name,
                launchpad.remote_tx.clone(),
            )),
            config::ControllerConfig::VT4Key { output } => {
                let device_port = get_port(&mut output_ports, &output.name);
                Box::new(controllers::VT4Key::new(
                    device_port,
                    output.channel,
                    scale.clone(),
                ))
            }
            config::ControllerConfig::Init { modulators } => Box::new(controllers::Init::new(
                resolve_modulators(&mut output_ports, &modulators),
            )),
        })
    }

    let mut clock_outputs: Vec<midi_connection::SharedMidiOutputConnection> = Vec::new();
    for name in myconfig.clock_output_port_names {
        clock_outputs.push(get_port(&mut output_ports, &name))
    }

    let mut keep_alive_outputs: Vec<midi_connection::SharedMidiOutputConnection> = Vec::new();
    for name in myconfig.keep_alive_port_names {
        keep_alive_outputs.push(get_port(&mut output_ports, &name))
    }

    let mut resync_outputs: Vec<midi_connection::SharedMidiOutputConnection> = Vec::new();
    for name in myconfig.resync_port_names {
        resync_outputs.push(get_port(&mut output_ports, &name))
    }

    for range in Scheduler::start(clock_input_name) {
        // sending clock is the highest priority, so lets do these first
        if range.ticked {
            if range.tick_pos % MidiTime::from_beats(32) == MidiTime::zero() {
                for output in &mut resync_outputs {
                    output.send(&[250]).unwrap();
                }
            }

            for output in &mut clock_outputs {
                output.send(&[248]).unwrap();
            }
        }

        // if range.ticked && range.from.ticks() != range.to.ticks() {
        //     // HACK: straighten out missing sub ticks into separate schedules
        //     let mut a = range.clone();
        //     a.to = MidiTime::new(a.to.ticks(), 0);
        //     a.tick_pos = MidiTime::new(a.from.ticks(), 0);
        //     a.ticked = false;
        //     let mut b = range.clone();
        //     b.from = MidiTime::new(b.to.ticks(), 0);
        //     launchpad.schedule(a);
        //     launchpad.schedule(b);
        // } else {
        let start = Instant::now();
        launchpad.schedule(range);
        if start.elapsed() > Duration::from_millis(15) {
            println!("[WARN] SCHEDULE TIME {:?}", start.elapsed());
        }
        // }

        // now for the lower priority stuff
        if range.ticked {
            let length = MidiTime::tick();
            for controller in &mut controller_references {
                controller.schedule(range.tick_pos, length)
            }

            for output in &mut keep_alive_outputs {
                output.send(&[254]).unwrap();
            }
        }
    }
}

// Helper functions
fn resolve_modulators(
    output_ports: &mut PortLookup,
    modulators: &Vec<Option<config::ModulatorConfig>>,
) -> Vec<Option<controllers::Modulator>> {
    modulators
        .iter()
        .map(|modulator| {
            if let Some(modulator) = modulator {
                Some(controllers::Modulator {
                    port: get_port(output_ports, &modulator.port.name),
                    channel: modulator.port.channel,
                    rx_port: modulator.rx_port.clone(),
                    modulator: modulator.modulator.clone(),
                })
            } else {
                None
            }
        })
        .collect()
}

fn get_port(
    ports_lookup: &mut PortLookup,
    port_name: &str,
) -> midi_connection::SharedMidiOutputConnection {
    if !ports_lookup.contains_key(port_name) {
        ports_lookup.insert(
            String::from(port_name),
            midi_connection::get_shared_output(port_name),
        );
    }

    ports_lookup.get(port_name).unwrap().clone()
}

fn get_offset(offset_lookup: &mut OffsetLookup, id: &str) -> Arc<Mutex<Offset>> {
    if !offset_lookup.contains_key(id) {
        offset_lookup.insert(String::from(id), Offset::new(0));
    }

    offset_lookup.get(id).unwrap().clone()
}

fn set_offset(offset: Arc<Mutex<Offset>>, note_offset: &i32) {
    let mut value = offset.lock().unwrap();

    value.base = *note_offset;
}

fn make_device(
    device: config::DeviceConfig,
    output_ports: &mut PortLookup,
    offset_lookup: &mut OffsetLookup,
    scale: &Arc<Mutex<Scale>>,
) -> Box<Triggerable + Send> {
    let mut output_ports = output_ports;
    let mut offset_lookup = offset_lookup;

    match device {
        config::DeviceConfig::Multi { devices } => {
            let instances = devices
                .iter()
                .map(|device| make_device(device.clone(), output_ports, offset_lookup, scale))
                .collect();
            Box::new(devices::MultiChunk::new(instances))
        }
        config::DeviceConfig::MidiKeys {
            output,
            offset_id,
            note_offset,
            octave_offset,
            velocity_map,
        } => {
            let device_port = get_port(&mut output_ports, &output.name);
            let offset = get_offset(&mut offset_lookup, &offset_id);
            set_offset(offset.clone(), &note_offset);

            Box::new(devices::MidiKeys::new(
                device_port,
                output.channel,
                scale.clone(),
                offset,
                octave_offset,
                velocity_map,
            ))
        }
        config::DeviceConfig::OffsetChunk { id } => Box::new(devices::OffsetChunk::new(
            get_offset(&mut offset_lookup, &id),
        )),
        config::DeviceConfig::RootSelect { output_modulators } => {
            Box::new(devices::RootSelect::new(
                scale.clone(),
                resolve_modulators(&mut output_ports, &output_modulators),
            ))
        }
        config::DeviceConfig::ScaleSelect => Box::new(devices::ScaleSelect::new(scale.clone())),
        config::DeviceConfig::PitchOffsetChunk { output } => {
            Box::new(devices::PitchOffsetChunk::new(
                get_port(&mut output_ports, &output.name),
                output.channel,
            ))
        }
        config::DeviceConfig::MidiTriggers {
            output,
            sidechain_output,
            trigger_ids,
            velocity_map,
        } => {
            let device_port = get_port(&mut output_ports, &output.name);

            let sidechain_output = if let Some(sidechain_output) = sidechain_output {
                Some(devices::SidechainOutput {
                    midi_port: get_port(&mut output_ports, &sidechain_output.port.name),
                    midi_channel: sidechain_output.port.channel,
                    trigger_id: sidechain_output.trigger_id,
                    id: sidechain_output.id,
                })
            } else {
                None
            };

            Box::new(devices::MidiTriggers::new(
                device_port,
                output.channel,
                sidechain_output,
                trigger_ids,
                velocity_map,
            ))
        }
    }
}
