#[macro_use]
extern crate lazy_static;
extern crate indexmap;
extern crate rand;
extern crate serde;
extern crate serde_json;

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

mod chunk;
mod config;
mod controllers;
mod devices;
mod lfo;
mod loop_event;
mod loop_grid;
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

use chunk::{ChunkMap, RepeatMode, Shape, Triggerable};
use loop_grid::{LoopGrid, LoopGridParams};
use midi_time::MidiTime;
use scale::{Offset, Scale};
use scheduler::Scheduler;

const APP_NAME: &str = "Loop Drop";
// const CONFIG_FILEPATH: &str = "./loopdrop-config.json";

type PortLookup = HashMap<String, midi_connection::SharedMidiOutputConnection>;
type OffsetLookup = HashMap<String, Arc<Mutex<Offset>>>;

fn main() {
    let output = midi_connection::MidiOutput::new(APP_NAME).unwrap();
    let input = midi_connection::MidiInput::new(APP_NAME).unwrap();
    let inputs = midi_connection::get_inputs(&input);

    let myconfig = config::Config::default();
    let use_internal_clock = Arc::new(AtomicBool::new(false));
    let internal_bpm = Arc::new(Mutex::new(120.0));

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

    let clock_input_name = myconfig.clock_input_port_name.as_deref();
    if clock_input_name.is_none() {
        use_internal_clock.store(true, Ordering::Relaxed);
    }

    let scale = Scale::new(60);

    let params = Arc::new(Mutex::new(LoopGridParams {
        swing: 0.0,
        bank: 0,
        select_held: false,
        prepare_held: false,
        root_overlay_until: None,
        root_overlay_note: None,
        lfo_speed_overlay_until: None,
        lfo_speed_overlay_value: None,
        lfo_wave_overlay_until: None,
        lfo_wave_overlay_mode: None,
        frozen: false,
        cueing: false,
        duck_triggered: false,
        duck_tick_multiplier: 0.1,
        duck_reduction: 0.5,
        channel_triggered: HashSet::new(),
        activity_flash_until: HashMap::new(),
        reset_automation: false,
        reset_beat: 0,
        active_notes: HashSet::new(),
        slicer_offsets: HashMap::new(),
        slicer_pitches: HashMap::new(),
    }));

    let sample_mixer_state = Arc::new(Mutex::new(controllers::SampleMixerState::new()));

    let mut output_ports = HashMap::new();
    let mut offset_lookup = HashMap::new();
    let chunks = build_chunks(
        &myconfig,
        &mut output_ports,
        &mut offset_lookup,
        &scale,
        &params,
    );

    let mut loop_grid = LoopGrid::new(
        chunks,
        Arc::clone(&params),
        Arc::clone(&use_internal_clock),
        Arc::clone(&internal_bpm),
    );

    let mut controller_references: Vec<Box<dyn controllers::Schedulable>> = vec![
        Box::new(controllers::ModulationSurface::new(
            myconfig.modulation.encoders.clone(),
            myconfig.modulation.tap_ms,
            myconfig.modulation.double_tap_ms,
            myconfig.modulation.hold_ms,
            Arc::clone(&params),
            Arc::clone(&scale),
            controllers::ModulationSurfaceShared {
                sample_mixer_state: Arc::clone(&sample_mixer_state),
            },
            &mut output_ports,
        )),
        Box::new(controllers::SampleMixer::new(
            get_port(&mut output_ports, &myconfig.samples.output.name),
            myconfig.samples.output.channel,
            myconfig.samples.volume_ccs.to_vec(),
            vec![2, 3, 10, 11, 12, 13, 14, 15],
            Arc::clone(&params),
            Arc::clone(&sample_mixer_state),
        )),
    ];

    // Encoder feedback is now driven by app config, so do not request the full
    // controller state dump on connect.

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

    for range in Scheduler::start(
        clock_input_name,
        use_internal_clock,
        Arc::clone(&internal_bpm),
    ) {
        // sending clock is the highest priority, so lets do these first
        if range.ticked {
            if (range.tick_pos) % MidiTime::from_beats(32) == MidiTime::zero() {
                for output in &mut resync_outputs {
                    output.send(&[250]).unwrap();
                    output.send(&[242, 0, 0]).unwrap();
                }

                for output in &mut keep_alive_outputs {
                    output.send(&[254]).unwrap();
                }
            }

            for output in &mut clock_outputs {
                output.send(&[248]).unwrap();
            }
        }

        let start = Instant::now();
        loop_grid.schedule(range);
        if start.elapsed() > Duration::from_millis(15) {
            println!("[WARN] SCHEDULE TIME {:?}", start.elapsed());
        }

        // now for the lower priority stuff
        for controller in &mut controller_references {
            controller.schedule(range)
        }

        if range.ticked {
            // reset shared per-tick flags after all controllers have consumed them
            let mut params = params.lock().unwrap();
            params.duck_triggered = false;
            params.channel_triggered.clear();
            let now = Instant::now();
            params.activity_flash_until.retain(|_, until| *until > now);
        }
    }
}

// Helper functions
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

