use crate::controllers::ClockPulse;
use crate::devices::MidiTrigger;
use crate::scale::Scale;
use chunk::{Coords, RepeatMode, Shape};
use serde::{Deserialize, Serialize};
use serde_json::{json, to_writer_pretty};
use std::collections::HashMap;
use std::error::Error;
use std::fs::File;
use std::hash::Hash;
use std::io::BufReader;
use std::iter::FromIterator;

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
        let launchpad_output_name = "Launchpad Pro MK3 PORT 2";
        let rig_port_name = "Univer Inter";
        let launchpad_clock_out = "Launchpad Pro MK3";

        let mut channel_map: HashMap<usize, u32> = HashMap::new();

        // C1
        channel_map.insert(4, 2);
        channel_map.insert(8, 2);
        channel_map.insert(12, 2);
        // C2
        channel_map.insert(5, 3);
        channel_map.insert(9, 3);
        channel_map.insert(13, 3);
        // T3
        channel_map.insert(6, 10);
        channel_map.insert(10, 10);
        channel_map.insert(14, 10);
        // T4
        channel_map.insert(7, 11);
        channel_map.insert(11, 11);
        channel_map.insert(15, 11);
        // T5
        channel_map.insert(16 + 0, 12);
        channel_map.insert(16 + 4, 12);
        channel_map.insert(16 + 8, 12);
        channel_map.insert(16 + 12, 12);
        // T6
        channel_map.insert(16 + 1, 13);
        channel_map.insert(16 + 5, 13);
        channel_map.insert(16 + 9, 13);
        channel_map.insert(16 + 13, 13);
        // T7
        channel_map.insert(16 + 2, 14);
        channel_map.insert(16 + 6, 14);
        channel_map.insert(16 + 10, 14);
        channel_map.insert(16 + 14, 14);
        // T8
        channel_map.insert(16 + 11, 15);
        channel_map.insert(16 + 15, 15);
        // BASS pitch
        channel_map.insert(16 + 3, 4);
        // SYNTH pitch
        channel_map.insert(16 + 7, 5);

        for i in 0..4 {
            channel_map.insert(16 * 3 + i, 16 + i as u32);
            channel_map.insert(16 * 3 + i + 4, 16 + i as u32);
        }

        for i in 0..4 {
            channel_map.insert(16 * 3 + i + 8, 20 + i as u32);
            channel_map.insert(16 * 3 + i + 4 + 8, 20 + i as u32);
        }

        Config {
            chunks: vec![
                // Triggers B (schedule early to better handle clip start sync on bitbox)
                ChunkConfig {
                    device: DeviceConfig::multi(vec![DeviceConfig::CcTriggers {
                        velocity_map: Some(vec![80, 80, 100, 100, 100, 127]),
                        output: MidiPortConfig::new(rig_port_name, 10),
                        triggers: vec![
                            MidiTrigger::NoteVelocity(10, 36),
                            MidiTrigger::NoteVelocity(10, 37),
                            MidiTrigger::NoteVelocity(10, 38),
                            MidiTrigger::NoteVelocity(10, 39),
                        ],
                    }]),
                    coords: Coords::new(0, 4),
                    shape: Shape::new(1, 4),
                    color: 15, // yellow
                    channel: None,
                    trigger_channels: Some(vec![12, 13, 14, 15]),
                    repeat_mode: RepeatMode::NoCycle,
                },
                // EXT SYNTH OFFSET
                // (also sends pitch mod on channel 2 for slicer)
                ChunkConfig {
                    coords: Coords::new(3 + 8, 0),
                    shape: Shape::new(1, 8),
                    color: 12,
                    channel: None,
                    trigger_channels: None,
                    repeat_mode: RepeatMode::OnlyQuant,
                    device: DeviceConfig::multi(vec![DeviceConfig::offset("poly")]),
                },
                // INCUS OFFSET
                ChunkConfig {
                    device: DeviceConfig::offset("bass"),
                    coords: Coords::new(4 + 8, 0),
                    shape: Shape::new(1, 8),
                    color: 55, // pink
                    channel: None,
                    trigger_channels: None,
                    repeat_mode: RepeatMode::OnlyQuant,
                },
                // SYNTH OFFSET
                ChunkConfig {
                    device: DeviceConfig::offset("lead"),
                    coords: Coords::new(5 + 8, 0),
                    shape: Shape::new(1, 8),
                    color: 43, // blue
                    channel: None,
                    trigger_channels: None,
                    repeat_mode: RepeatMode::OnlyQuant,
                },
                // ROOT NOTE SELECTOR
                ChunkConfig {
                    device: DeviceConfig::RootSelect,
                    coords: Coords::new(6 + 8, 0),
                    shape: Shape::new(2, 8),
                    color: 35, // soft green
                    channel: None,
                    trigger_channels: None,
                    repeat_mode: RepeatMode::OnlyQuant,
                },
                // SCALE MODE SELECTOR
                ChunkConfig {
                    device: DeviceConfig::ScaleDegreeToggle(ScaleDegree::Second),
                    coords: Coords::new(16, 0),
                    shape: Shape::new(1, 2),
                    color: 95, // purple
                    channel: None,
                    trigger_channels: None,
                    repeat_mode: RepeatMode::OnlyQuant,
                },
                ChunkConfig {
                    device: DeviceConfig::ScaleDegreeToggle(ScaleDegree::Third),
                    coords: Coords::new(16, 2),
                    shape: Shape::new(1, 2),
                    color: 95, // black
                    channel: None,
                    trigger_channels: None,
                    repeat_mode: RepeatMode::OnlyQuant,
                },
                ChunkConfig {
                    device: DeviceConfig::ScaleDegreeToggle(ScaleDegree::Sixth),
                    coords: Coords::new(16, 4),
                    shape: Shape::new(1, 2),
                    color: 95, // purple
                    channel: None,
                    trigger_channels: None,
                    repeat_mode: RepeatMode::OnlyQuant,
                },
                ChunkConfig {
                    device: DeviceConfig::ScaleDegreeToggle(ScaleDegree::Seventh),
                    coords: Coords::new(16, 6),
                    shape: Shape::new(1, 2),
                    color: 95, // purple
                    channel: None,
                    trigger_channels: None,
                    repeat_mode: RepeatMode::OnlyQuant,
                },
                // Slicer
                ChunkConfig {
                    device: DeviceConfig::multi(vec![DeviceConfig::CcTriggers {
                        velocity_map: Some(vec![80, 80, 100, 100, 100, 127]),
                        output: MidiPortConfig::new(rig_port_name, 3),
                        triggers: vec![
                            MidiTrigger::NoteVelocity(3, 37),
                            MidiTrigger::NoteVelocity(3, 38),
                            MidiTrigger::NoteVelocity(3, 39),
                            MidiTrigger::NoteVelocity(3, 40),
                            MidiTrigger::NoteVelocity(3, 41),
                            MidiTrigger::NoteVelocity(3, 42),
                            MidiTrigger::NoteVelocity(3, 43),
                            MidiTrigger::NoteVelocity(3, 44),
                            MidiTrigger::NoteVelocity(3, 45),
                            MidiTrigger::NoteVelocity(3, 46),
                            MidiTrigger::NoteVelocity(3, 47),
                            MidiTrigger::NoteVelocity(3, 48),
                            MidiTrigger::NoteVelocity(3, 49),
                            MidiTrigger::NoteVelocity(3, 50),
                            MidiTrigger::NoteVelocity(3, 51),
                            MidiTrigger::NoteVelocity(3, 127),
                        ],
                    }]),
                    coords: Coords::new(1, 0),
                    shape: Shape::new(2, 8),
                    color: 10, // orange
                    channel: Some(15),
                    trigger_channels: None,
                    repeat_mode: RepeatMode::NoCycle,
                },
                // Triggers A
                ChunkConfig {
                    device: DeviceConfig::multi(vec![DeviceConfig::CcTriggers {
                        velocity_map: Some(vec![80, 80, 100, 100, 100, 127]),
                        output: MidiPortConfig::new(rig_port_name, 2),
                        triggers: vec![
                            MidiTrigger::NoteVelocity(10, 40),
                            MidiTrigger::NoteVelocity(10, 41),
                            MidiTrigger::NoteVelocity(10, 42),
                            MidiTrigger::NoteVelocity(10, 43),
                        ],
                    }]),
                    coords: Coords::new(0, 0),
                    shape: Shape::new(1, 4),
                    color: 8, // warm white
                    channel: None,
                    trigger_channels: Some(vec![2, 3, 10, 11]),
                    repeat_mode: RepeatMode::NoCycle,
                },
                // Telepathy
                ChunkConfig {
                    device: DeviceConfig::MidiKeys {
                        output: MidiPortConfig::new(rig_port_name, 15),
                        velocity_map: None,
                        offset_wrap: false,
                        monophonic: true,
                        offset_id: String::from("lead"),
                        note_offset: -4,
                        octave_offset: -1,
                        midi_offset: 0,
                    },
                    coords: Coords::new(3, 0),

                    shape: Shape::new(5, 4),
                    color: 43, //
                    channel: Some(5),
                    trigger_channels: None,
                    repeat_mode: RepeatMode::Global,
                },
                // Plaits
                ChunkConfig {
                    device: DeviceConfig::MidiKeys {
                        output: MidiPortConfig::new(rig_port_name, 14),
                        velocity_map: None,
                        offset_wrap: false,
                        offset_id: String::from("bass"),
                        monophonic: true,
                        note_offset: -4,
                        octave_offset: -2,
                        midi_offset: 0,
                    },
                    coords: Coords::new(3, 4),

                    shape: Shape::new(5, 4),
                    color: 59, // pink
                    channel: Some(4),
                    trigger_channels: None,
                    repeat_mode: RepeatMode::Global,
                },
                // Poly Synth
                ChunkConfig {
                    coords: Coords::new(0 + 8, 0),
                    shape: Shape::new(3, 8),
                    color: 51,
                    channel: None,
                    trigger_channels: None,
                    repeat_mode: RepeatMode::Global,
                    device: DeviceConfig::multi(vec![DeviceConfig::MidiKeys {
                        offset_wrap: true,
                        output: MidiPortConfig::new(rig_port_name, 7),
                        velocity_map: Some(vec![100, 100, 100, 100, 100, 100, 127]),
                        monophonic: false,
                        offset_id: String::from("poly"),
                        note_offset: -4,
                        octave_offset: -1,
                        midi_offset: 0,
                    }]),
                },
            ],
            straight_trigger_ids: vec![Coords::id_from(0, 6)],
            clock_input_port_name: String::from("Launchpad Pro MK3"),
            clock_output_port_names: vec![rig_port_name.to_string()],
            resync_port_names: vec![rig_port_name.to_string()],
            keep_alive_port_names: vec![],
            controllers: vec![
                ControllerConfig::Umi3 {
                    port_name: String::from("Logidy UMI3"),
                },
                ControllerConfig::ModTwister {
                    port_name: String::from("Midi Fighter Twister"),
                    continuously_send: vec![],
                    continuously_send_rr: Vec::from_iter(16..32),
                    channel_map,
                    modulators: vec![
                        // row 1
                        ModulatorConfig::Swing(0), // global shuffle
                        ModulatorConfig::new(rig_port_name, 10, Modulator::Cc(10, 63)), // Delay Time
                        ModulatorConfig::new(rig_port_name, 2, Modulator::Cc(9, 0)),    // mod a
                        ModulatorConfig::new(rig_port_name, 2, Modulator::Cc(10, 0)),   // mod b
                        // row 2
                        ModulatorConfig::new(rig_port_name, 10, Modulator::Cc(31, 63)), // T1 Decay
                        ModulatorConfig::new(rig_port_name, 10, Modulator::Cc(32, 63)), // T2 Decay
                        ModulatorConfig::new(rig_port_name, 10, Modulator::Cc(33, 63)), // T3 Decay
                        ModulatorConfig::new(rig_port_name, 10, Modulator::Cc(34, 63)), // T4 Decay
                        // row 3
                        ModulatorConfig::new(rig_port_name, 10, Modulator::Cc(21, 63)), // T1 Pitch
                        ModulatorConfig::new(rig_port_name, 10, Modulator::Cc(22, 63)), // T2 Pitch
                        ModulatorConfig::new(rig_port_name, 10, Modulator::Cc(23, 63)), // T3 Pitch
                        ModulatorConfig::new(rig_port_name, 10, Modulator::Cc(24, 63)), // T4 Pitch
                        // row 4
                        ModulatorConfig::new(rig_port_name, 10, Modulator::Cc(11, 100)), // T1 Volume
                        ModulatorConfig::new(rig_port_name, 10, Modulator::Cc(12, 100)), // T2 Volume
                        ModulatorConfig::new(rig_port_name, 10, Modulator::Cc(13, 100)), // T3 Volume
                        ModulatorConfig::new(rig_port_name, 10, Modulator::Cc(14, 100)), // T4 Volume
                        ////////////////////////
                        // row 1
                        ModulatorConfig::new(rig_port_name, 10, Modulator::Cc(45, 63)), // T5 Stretch
                        ModulatorConfig::new(rig_port_name, 10, Modulator::Cc(46, 63)), // T6 Stretch
                        ModulatorConfig::new(rig_port_name, 15, Modulator::PitchBend(0.0)), // bass pitch
                        ModulatorConfig::new(rig_port_name, 14, Modulator::PitchBend(0.0)), // lead pitch
                        // ModulatorConfig::new(rig_port_name, 10, Modulator::Cc(47, 63)), // T7 Stretch

                        // row 2
                        ModulatorConfig::new(rig_port_name, 10, Modulator::Cc(35, 0)), // T5 Start
                        ModulatorConfig::new(rig_port_name, 10, Modulator::Cc(36, 0)), // T6 Start
                        ModulatorConfig::new(rig_port_name, 10, Modulator::Cc(37, 64)), // T7 Filter
                        ModulatorConfig::new(rig_port_name, 10, Modulator::Cc(38, 64)), // T8 Filter
                        // row 3
                        ModulatorConfig::new(rig_port_name, 10, Modulator::Cc(25, 63)), // T5 Pitch
                        ModulatorConfig::new(rig_port_name, 10, Modulator::Cc(26, 63)), // T6 Pitch
                        ModulatorConfig::new(rig_port_name, 10, Modulator::Cc(27, 63)), // T7 Pitch
                        ModulatorConfig::new(rig_port_name, 10, Modulator::Cc(28, 63)), // T8 Pitch
                        // row 4
                        ModulatorConfig::new(rig_port_name, 10, Modulator::Cc(15, 100)), // T5 Volume
                        ModulatorConfig::new(rig_port_name, 10, Modulator::Cc(16, 100)), // T6 Volume
                        ModulatorConfig::new(rig_port_name, 10, Modulator::Cc(17, 100)), // T7 Volume
                        ModulatorConfig::new(rig_port_name, 10, Modulator::Cc(18, 100)), // T8 Volume
                        ////////////////////////
                        // L6 MIXER
                        // row 1
                        ModulatorConfig::new(rig_port_name, 11, Modulator::Cc(43, 0)), // Send A1
                        ModulatorConfig::new(rig_port_name, 11, Modulator::Cc(44, 0)), // Send A2
                        ModulatorConfig::new(rig_port_name, 11, Modulator::Cc(45, 0)), // Send A3
                        ModulatorConfig::new(rig_port_name, 11, Modulator::Cc(46, 0)), // Send A4
                        // row 2
                        ModulatorConfig::new(rig_port_name, 11, Modulator::Cc(53, 0)), // Send B1
                        ModulatorConfig::new(rig_port_name, 11, Modulator::Cc(54, 0)), // Send B2
                        ModulatorConfig::new(rig_port_name, 11, Modulator::Cc(55, 0)), // Send B3
                        ModulatorConfig::new(rig_port_name, 11, Modulator::Cc(56, 0)), // Send B4
                        // row 3
                        ModulatorConfig::new(rig_port_name, 11, Modulator::Cc(73, 64)), // Pan 1
                        ModulatorConfig::new(rig_port_name, 11, Modulator::Cc(74, 64)), // Pan 2
                        ModulatorConfig::new(rig_port_name, 11, Modulator::Cc(75, 64)), // Pan 3
                        ModulatorConfig::new(rig_port_name, 11, Modulator::Cc(76, 64)), // Pan 4
                        // row 4
                        ModulatorConfig::new(rig_port_name, 11, Modulator::Cc(12, 0)), // Reverb Time
                        ModulatorConfig::new(rig_port_name, 11, Modulator::Cc(13, 0)), // Delay Time
                        ModulatorConfig::new(rig_port_name, 11, Modulator::Cc(14, 64)), // Tone
                        ModulatorConfig::new(rig_port_name, 10, Modulator::Cc(51, 64)), // DJ Filter
                        ////////////////////////
                        // BITBOX SLICERS
                        // row 1
                        ModulatorConfig::SlicerOffset(0, 0, 0), // A1 Offset
                        ModulatorConfig::SlicerOffset(0, 1, 32), // A2 Offset
                        ModulatorConfig::SlicerOffset(0, 2, 64), // A3 Offset
                        ModulatorConfig::SlicerOffset(0, 3, 96), // A4 Offset
                        // row 2
                        ModulatorConfig::SlicerPitch(0, 0, 63), // A1 Pitch
                        ModulatorConfig::SlicerPitch(0, 1, 63), // A2 Pitch
                        ModulatorConfig::SlicerPitch(0, 2, 63), // A3 Pitch
                        ModulatorConfig::SlicerPitch(0, 3, 63), // A4 Pitch
                        // row 3
                        ModulatorConfig::SlicerOffset(1, 0, 0), // B1 Offset
                        ModulatorConfig::SlicerOffset(1, 1, 42), // B2 Offset
                        ModulatorConfig::SlicerOffset(1, 2, 84), // B3 Offset
                        ModulatorConfig::SlicerOffset(1, 3, 0), // B4 Offset
                        // row 4
                        ModulatorConfig::SlicerPitch(1, 0, 63), // B1 Pitch
                        ModulatorConfig::SlicerPitch(1, 1, 63), // B2 Pitch
                        ModulatorConfig::SlicerPitch(1, 2, 63), // B3 Pitch
                        ModulatorConfig::SlicerPitch(1, 3, 63), // B4 Pitch
                    ],
                },
                // ControllerConfig::DuckOutput {
                //     modulators: vec![ModulatorConfig::new(
                //         rig_port_name,
                //         2,
                //         Modulator::InvertMaxCc(5, 127, 0),
                //     )],
                // },
                ControllerConfig::ClockPulse {
                    output: MidiPortConfig::new(rig_port_name, 12),
                    divider: 6,
                },
                ControllerConfig::LaunchpadTempo {
                    daw_port_name: String::from("Launchpad Pro MK3 PORT 3"),
                },
            ],
        }
    }

    pub fn minimal() -> Self {
        let sp404_port_name = "SP-404MKII"; // drums
        let launchpad_output_name = "Launchpad Pro MK3 PORT 2";
        let rig_port_name = launchpad_output_name;
        let launchpad_clock_out = "Launchpad Pro MK3";

        let mut channel_map = HashMap::new();
        channel_map.insert(4, 2);
        channel_map.insert(5, 4);
        channel_map.insert(6, 5);
        channel_map.insert(7, 6);

        Config {
            chunks: vec![
                // EXT SYNTH OFFSET
                // (also sends pitch mod on channel 2 for slicer)
                ChunkConfig {
                    coords: Coords::new(3 + 8, 0),
                    shape: Shape::new(1, 8),
                    color: 12, // soft yellow
                    channel: None,
                    trigger_channels: None,
                    repeat_mode: RepeatMode::OnlyQuant,
                    device: DeviceConfig::multi(vec![DeviceConfig::offset("ext")]),
                },
                // BASS OFFSET
                ChunkConfig {
                    device: DeviceConfig::offset("bass"),
                    coords: Coords::new(4 + 8, 0),
                    shape: Shape::new(1, 8),
                    color: 62,
                    channel: None,
                    trigger_channels: None,
                    repeat_mode: RepeatMode::OnlyQuant,
                },
                // SYNTH OFFSET
                ChunkConfig {
                    device: DeviceConfig::offset("keys"),
                    coords: Coords::new(5 + 8, 0),
                    shape: Shape::new(1, 8),
                    color: 94,
                    channel: None,
                    trigger_channels: None,
                    repeat_mode: RepeatMode::OnlyQuant,
                },
                // ROOT NOTE SELECTOR
                ChunkConfig {
                    device: DeviceConfig::RootSelect,
                    coords: Coords::new(6 + 8, 0),
                    shape: Shape::new(2, 8),
                    color: 35, // soft green
                    channel: None,
                    trigger_channels: None,
                    repeat_mode: RepeatMode::OnlyQuant,
                },
                // SCALE MODE SELECTOR
                ChunkConfig {
                    device: DeviceConfig::ScaleDegreeToggle(ScaleDegree::Second),
                    coords: Coords::new(16, 0),
                    shape: Shape::new(1, 2),
                    color: 95, // purple
                    channel: None,
                    trigger_channels: None,
                    repeat_mode: RepeatMode::OnlyQuant,
                },
                ChunkConfig {
                    device: DeviceConfig::ScaleDegreeToggle(ScaleDegree::Third),
                    coords: Coords::new(16, 2),
                    shape: Shape::new(1, 2),
                    color: 95, // black
                    channel: None,
                    trigger_channels: None,
                    repeat_mode: RepeatMode::OnlyQuant,
                },
                ChunkConfig {
                    device: DeviceConfig::ScaleDegreeToggle(ScaleDegree::Sixth),
                    coords: Coords::new(16, 4),
                    shape: Shape::new(1, 2),
                    color: 95, // purple
                    channel: None,
                    trigger_channels: None,
                    repeat_mode: RepeatMode::OnlyQuant,
                },
                ChunkConfig {
                    device: DeviceConfig::ScaleDegreeToggle(ScaleDegree::Seventh),
                    coords: Coords::new(16, 6),
                    shape: Shape::new(1, 2),
                    color: 95, // purple
                    channel: None,
                    trigger_channels: None,
                    repeat_mode: RepeatMode::OnlyQuant,
                },
                // SP-404mk2 samples (schedule first so that samples coinciding with drum triggers don't get delayed, drums are fine because running on USB midi)
                ChunkConfig {
                    device: DeviceConfig::Sp404Mk2 {
                        port_name: String::from(sp404_port_name),
                        velocity_map: Some(vec![10, 20, 30, 40, 50, 60, 70, 70, 70, 90, 100]),
                        sidechain_output: None,
                    },
                    coords: Coords::new(0, 0),
                    shape: Shape::new(2, 8),
                    color: 9, // orange
                    channel: Some(1),
                    trigger_channels: None,
                    repeat_mode: RepeatMode::Global,
                },
                // WESTON B2
                ChunkConfig {
                    device: DeviceConfig::MidiKeys {
                        output: MidiPortConfig::new(rig_port_name, 14),
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
                    color: 15, // blue
                    channel: Some(4),
                    trigger_channels: None,
                    repeat_mode: RepeatMode::Global,
                },
                // NYMPHES
                ChunkConfig {
                    device: DeviceConfig::MidiKeys {
                        output: MidiPortConfig::new(rig_port_name, 7),
                        velocity_map: None,
                        offset_wrap: false,
                        offset_id: String::from("keys"),
                        monophonic: false,
                        note_offset: -4,
                        octave_offset: -2,
                        midi_offset: 0,
                    },
                    coords: Coords::new(2, 0),
                    shape: Shape::new(6, 8),
                    color: 51, // pink
                    channel: Some(5),
                    trigger_channels: None,
                    repeat_mode: RepeatMode::Global,
                },
                // 404 chromatic
                ChunkConfig {
                    coords: Coords::new(0 + 8, 0),
                    shape: Shape::new(3, 8),
                    color: 11,
                    channel: Some(6),
                    trigger_channels: None,
                    repeat_mode: RepeatMode::Global,
                    device: DeviceConfig::multi(vec![DeviceConfig::MidiKeys {
                        offset_wrap: true,
                        output: MidiPortConfig::new(sp404_port_name, 16),
                        velocity_map: None,
                        monophonic: true,
                        offset_id: String::from("ext"),
                        note_offset: -4,
                        octave_offset: -1,
                        midi_offset: 0,
                    }]),
                },
            ],
            straight_trigger_ids: vec![],
            clock_input_port_name: String::from("Launchpad Pro MK3"),
            clock_output_port_names: vec![rig_port_name.to_string()],
            resync_port_names: vec![rig_port_name.to_string()],
            keep_alive_port_names: vec![],
            controllers: vec![
                ControllerConfig::Umi3 {
                    port_name: String::from("Logidy UMI3"),
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
    pub straight_trigger_ids: Vec<u32>,
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
    pub trigger_channels: Option<Vec<u32>>,
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
    MidiSlicer {
        output: MidiPortConfig,
        slicer_channel: u32,
        start_trigger_id: u8,
        trigger_count: u32,
        velocity_map: Option<Vec<u8>>,
    },
    CcSlicer {
        output: MidiPortConfig,
        slicer_channel: u32,
        cc: u32,
        velocity_map: Option<Vec<u8>>,
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
    },
    SlicerOffset(u32, u32, u8),
    SlicerPitch(u32, u32, u8),
    LfoAmount(usize, u8),
    LfoSpeed(u8),
    LfoHold(u8),
    LfoOffset(u8),
    LfoSkew(u8),
    DuckDecay(u8),
    DuckAmount(u8),
    Swing(u8),
}

#[derive(Serialize, Deserialize, Clone)]
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

#[derive(Serialize, Deserialize, Clone)]
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
        }
    }
    pub fn rx(port_name: &str, port_number: u8, modulator: Modulator) -> ModulatorConfig {
        ModulatorConfig::Midi {
            port: MidiPortConfig::new(port_name, port_number),
            rx_port: Some(MidiPortConfig::new(port_name, port_number)),
            modulator,
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
