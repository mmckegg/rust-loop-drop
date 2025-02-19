use crate::controllers::ClockPulse;
use crate::devices::MidiTrigger;
use crate::scale::Scale;
use chunk::{Coords, RepeatMode, Shape};
use serde::{Deserialize, Serialize};
use serde_json::{json, to_writer_pretty};
use std::collections::HashMap;
use std::error::Error;
use std::fs::File;
use std::io::BufReader;

impl Config {
    pub fn read(filepath: &str) -> Result<Self, Box<dyn Error>> {
        let file = File::open(filepath)?;
        let reader = BufReader::new(file);

        let config = serde_json::from_reader(reader)?;
        Ok(config)
    }

    pub fn write(&self, filepath: &str) -> std::io::Result<()> {
        let myjson = json!(self);
        // println!("{}", myjson.to_string());
        to_writer_pretty(&File::create(filepath)?, &myjson)?;
        Ok(())
    }

    pub fn default() -> Self {
        let sp404_port_name = "TR-6S"; // drums
        let launchpad_output_name = "Launchpad Pro MK3 PORT 2";
        let rig_port_name = "Univer Inter";
        let launchpad_clock_out = "Launchpad Pro MK3";

        let mut channel_map = HashMap::new();
        channel_map.insert(8, 1);
        channel_map.insert(5, 4);
        channel_map.insert(6, 5);
        channel_map.insert(7, 6);

        Config {
            post_schedule_channels: vec![],
            chunks: vec![
                // EXT SYNTH OFFSET
                // (also sends pitch mod on channel 2 for slicer)
                ChunkConfig {
                    coords: Coords::new(3 + 8, 0),
                    shape: Shape::new(1, 8),
                    color: 12, // soft yellow
                    channel: None,
                    repeat_mode: RepeatMode::OnlyQuant,
                    device: DeviceConfig::multi(vec![DeviceConfig::offset("ext")]),
                },
                // BASS OFFSET
                ChunkConfig {
                    device: DeviceConfig::offset("bass"),
                    coords: Coords::new(4 + 8, 0),
                    shape: Shape::new(1, 8),
                    color: 43, // blue
                    channel: None,
                    repeat_mode: RepeatMode::OnlyQuant,
                },
                // SYNTH OFFSET
                ChunkConfig {
                    device: DeviceConfig::offset("keys"),
                    coords: Coords::new(5 + 8, 0),
                    shape: Shape::new(1, 8),
                    color: 55, // pink
                    channel: None,
                    repeat_mode: RepeatMode::OnlyQuant,
                },
                // ROOT NOTE SELECTOR
                ChunkConfig {
                    device: DeviceConfig::RootSelect,
                    coords: Coords::new(6 + 8, 0),
                    shape: Shape::new(2, 8),
                    color: 35, // soft green
                    channel: None,
                    repeat_mode: RepeatMode::OnlyQuant,
                },
                // SCALE MODE SELECTOR
                ChunkConfig {
                    device: DeviceConfig::ScaleDegreeToggle(ScaleDegree::Second),
                    coords: Coords::new(16, 0),
                    shape: Shape::new(1, 2),
                    color: 95, // purple
                    channel: None,
                    repeat_mode: RepeatMode::OnlyQuant,
                },
                ChunkConfig {
                    device: DeviceConfig::ScaleDegreeToggle(ScaleDegree::Third),
                    coords: Coords::new(16, 2),
                    shape: Shape::new(1, 2),
                    color: 95, // black
                    channel: None,
                    repeat_mode: RepeatMode::OnlyQuant,
                },
                ChunkConfig {
                    device: DeviceConfig::ScaleDegreeToggle(ScaleDegree::Sixth),
                    coords: Coords::new(16, 4),
                    shape: Shape::new(1, 2),
                    color: 95, // purple
                    channel: None,
                    repeat_mode: RepeatMode::OnlyQuant,
                },
                ChunkConfig {
                    device: DeviceConfig::ScaleDegreeToggle(ScaleDegree::Seventh),
                    coords: Coords::new(16, 6),
                    shape: Shape::new(1, 2),
                    color: 95, // purple
                    channel: None,
                    repeat_mode: RepeatMode::OnlyQuant,
                },
                // SP-404mk2 samples (schedule first so that samples coinciding with drum triggers don't get delayed, drums are fine because running on USB midi)
                ChunkConfig {
                    device: DeviceConfig::Sp404Mk2 {
                        port_name: String::from(sp404_port_name),
                        velocity_map: Some(vec![10, 20, 30, 40, 50, 60, 70, 70, 70, 90, 100]),
                        sidechain_output: None,
                    },
                    coords: Coords::new(2, 0),
                    shape: Shape::new(1, 8),
                    color: 15, // yellow
                    channel: Some(1),
                    repeat_mode: RepeatMode::OnlyQuant,
                },
                // TR-6S
                ChunkConfig {
                    device: DeviceConfig::multi(vec![DeviceConfig::MidiTriggers {
                        output: MidiPortConfig::new(sp404_port_name, 12),
                        velocity_map: Some(vec![40, 40, 40, 80, 80, 80, 80, 127]),
                        trigger_ids: vec![36, 38, 43, 39, 42, 46],
                        sidechain_output: Some(SidechainOutput { id: 0 }),
                    }]),
                    coords: Coords::new(0, 0),
                    shape: Shape::new(2, 3),
                    color: 8, // warm white
                    channel: Some(0),
                    repeat_mode: RepeatMode::NoCycle,
                },
                // zaps
                ChunkConfig {
                    device: DeviceConfig::multi(vec![DeviceConfig::CcTriggers {
                        velocity_map: Some(vec![10, 10, 10, 127]),
                        output: MidiPortConfig::new(rig_port_name, 13),
                        triggers: vec![
                            MidiTrigger::NoteVelocity(13, 60),
                            MidiTrigger::NoteVelocity(13, 61),
                            MidiTrigger::NoteVelocity(13, 62),
                            MidiTrigger::NoteVelocity(13, 63),
                            MidiTrigger::NoteVelocity(13, 64),
                            MidiTrigger::NoteVelocity(13, 65),
                            MidiTrigger::NoteVelocity(13, 66),
                            MidiTrigger::NoteVelocity(13, 67),
                            MidiTrigger::NoteVelocity(13, 68),
                            MidiTrigger::NoteVelocity(13, 69),
                        ],
                    }]),
                    coords: Coords::new(0, 3),
                    shape: Shape::new(2, 5),
                    color: 9, // orange
                    channel: Some(2),
                    repeat_mode: RepeatMode::Global,
                },
                // PLAITS BASS
                ChunkConfig {
                    device: DeviceConfig::MidiKeys {
                        output: MidiPortConfig::new(rig_port_name, 15),
                        velocity_map: None,
                        offset_wrap: false,
                        monophonic: true,
                        offset_id: String::from("bass"),
                        note_offset: -4,
                        octave_offset: -1,
                        midi_offset: 0,
                    },
                    coords: Coords::new(3, 0),
                    shape: Shape::new(5, 4),
                    color: 43, // blue
                    channel: Some(4),
                    repeat_mode: RepeatMode::Global,
                },
                // Telepathy
                ChunkConfig {
                    device: DeviceConfig::MidiKeys {
                        output: MidiPortConfig::new(rig_port_name, 14),
                        velocity_map: None,
                        offset_wrap: false,
                        offset_id: String::from("keys"),
                        monophonic: false,
                        note_offset: -4,
                        octave_offset: -3,
                        midi_offset: 0,
                    },
                    coords: Coords::new(3, 4),
                    shape: Shape::new(5, 4),
                    color: 59, // pink
                    channel: Some(5),
                    repeat_mode: RepeatMode::Global,
                },
                // Lemondrop
                ChunkConfig {
                    coords: Coords::new(0 + 8, 0),
                    shape: Shape::new(3, 8),
                    color: 51,
                    channel: Some(6),
                    repeat_mode: RepeatMode::Global,
                    device: DeviceConfig::multi(vec![DeviceConfig::MidiKeys {
                        offset_wrap: true,
                        output: MidiPortConfig::new(rig_port_name, 7),
                        velocity_map: None,
                        monophonic: false,
                        offset_id: String::from("ext"),
                        note_offset: -4,
                        octave_offset: -1,
                        midi_offset: 0,
                    }]),
                },
            ],
            clock_input_port_name: String::from(sp404_port_name),
            clock_output_port_names: vec![String::from(launchpad_clock_out)],
            resync_port_names: vec![String::from(launchpad_output_name)],
            keep_alive_port_names: vec![],
            controllers: vec![
                ControllerConfig::Umi3 {
                    port_name: String::from("Logidy UMI3"),
                },
                ControllerConfig::ModTwister {
                    port_name: String::from("Midi Fighter Twister"),
                    continuously_send: vec![],
                    continuously_send_rr: vec![0, 1, 2, 4, 5, 6, 7, 9, 10, 11, 12, 13],
                    channel_map,
                    modulators: vec![
                        // row 1
                        ModulatorConfig::new(rig_port_name, 2, Modulator::Cc(8, 64)), // main filter
                        ModulatorConfig::new(rig_port_name, 2, Modulator::Cc(6, 0)),  // main fx
                        ModulatorConfig::new(rig_port_name, 2, Modulator::Cc(7, 0)),  // main fx mod
                        ModulatorConfig::Swing(0), // global shuffle
                        // row 2
                        ModulatorConfig::new(rig_port_name, 2, Modulator::Cc(1, 127)), // dfam mod
                        ModulatorConfig::new(rig_port_name, 2, Modulator::Cc(4, 32)),  // bass mod
                        ModulatorConfig::new(rig_port_name, 2, Modulator::Cc(3, 64)),  // synth mod
                        ModulatorConfig::new(rig_port_name, 2, Modulator::Cc(2, 64)), // pianophonic filter
                        // row 3
                        ModulatorConfig::new(sp404_port_name, 4, Modulator::Cc(16, 127)), // sp404 filter
                        ModulatorConfig::new(rig_port_name, 15, Modulator::PitchBend(0.0)), // bass pitch
                        ModulatorConfig::new(rig_port_name, 14, Modulator::PitchBend(0.0)), // synth pitch
                        ModulatorConfig::DuckDecay(10), // duck decay
                        // row 4
                        ModulatorConfig::new(rig_port_name, 2, Modulator::Cc(9, 0)), // mod a
                        ModulatorConfig::new(rig_port_name, 2, Modulator::Cc(10, 0)), // mod b
                        ModulatorConfig::new(rig_port_name, 2, Modulator::Cc(11, 0)), // mod c
                        ModulatorConfig::DuckAmount(64),                             // duck amount
                        ////////////////////////
                        // DRUMS
                        // row 1
                        ModulatorConfig::new(sp404_port_name, 12, Modulator::Cc(23, 20)), // bd decay
                        ModulatorConfig::new(sp404_port_name, 12, Modulator::Cc(28, 10)), // sd decay
                        ModulatorConfig::new(sp404_port_name, 12, Modulator::Cc(47, 10)), // lt decay
                        ModulatorConfig::new(sp404_port_name, 12, Modulator::Cc(59, 32)), // hc decay
                        // row 2
                        ModulatorConfig::new(sp404_port_name, 12, Modulator::Cc(96, 0)), // bd ctrl
                        ModulatorConfig::new(sp404_port_name, 12, Modulator::Cc(97, 0)), // sd ctrl
                        ModulatorConfig::new(sp404_port_name, 12, Modulator::Cc(46, 64)), // lt pitch
                        ModulatorConfig::new(sp404_port_name, 12, Modulator::Cc(106, 0)), // hc ctrl
                        // row 3
                        ModulatorConfig::new(sp404_port_name, 12, Modulator::Cc(20, 64)), // bd pitch
                        ModulatorConfig::new(sp404_port_name, 12, Modulator::Cc(17, 64)), // delay time
                        ModulatorConfig::new(sp404_port_name, 12, Modulator::Cc(62, 32)), // ch decay
                        ModulatorConfig::new(sp404_port_name, 12, Modulator::Cc(81, 64)), // oh decay
                        // row 4
                        ModulatorConfig::new(sp404_port_name, 12, Modulator::Cc(91, 0)), // reverb amount
                        ModulatorConfig::new(sp404_port_name, 12, Modulator::Cc(18, 40)), // delay feedback
                        ModulatorConfig::new(sp404_port_name, 12, Modulator::Cc(107, 0)), // ch ctrl
                        ModulatorConfig::new(sp404_port_name, 12, Modulator::Cc(108, 0)), // oh ctrl
                        ////////////////////////
                        // LFO MODULATORS
                        // row 1
                        ModulatorConfig::LfoSpeed(50),
                        ModulatorConfig::LfoSkew(64),
                        ModulatorConfig::LfoHold(0),
                        ModulatorConfig::LfoOffset(64),
                        // row 2
                        ModulatorConfig::LfoAmount(4, 64),
                        ModulatorConfig::LfoAmount(5, 64),
                        ModulatorConfig::LfoAmount(6, 64),
                        ModulatorConfig::LfoAmount(7, 64),
                        // row 3
                        ModulatorConfig::LfoAmount(8, 64),
                        ModulatorConfig::LfoAmount(9, 64),
                        ModulatorConfig::LfoAmount(10, 64),
                        ModulatorConfig::LfoAmount(11, 64),
                        // row 4
                        ModulatorConfig::LfoAmount(12, 64),
                        ModulatorConfig::LfoAmount(13, 64),
                        ModulatorConfig::LfoAmount(14, 64),
                        ModulatorConfig::LfoAmount(15, 64),
                    ],
                },
                ControllerConfig::DuckOutput {
                    modulators: vec![ModulatorConfig::new(
                        rig_port_name,
                        2,
                        Modulator::InvertMaxCc(5, 127, 0),
                    )],
                },
                ControllerConfig::ClockPulse {
                    output: MidiPortConfig::new(rig_port_name, 12),
                    divider: 12,
                }, // ControllerConfig::LaunchpadTempo {
                   //     daw_port_name: String::from("Launchpad Pro MK3 PORT 3"),
                   // },
            ],
        }
    }
    pub fn minimal() -> Self {
        let polyend_synth_port = "Synth";
        let launchpad_output_name = "Launchpad Pro MK3 PORT 2";
        let sp404_port_name = launchpad_output_name;
        let rig_port_name = launchpad_output_name;

        let mut channel_map = HashMap::new();
        channel_map.insert(4, 2);
        channel_map.insert(5, 4);
        channel_map.insert(6, 5);
        channel_map.insert(7, 6);
        channel_map.insert(8, 1);

        Config {
            post_schedule_channels: vec![2, 3],
            chunks: vec![
                // EXT SYNTH OFFSET
                // (also sends pitch mod on channel 2 for slicer)
                ChunkConfig {
                    coords: Coords::new(3 + 8, 0),
                    shape: Shape::new(1, 8),
                    color: 12, // soft yellow
                    channel: None,
                    repeat_mode: RepeatMode::OnlyQuant,
                    device: DeviceConfig::multi(vec![DeviceConfig::offset("ext")]),
                },
                // BASS OFFSET
                ChunkConfig {
                    device: DeviceConfig::offset("bass"),
                    coords: Coords::new(4 + 8, 0),
                    shape: Shape::new(1, 8),
                    color: 43, // blue
                    channel: None,
                    repeat_mode: RepeatMode::OnlyQuant,
                },
                // SYNTH OFFSET
                ChunkConfig {
                    device: DeviceConfig::offset("keys"),
                    coords: Coords::new(5 + 8, 0),
                    shape: Shape::new(1, 8),
                    color: 55, // pink
                    channel: None,
                    repeat_mode: RepeatMode::OnlyQuant,
                },
                // ROOT NOTE SELECTOR
                ChunkConfig {
                    device: DeviceConfig::RootSelect,
                    coords: Coords::new(6 + 8, 0),
                    shape: Shape::new(2, 8),
                    color: 35, // soft green
                    channel: None,
                    repeat_mode: RepeatMode::OnlyQuant,
                },
                // SCALE MODE SELECTOR
                ChunkConfig {
                    device: DeviceConfig::ScaleDegreeToggle(ScaleDegree::Second),
                    coords: Coords::new(16, 0),
                    shape: Shape::new(1, 2),
                    color: 95, // purple
                    channel: None,
                    repeat_mode: RepeatMode::OnlyQuant,
                },
                ChunkConfig {
                    device: DeviceConfig::ScaleDegreeToggle(ScaleDegree::Third),
                    coords: Coords::new(16, 2),
                    shape: Shape::new(1, 2),
                    color: 95, // black
                    channel: None,
                    repeat_mode: RepeatMode::OnlyQuant,
                },
                ChunkConfig {
                    device: DeviceConfig::ScaleDegreeToggle(ScaleDegree::Sixth),
                    coords: Coords::new(16, 4),
                    shape: Shape::new(1, 2),
                    color: 95, // purple
                    channel: None,
                    repeat_mode: RepeatMode::OnlyQuant,
                },
                ChunkConfig {
                    device: DeviceConfig::ScaleDegreeToggle(ScaleDegree::Seventh),
                    coords: Coords::new(16, 6),
                    shape: Shape::new(1, 2),
                    color: 95, // purple
                    channel: None,
                    repeat_mode: RepeatMode::OnlyQuant,
                },
                // SP-404mk2 samples (schedule first so that samples coinciding with drum triggers don't get delayed, drums are fine because running on USB midi)
                ChunkConfig {
                    device: DeviceConfig::Sp404Mk2 {
                        port_name: String::from(sp404_port_name),
                        velocity_map: Some(vec![10, 20, 30, 40, 50, 60, 70, 70, 70, 90, 100]),
                        sidechain_output: None,
                    },
                    coords: Coords::new(0, 4),
                    shape: Shape::new(2, 4),
                    color: 9, // orange
                    channel: Some(1),
                    repeat_mode: RepeatMode::OnlyQuant,
                },
                // CRUST
                ChunkConfig {
                    device: DeviceConfig::CcTriggers {
                        velocity_map: Some(vec![20, 20, 20, 40, 40, 40, 40, 127]),
                        output: MidiPortConfig::new(rig_port_name, 12),
                        triggers: vec![
                            MidiTrigger::NoteVelocity(12, 30),
                            MidiTrigger::NoteVelocity(12, 42),
                            MidiTrigger::NoteVelocity(12, 54),
                            MidiTrigger::NoteVelocity(12, 66),
                        ],
                    },
                    coords: Coords::new(0, 0),
                    shape: Shape::new(1, 4),
                    color: 8, // warm white
                    channel: Some(2),
                    repeat_mode: RepeatMode::Global,
                },
                // BIA
                ChunkConfig {
                    device: DeviceConfig::CcTriggers {
                        velocity_map: Some(vec![20, 20, 20, 40, 40, 40, 40, 127]),
                        output: MidiPortConfig::new(rig_port_name, 13),
                        triggers: vec![
                            MidiTrigger::NoteVelocity(13, 30),
                            MidiTrigger::NoteVelocity(13, 42),
                            MidiTrigger::NoteVelocity(13, 54),
                            MidiTrigger::NoteVelocity(13, 66),
                        ],
                    },
                    coords: Coords::new(1, 0),
                    shape: Shape::new(1, 4),
                    color: 15, // yellow
                    channel: Some(3),
                    repeat_mode: RepeatMode::Global,
                },
                // SYNTH 1
                ChunkConfig {
                    device: DeviceConfig::MidiKeys {
                        output: MidiPortConfig::new(polyend_synth_port, 1),
                        velocity_map: None,
                        offset_wrap: false,
                        monophonic: true,
                        offset_id: String::from("bass"),
                        note_offset: -4,
                        octave_offset: -1,
                        midi_offset: 0,
                    },
                    coords: Coords::new(2, 0),
                    shape: Shape::new(6, 4),
                    color: 43, // blue
                    channel: Some(4),
                    repeat_mode: RepeatMode::Global,
                },
                // SYNTH 2
                ChunkConfig {
                    device: DeviceConfig::MidiKeys {
                        output: MidiPortConfig::new(polyend_synth_port, 2),
                        velocity_map: None,
                        offset_wrap: false,
                        offset_id: String::from("keys"),
                        monophonic: false,
                        note_offset: -4,
                        octave_offset: -1,
                        midi_offset: 0,
                    },
                    coords: Coords::new(2, 4),
                    shape: Shape::new(6, 4),
                    color: 59, // pink
                    channel: Some(5),
                    repeat_mode: RepeatMode::Global,
                },
                // SYNTH 3
                ChunkConfig {
                    coords: Coords::new(0 + 8, 0),
                    shape: Shape::new(3, 8),
                    color: 51,
                    channel: Some(6),
                    repeat_mode: RepeatMode::Global,
                    device: DeviceConfig::multi(vec![DeviceConfig::MidiKeys {
                        offset_wrap: true,
                        output: MidiPortConfig::new(polyend_synth_port, 3),
                        velocity_map: None,
                        monophonic: false,
                        offset_id: String::from("ext"),
                        note_offset: -4,
                        octave_offset: -1,
                        midi_offset: 0,
                    }]),
                },
            ],
            clock_input_port_name: String::from("Launchpad Pro MK3"),
            clock_output_port_names: vec![String::from(polyend_synth_port)],
            resync_port_names: vec![String::from(polyend_synth_port)],
            keep_alive_port_names: vec![],
            controllers: vec![
                ControllerConfig::Umi3 {
                    port_name: String::from("Logidy UMI3"),
                },
                ControllerConfig::ModTwister {
                    port_name: String::from("Midi Fighter Twister"),
                    continuously_send: vec![],
                    continuously_send_rr: vec![],
                    channel_map,
                    modulators: vec![
                        // row 1
                        ModulatorConfig::new(sp404_port_name, 3, Modulator::Cc(16, 0)), // sp404 filter
                        ModulatorConfig::new(sp404_port_name, 4, Modulator::Cc(18, 0)), // sp404 reverb amount
                        ModulatorConfig::new(sp404_port_name, 4, Modulator::Cc(17, 0)), // sp404 reverb time
                        ModulatorConfig::Swing(0), // global shuffle
                        // row 2
                        ModulatorConfig::new(sp404_port_name, 2, Modulator::Cc(16, 127)), // 404 bank b
                        ModulatorConfig::new(polyend_synth_port, 1, Modulator::Cc(4, 74)), // blue synth
                        ModulatorConfig::new(polyend_synth_port, 2, Modulator::Cc(3, 74)), // gold synth
                        ModulatorConfig::new(polyend_synth_port, 3, Modulator::Cc(2, 64)), // purple synth
                        // row 3
                        ModulatorConfig::new(sp404_port_name, 1, Modulator::Cc(16, 127)), // 404 bank a
                        ModulatorConfig::new(polyend_synth_port, 1, Modulator::PitchBend(0.0)), // blue pitch
                        ModulatorConfig::new(polyend_synth_port, 2, Modulator::PitchBend(0.0)), // gold pitch
                        ModulatorConfig::new(polyend_synth_port, 3, Modulator::PitchBend(0.0)), // purple pitch
                        // row 4
                        ModulatorConfig::new(sp404_port_name, 1, Modulator::Cc(17, 0)), // 404 bank a
                        ModulatorConfig::new(rig_port_name, 14, Modulator::Cc(9, 0)),   // mod a
                        ModulatorConfig::new(rig_port_name, 14, Modulator::Cc(10, 0)),  // mod b
                        ModulatorConfig::new(rig_port_name, 14, Modulator::Cc(11, 0)),  // mod c
                        ////////////////////////
                        // CRUST
                        // row 1
                        ModulatorConfig::step((0, 2), sp404_port_name, 12, Modulator::Cc(1, 0)),
                        ModulatorConfig::step((1, 2), rig_port_name, 12, Modulator::Cc(1, 0)),
                        ModulatorConfig::step((2, 2), rig_port_name, 12, Modulator::Cc(1, 0)),
                        ModulatorConfig::step((3, 2), rig_port_name, 12, Modulator::Cc(1, 0)),
                        // row 2
                        ModulatorConfig::step((0, 2), rig_port_name, 12, Modulator::Cc(2, 0)),
                        ModulatorConfig::step((1, 2), rig_port_name, 12, Modulator::Cc(2, 0)),
                        ModulatorConfig::step((2, 2), rig_port_name, 12, Modulator::Cc(2, 0)),
                        ModulatorConfig::step((3, 2), rig_port_name, 12, Modulator::Cc(2, 0)),
                        // row 3
                        ModulatorConfig::step((0, 2), rig_port_name, 12, Modulator::Cc(3, 0)),
                        ModulatorConfig::step((1, 2), rig_port_name, 12, Modulator::Cc(3, 0)),
                        ModulatorConfig::step((2, 2), rig_port_name, 12, Modulator::Cc(3, 0)),
                        ModulatorConfig::step((3, 2), rig_port_name, 12, Modulator::Cc(3, 0)),
                        // row 4
                        ModulatorConfig::step((0, 2), rig_port_name, 12, Modulator::Cc(4, 0)),
                        ModulatorConfig::step((1, 2), rig_port_name, 12, Modulator::Cc(4, 0)),
                        ModulatorConfig::step((2, 2), rig_port_name, 12, Modulator::Cc(4, 0)),
                        ModulatorConfig::step((3, 2), rig_port_name, 12, Modulator::Cc(4, 0)),
                        ////////////////////////
                        // BIA
                        // row 1
                        ModulatorConfig::step((0, 3), rig_port_name, 13, Modulator::Cc(1, 0)),
                        ModulatorConfig::step((1, 3), rig_port_name, 13, Modulator::Cc(1, 0)),
                        ModulatorConfig::step((2, 3), rig_port_name, 13, Modulator::Cc(1, 0)),
                        ModulatorConfig::step((3, 3), rig_port_name, 13, Modulator::Cc(1, 0)),
                        // row 2
                        ModulatorConfig::step((0, 3), rig_port_name, 13, Modulator::Cc(2, 0)),
                        ModulatorConfig::step((1, 3), rig_port_name, 13, Modulator::Cc(2, 0)),
                        ModulatorConfig::step((2, 3), rig_port_name, 13, Modulator::Cc(2, 0)),
                        ModulatorConfig::step((3, 3), rig_port_name, 13, Modulator::Cc(2, 0)),
                        // row 3
                        ModulatorConfig::step((0, 3), rig_port_name, 13, Modulator::Cc(3, 0)),
                        ModulatorConfig::step((1, 3), rig_port_name, 13, Modulator::Cc(3, 0)),
                        ModulatorConfig::step((2, 3), rig_port_name, 13, Modulator::Cc(3, 0)),
                        ModulatorConfig::step((3, 3), rig_port_name, 13, Modulator::Cc(3, 0)),
                        // row 4
                        ModulatorConfig::step((0, 3), rig_port_name, 13, Modulator::Cc(4, 0)),
                        ModulatorConfig::step((1, 3), rig_port_name, 13, Modulator::Cc(4, 0)),
                        ModulatorConfig::step((2, 3), rig_port_name, 13, Modulator::Cc(4, 0)),
                        ModulatorConfig::step((3, 3), rig_port_name, 13, Modulator::Cc(4, 0)),
                        ////////////////////////
                        // LFO MODULATORS
                        // row 1
                        ModulatorConfig::LfoSpeed(50),
                        ModulatorConfig::LfoSkew(64),
                        ModulatorConfig::LfoHold(0),
                        ModulatorConfig::LfoOffset(64),
                        // row 2
                        ModulatorConfig::LfoAmount(4, 64),
                        ModulatorConfig::LfoAmount(5, 64),
                        ModulatorConfig::LfoAmount(6, 64),
                        ModulatorConfig::LfoAmount(7, 64),
                        // row 3
                        ModulatorConfig::LfoAmount(8, 64),
                        ModulatorConfig::LfoAmount(9, 64),
                        ModulatorConfig::LfoAmount(10, 64),
                        ModulatorConfig::LfoAmount(11, 64),
                        // row 4
                        ModulatorConfig::LfoAmount(12, 64),
                        ModulatorConfig::LfoAmount(13, 64),
                        ModulatorConfig::LfoAmount(14, 64),
                        ModulatorConfig::LfoAmount(15, 64),
                    ],
                },
                ControllerConfig::LaunchpadTempo {
                    daw_port_name: String::from("Launchpad Pro MK3 PORT 3"),
                },
            ],
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct Config {
    pub chunks: Vec<ChunkConfig>,
    pub post_schedule_channels: Vec<u32>,
    pub clock_input_port_name: String,
    pub clock_output_port_names: Vec<String>,
    pub keep_alive_port_names: Vec<String>,
    pub resync_port_names: Vec<String>,
    pub controllers: Vec<ControllerConfig>,
}

#[derive(Serialize, Deserialize)]
pub struct ChunkConfig {
    pub coords: Coords,
    pub shape: Shape,
    pub color: u8,
    pub channel: Option<u32>,
    pub repeat_mode: RepeatMode,
    pub device: DeviceConfig,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct MidiPortConfig {
    pub name: String,
    pub channel: u8,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct SidechainOutput {
    pub id: u32,
}

impl MidiPortConfig {
    pub fn new(name: &str, channel: u8) -> Self {
        MidiPortConfig {
            name: String::from(name),
            channel,
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub enum DeviceConfig {
    Multi {
        devices: Vec<DeviceConfig>,
    },
    MidiKeys {
        output: MidiPortConfig,
        offset_id: String,
        offset_wrap: bool,
        note_offset: i32,
        midi_offset: i8,
        velocity_map: Option<Vec<u8>>,
        octave_offset: i32,
        monophonic: bool,
    },
    OffsetChunk {
        id: String,
    },
    PitchOffsetChunk {
        output: MidiPortConfig,
    },
    RootSelect,
    ScaleDegreeToggle(ScaleDegree),
    MidiTriggers {
        output: MidiPortConfig,
        trigger_ids: Vec<u8>,
        velocity_map: Option<Vec<u8>>,
        sidechain_output: Option<SidechainOutput>,
    },
    CcTriggers {
        output: MidiPortConfig,
        velocity_map: Option<Vec<u8>>,
        triggers: Vec<MidiTrigger>,
    },
    Sp404Mk2 {
        port_name: String,
        velocity_map: Option<Vec<u8>>,
        sidechain_output: Option<SidechainOutput>,
    },
}

impl DeviceConfig {
    pub fn offset(id: &str) -> Self {
        DeviceConfig::OffsetChunk {
            id: String::from(id),
        }
    }

    pub fn multi(devices: Vec<DeviceConfig>) -> Self {
        DeviceConfig::Multi { devices }
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub enum ControllerConfig {
    Twister {
        port_name: String,
        mixer_port: MidiPortConfig,
        modulators: Vec<ModulatorConfig>,
    },
    ModTwister {
        port_name: String,
        continuously_send: Vec<usize>,
        continuously_send_rr: Vec<usize>,
        modulators: Vec<ModulatorConfig>,
        channel_map: HashMap<usize, u32>,
    },
    Umi3 {
        port_name: String,
    },
    ClockPulse {
        output: MidiPortConfig,
        divider: i32,
    },
    LaunchpadTempo {
        daw_port_name: String,
    },
    Init {
        modulators: Vec<ModulatorConfig>,
    },
    DuckOutput {
        modulators: Vec<ModulatorConfig>,
    },
}

#[derive(Serialize, Deserialize, Clone)]
pub enum ModulatorConfig {
    None,
    Midi {
        port: MidiPortConfig,
        rx_port: Option<MidiPortConfig>,
        modulator: Modulator,
        step_channel: Option<(u32, u32)>,
    },
    LfoAmount(usize, u8),
    LfoSpeed(u8),
    LfoHold(u8),
    LfoOffset(u8),
    LfoSkew(u8),
    DuckDecay(u8),
    DuckAmount(u8),
    Swing(u8),
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum Modulator {
    Cc(u8, u8),
    InvertCc(u8, u8),
    InvertMaxCc(u8, u8, u8),
    TriggerWhen(TriggerCondition, (u8, u8)),
    // id, max, default
    MaxCc(u8, u8, u8),
    PolarCcSwitch {
        cc_low: Option<u8>,
        cc_high: Option<u8>,
        cc_switch: Option<u8>,
        default: u8,
    },
    PitchBend(f64),
    Aftertouch(u8),
    PositivePitchBend(f64),
    Multi(Vec<Modulator>),
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum TriggerCondition {
    Gt(u8),
    Lt(u8),
}

impl TriggerCondition {
    pub fn check(&self, value: u8) -> bool {
        match self {
            TriggerCondition::Gt(v) => &value > v,
            TriggerCondition::Lt(v) => &value < v,
        }
    }
}

impl Modulator {
    pub fn all(&self) -> Vec<Modulator> {
        match self {
            Modulator::Multi(values) => values.clone(),
            _ => vec![self.clone()],
        }
    }
}

impl ModulatorConfig {
    pub fn new(port_name: &str, port_number: u8, modulator: Modulator) -> ModulatorConfig {
        ModulatorConfig::Midi {
            port: MidiPortConfig::new(port_name, port_number),
            rx_port: None,
            modulator,
            step_channel: None,
        }
    }
    pub fn step(
        step_channel: (u32, u32),
        port_name: &str,
        port_number: u8,
        modulator: Modulator,
    ) -> ModulatorConfig {
        ModulatorConfig::Midi {
            port: MidiPortConfig::new(port_name, port_number),
            rx_port: None,
            modulator,
            step_channel: Some(step_channel),
        }
    }
    pub fn rx(port_name: &str, port_number: u8, modulator: Modulator) -> ModulatorConfig {
        ModulatorConfig::Midi {
            port: MidiPortConfig::new(port_name, port_number),
            rx_port: Some(MidiPortConfig::new(port_name, port_number)),
            modulator,
            step_channel: None,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq)]
pub enum ScaleDegree {
    Second,
    Third,
    Fourth,
    Fifth,
    Sixth,
    Seventh,
}

#[derive(Serialize, Deserialize, Clone, Copy, Eq, PartialEq)]
pub enum Quality {
    Major = 0,
    Minor = -1,
}

#[derive(Serialize, Deserialize, Clone, Copy, Eq, PartialEq)]
pub enum PerfectQuality {
    Diminished = -1,
    Perfect = 0,
    Augmented = 1,
}