fn build_chunks(
    config: &config::Config,
    output_ports: &mut PortLookup,
    offset_lookup: &mut OffsetLookup,
    scale: &Arc<Mutex<Scale>>,
    params: &Arc<Mutex<LoopGridParams>>,
) -> Vec<Box<ChunkMap>> {
    let mut chunks = Vec::new();

    let make_offset = |id: &str, offset_lookup: &mut OffsetLookup| {
        Box::new(devices::OffsetChunk::new(get_offset(offset_lookup, id)))
            as Box<dyn Triggerable + Send>
    };

    let make_voice = |voice: &config::VoiceConfig,
                      offset_id: &str,
                      output_ports: &mut PortLookup,
                      offset_lookup: &mut OffsetLookup,
                      scale: &Arc<Mutex<Scale>>| {
        let device_port = get_port(output_ports, &voice.output.name);
        let offset = get_offset(offset_lookup, offset_id);
        set_offset(offset.clone(), &voice.note_offset);
        Box::new(devices::MidiKeys::new(
            device_port,
            voice.output.channel,
            scale.clone(),
            offset,
            voice.octave_offset,
            None,
            voice.offset_wrap,
            voice.monophonic,
            0,
        )) as Box<dyn Triggerable + Send>
    };

    let make_note_triggers =
        |output: &config::MidiPortConfig, notes: &[u8], output_ports: &mut PortLookup| {
            Box::new(devices::MidiTriggers::new(
                get_port(output_ports, &output.name),
                output.channel,
                None,
                notes.to_vec(),
                None,
            )) as Box<dyn Triggerable + Send>
        };

    chunks.push(
        ChunkMap::new(
            make_note_triggers(
                &config.samples.output,
                &config.samples.notes[4..8],
                output_ports,
            ),
            chunk::Coords::new(0, 4),
            Shape::new(1, 4),
            config.samples.color.to_midi(),
            None,
            Some(vec![12, 13, 14, 15]),
            RepeatMode::NoCycle,
        )
        .with_no_suppress_held()
        .with_straight_timing_local_ids(vec![2]),
    );

    chunks.push(ChunkMap::new(
        make_offset("voice_a", offset_lookup),
        chunk::Coords::new(10, 0),
        Shape::new(1, 8),
        config.voice_a.color.to_midi(),
        None,
        None,
        RepeatMode::None,
    ));
    chunks.push(ChunkMap::new(
        make_offset("voice_b", offset_lookup),
        chunk::Coords::new(11, 0),
        Shape::new(1, 8),
        config.voice_b.color.to_midi(),
        None,
        None,
        RepeatMode::None,
    ));
    chunks.push(ChunkMap::new(
        make_offset("voice_c", offset_lookup),
        chunk::Coords::new(12, 0),
        Shape::new(1, 8),
        config.voice_c.color.to_midi(),
        None,
        None,
        RepeatMode::None,
    ));

    chunks.push(ChunkMap::new(
        Box::new(devices::ScaleDegreeToggle::new(
            scale.clone(),
            config::ScaleDegree::Second,
            params.clone(),
        )),
        chunk::Coords::new(13, 0),
        Shape::new(1, 2),
        config::ChunkColor::Yellow.to_midi(),
        None,
        None,
        RepeatMode::None,
    ));
    chunks.push(ChunkMap::new(
        Box::new(devices::ScaleDegreeToggle::new(
            scale.clone(),
            config::ScaleDegree::Third,
            params.clone(),
        )),
        chunk::Coords::new(13, 2),
        Shape::new(1, 2),
        config::ChunkColor::Yellow.to_midi(),
        None,
        None,
        RepeatMode::None,
    ));
    chunks.push(ChunkMap::new(
        Box::new(devices::ScaleDegreeToggle::new(
            scale.clone(),
            config::ScaleDegree::Sixth,
            params.clone(),
        )),
        chunk::Coords::new(13, 4),
        Shape::new(1, 2),
        config::ChunkColor::Yellow.to_midi(),
        None,
        None,
        RepeatMode::None,
    ));
    chunks.push(ChunkMap::new(
        Box::new(devices::ScaleDegreeToggle::new(
            scale.clone(),
            config::ScaleDegree::Seventh,
            params.clone(),
        )),
        chunk::Coords::new(13, 6),
        Shape::new(1, 2),
        config::ChunkColor::Yellow.to_midi(),
        None,
        None,
        RepeatMode::None,
    ));

    chunks.push(ChunkMap::new(
        make_note_triggers(
            &config.triggers.output,
            &config.triggers.notes,
            output_ports,
        ),
        chunk::Coords::new(1, 0),
        Shape::new(1, 8),
        config.triggers.color.to_midi(),
        Some(15),
        None,
        RepeatMode::NoCycle,
    ));

    chunks.push(ChunkMap::new(
        make_note_triggers(
            &config.samples.output,
            &config.samples.notes[0..4],
            output_ports,
        ),
        chunk::Coords::new(0, 0),
        Shape::new(1, 4),
        config.samples.color.to_midi(),
        None,
        Some(vec![2, 3, 10, 11]),
        RepeatMode::NoCycle,
    ));
    chunks.push(ChunkMap::new(
        make_voice(
            &config.voice_a,
            "voice_a",
            output_ports,
            offset_lookup,
            scale,
        ),
        chunk::Coords::new(2, 0),
        Shape::new(5, 4),
        config.voice_a.color.to_midi(),
        Some(5),
        None,
        RepeatMode::Global,
    ));
    chunks.push(ChunkMap::new(
        make_voice(
            &config.voice_b,
            "voice_b",
            output_ports,
            offset_lookup,
            scale,
        ),
        chunk::Coords::new(2, 4),
        Shape::new(5, 4),
        config.voice_b.color.to_midi(),
        Some(4),
        None,
        RepeatMode::Global,
    ));
    chunks.push(ChunkMap::new(
        make_voice(
            &config.voice_c,
            "voice_c",
            output_ports,
            offset_lookup,
            scale,
        ),
        chunk::Coords::new(7, 0),
        Shape::new(3, 8),
        config.voice_c.color.to_midi(),
        None,
        None,
        RepeatMode::Global,
    ));

    chunks
}
