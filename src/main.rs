#[macro_use]
extern crate lazy_static;
extern crate indexmap;
extern crate rand;
extern crate serde;
extern crate serde_json;

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::atomic::AtomicBool;
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
mod trigger_envelope;

use chunk::{ChunkMap, Triggerable};
use controllers::Modulator;
use loop_grid_launchpad::{LoopGridLaunchpad, LoopGridParams};
use midi_time::MidiTime;
use scale::{Offset, Scale};
use scheduler::Scheduler;

const APP_NAME: &str = "Loop Drop";
const CONFIG_FILEPATH: &str = "./loopdrop-config.json";

type PortLookup = HashMap<String, midi_connection::SharedMidiOutputConnection>;
type OffsetLookup = HashMap<String, Arc<Mutex<Offset>>>;

fn main() {
    let output = midi_connection::MidiOutput::new(APP_NAME).unwrap();
    let input = midi_connection::MidiInput::new(APP_NAME).unwrap();
    let inputs = midi_connection::get_inputs(&input);
    let has_tr6s = inputs.iter().any(|x| x == "TR-6S");
    let has_sp404 = inputs.iter().any(|x| x == "SP-404MKII");

    let mut chunks = Vec::new();
    let myconfig = if has_sp404 && !has_tr6s {
        config::Config::minimal()
    } else {
        config::Config::default()
    };
    let use_internal_clock = Arc::new(AtomicBool::new(false));

    // TODO: enable config persistence when loaded with filepath
    // if Path::new(CONFIG_FILEPATH).exists() {
    //     myconfig = config::Config::read(CONFIG_FILEPATH).unwrap();
    //     println!("Read config from {}", CONFIG_FILEPATH);
    // } else {
    //     myconfig.write(CONFIG_FILEPATH).unwrap();
    //     println!("Wrote config to {}", CONFIG_FILEPATH);
    // }


    println!("Midi Outputs: {:?}", midi_connection::get_outputs(&output));
    println!("Midi Inputs: {:?}", &inputs);

    let clock_input_name = &myconfig.clock_input_port_name;

    let scale = Scale::new(60);

    let params = Arc::new(Mutex::new(LoopGridParams {
        swing: 0.0,
        bank: 0,
        frozen: false,
        cueing: false,
        duck_triggered: false,
        duck_tick_multiplier: 0.1,
        channel_triggered: HashSet::new(),
        reset_automation: false,
        reset_beat: 0,
        active_notes: HashSet::new(),
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
            make_device(
                chunk.device,
                &mut output_ports,
                &mut offset_lookup,
                &scale,
                &params,
            ),
            chunk.coords,
            chunk.shape,
            chunk.color,
            chunk.channel,
            chunk.repeat_mode,
        ))
    }

    let mut launchpad = LoopGridLaunchpad::new(
        launchpad_io_name,
        chunks,
        Arc::clone(&params),
        Arc::clone(&use_internal_clock),
    );

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
            config::ControllerConfig::ModTwister {
                port_name,
                modulators,
                continuously_send,
                channel_map,
            } => Box::new(controllers::ModTwister::new(
                &port_name,
                resolve_modulators(&mut output_ports, &modulators),
                Arc::clone(&params),
                continuously_send,
                channel_map,
            )),
            config::ControllerConfig::Umi3 { port_name } => Box::new(controllers::Umi3::new(
                &port_name,
                launchpad.remote_tx.clone(),
            )),
            config::ControllerConfig::ClockPulse { output, divider } => {
                let device_port = get_port(&mut output_ports, &output.name);
                Box::new(controllers::ClockPulse::new(
                    device_port,
                    output.channel,
                    divider,
                ))
            }
            config::ControllerConfig::LaunchpadTempo { daw_port_name } => {
                Box::new(controllers::LaunchpadTempo::new(&daw_port_name))
            }
            config::ControllerConfig::Init { modulators } => Box::new(controllers::Init::new(
                resolve_modulators(&mut output_ports, &modulators),
            )),
            config::ControllerConfig::DuckOutput { modulators } => {
                Box::new(controllers::DuckOutput::new(
                    resolve_modulators(&mut output_ports, &modulators),
                    Arc::clone(&params),
                ))
            }
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

    for range in Scheduler::start(clock_input_name, use_internal_clock) {
        // sending clock is the highest priority, so lets do these first
        if range.ticked {
            if (range.tick_pos) % MidiTime::from_beats(32) == MidiTime::zero() {
                for output in &mut resync_outputs {
                    output.send(&[250]).unwrap();
                    output.send(&[242, 0, 0]).unwrap();
                }
            }

            for output in &mut clock_outputs {
                output.send(&[248]).unwrap();
            }
        }

        let start = Instant::now();
        launchpad.schedule(range);
        if start.elapsed() > Duration::from_millis(15) {
            println!("[WARN] SCHEDULE TIME {:?}", start.elapsed());
        }

        // now for the lower priority stuff
        for controller in &mut controller_references {
            controller.schedule(range)
        }

        if range.ticked {
            for output in &mut keep_alive_outputs {
                output.send(&[254]).unwrap();
            }

            // reset duck_triggered on every tick
            let mut params = params.lock().unwrap();
            params.duck_triggered = false;
        }
    }
}

// Helper functions
fn resolve_modulators(
    output_ports: &mut PortLookup,
    modulators: &Vec<config::ModulatorConfig>,
) -> Vec<controllers::Modulator> {
    modulators
        .iter()
        .map(|modulator| match modulator {
            config::ModulatorConfig::None => Modulator::None,
            config::ModulatorConfig::Midi {
                port,
                rx_port,
                modulator,
            } => Modulator::MidiModulator(controllers::MidiModulator::new(
                get_port(output_ports, &port.name),
                port.channel,
                modulator.clone(),
                rx_port.clone(),
            )),
            &config::ModulatorConfig::DuckDecay(default) => Modulator::DuckDecay(default),
            &config::ModulatorConfig::Swing(default) => Modulator::Swing(default),
            &config::ModulatorConfig::LfoAmount(modulator_index, default) => {
                Modulator::LfoAmount(modulator_index, default)
            }
            &config::ModulatorConfig::LfoSpeed(default) => Modulator::LfoSpeed(default),
            &config::ModulatorConfig::LfoHold(default) => Modulator::LfoHold(default),
            &config::ModulatorConfig::LfoOffset(default) => Modulator::LfoOffset(default),
            &config::ModulatorConfig::LfoSkew(default) => Modulator::LfoSkew(default),
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
    params: &Arc<Mutex<LoopGridParams>>,
) -> Box<dyn Triggerable + Send> {
    let mut output_ports = output_ports;
    let mut offset_lookup = offset_lookup;

    match device {
        config::DeviceConfig::Multi { devices } => {
            let instances = devices
                .iter()
                .map(|device| {
                    make_device(device.clone(), output_ports, offset_lookup, scale, params)
                })
                .collect();
            Box::new(devices::MultiChunk::new(instances))
        }
        config::DeviceConfig::MidiKeys {
            output,
            offset_id,
            note_offset,
            octave_offset,
            velocity_map,
            offset_wrap,
            monophonic,
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
                offset_wrap,
                monophonic,
            ))
        }
        config::DeviceConfig::OffsetChunk { id } => Box::new(devices::OffsetChunk::new(
            get_offset(&mut offset_lookup, &id),
        )),
        config::DeviceConfig::RootSelect => Box::new(devices::RootSelect::new(scale.clone())),
        config::DeviceConfig::ScaleDegreeToggle(degree) => Box::new(
            devices::ScaleDegreeToggle::new(scale.clone(), degree, params.clone()),
        ),
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
                    params: Arc::clone(params),
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
        config::DeviceConfig::CcTriggers {
            output,
            triggers,
            velocity_map,
        } => {
            let device_port = get_port(&mut output_ports, &output.name);
            Box::new(devices::CcTriggers::new(
                device_port,
                triggers,
                velocity_map,
            ))
        }
        config::DeviceConfig::Sp404Mk2 {
            port_name,
            velocity_map,
            default_mapping,
            sidechain_output,
        } => {
            let sidechain_output = if let Some(sidechain_output) = sidechain_output {
                Some(devices::SidechainOutput {
                    params: Arc::clone(params),
                    id: sidechain_output.id,
                })
            } else {
                None
            };
            Box::new(devices::Sp404Mk2::new(
                &port_name,
                default_mapping,
                velocity_map,
                sidechain_output,
            ))
        }
    }
}
