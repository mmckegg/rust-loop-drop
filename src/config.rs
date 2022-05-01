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
        let tr6s_port_name = "TR-6S"; // drums
        let launchpad_output_name = "Launchpad Pro MK3 PORT 2";
        let rig_port_name = launchpad_output_name;
        let launchpad_clock_out = "Launchpad Pro MK3";

        Config {
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
                // TR-6S
                ChunkConfig {
                    device: DeviceConfig::MidiTriggers {
                        output: MidiPortConfig::new(tr6s_port_name, 12),
                        velocity_map: Some(vec![40, 80, 80, 80, 80, 127]),
                        trigger_ids: vec![36, 38, 43, 39, 42, 46],
                        sidechain_output: Some(SidechainOutput { id: 0 }),
                    },
                    coords: Coords::new(0, 0),
                    shape: Shape::new(1, 6),
                    color: 8, // warm white
                    channel: Some(0),
                    repeat_mode: RepeatMode::NoCycle,
                },
                // SP-404mk2 samples
                ChunkConfig {
                    device: DeviceConfig::Sp404Mk2 {
                        port_name: String::from(tr6s_port_name),
                        default_mapping: vec![],
                        velocity_map: Some(vec![10, 20, 30, 40, 50, 60, 70, 70, 70, 70, 80]),
                        sidechain_output: None,
                    },
                    coords: Coords::new(1, 0),
                    shape: Shape::new(1, 8),
                    color: 9, // orange
                    channel: Some(2),
                    repeat_mode: RepeatMode::OnlyQuant,
                },
                // dfam
                ChunkConfig {
                    device: DeviceConfig::CcTriggers {
                        velocity_map: Some(vec![
                            20, 30, 40, 40, 40, 40, 40, 40, 40, 40, 50, 50, 50, 60, 70, 80, 90, 127,
                        ]),
                        output: MidiPortConfig::new(rig_port_name, 13),
                        triggers: vec![
                            MidiTrigger::NoteVelocity(13, 0),
                            MidiTrigger::Note(13, 127, 127),
                        ],
                    },
                    coords: Coords::new(0, 6),
                    shape: Shape::new(1, 2),
                    color: 15, // yellow
                    channel: Some(0),
                    repeat_mode: RepeatMode::NoCycle,
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
                        octave_offset: -4,
                    },
                    coords: Coords::new(2, 0),
                    shape: Shape::new(6, 4),
                    color: 43, // blue
                    channel: Some(4),
                    repeat_mode: RepeatMode::Global,
                },
                // PLAITS
                ChunkConfig {
                    device: DeviceConfig::MidiKeys {
                        output: MidiPortConfig::new(rig_port_name, 15),
                        velocity_map: None,
                        offset_wrap: false,
                        offset_id: String::from("keys"),
                        monophonic: true,
                        note_offset: -4,
                        octave_offset: -2,
                    },
                    coords: Coords::new(2, 4),
                    shape: Shape::new(6, 4),
                    color: 59, // pink
                    channel: Some(5),
                    repeat_mode: RepeatMode::Global,
                },
                // POLY CINEMATIC
                ChunkConfig {
                    coords: Coords::new(0 + 8, 0),
                    shape: Shape::new(3, 8),
                    color: 51,
                    channel: Some(6),
                    repeat_mode: RepeatMode::Global,
                    device: DeviceConfig::multi(vec![DeviceConfig::MidiKeys {
                        offset_wrap: true,
                        output: MidiPortConfig::new(rig_port_name, 11),
                        velocity_map: None,
                        monophonic: false,
                        offset_id: String::from("ext"),
                        note_offset: -4,
                        octave_offset: -1,
                    }]),
                },
            ],
            clock_input_port_name: String::from(tr6s_port_name),
            clock_output_port_names: vec![String::from(launchpad_clock_out)],
            resync_port_names: vec![String::from(launchpad_output_name)],
            keep_alive_port_names: vec![],
            controllers: vec![
                ControllerConfig::Umi3 {
                    port_name: String::from("Logidy UMI3"),
                },
                ControllerConfig::ModTwister {
                    port_name: String::from("Midi Fighter Twister"),
                    continuously_send: vec![0,1,2, 4,5,6, 9,10, 12,13,14,15],
                    modulators: vec![
                        // row 1
                        ModulatorConfig::new(rig_port_name, 2, Modulator::Cc(5, 64)), // main filter
                        ModulatorConfig::new(rig_port_name, 2, Modulator::InvertCc(6, 0)), // main fx
                        ModulatorConfig::new(rig_port_name, 2, Modulator::Cc(7, 0)), // main fx mod
                        ModulatorConfig::Swing(0),
                        // row 2
                        ModulatorConfig::new(tr6s_port_name, 12, Modulator::Cc(19, 64)), // drum filter                        
                        ModulatorConfig::new(rig_port_name, 2, Modulator::Cc(3, 32)), // bass mod
                        ModulatorConfig::new(rig_port_name, 2, Modulator::Cc(4, 127)), // synth mod
                        ModulatorConfig::new(rig_port_name, 2, Modulator::Cc(1, 0)), // poly filter
                        // row 3
                        ModulatorConfig::DuckDecay(10), // duck decay
                        ModulatorConfig::new(rig_port_name, 14, Modulator::PitchBend(0.0)), // bass pitch
                        ModulatorConfig::new(rig_port_name, 15, Modulator::PitchBend(0.0)), // plaits pitch
                        ModulatorConfig::new(rig_port_name, 11, Modulator::Cc(94, 10)), // poly detune
                        // row 4
                        ModulatorConfig::new(rig_port_name, 2, Modulator::Cc(9, 0)), // mod a
                        ModulatorConfig::new(rig_port_name, 2, Modulator::Cc(10, 0)), // mod b
                        ModulatorConfig::new(rig_port_name, 2, Modulator::Cc(11, 0)), // mod c
                        ModulatorConfig::new(rig_port_name, 2, Modulator::Cc(12, 0)), // mod d
                        ////////////////////////
                        // DRUMS
                        // row 1
                        ModulatorConfig::new(tr6s_port_name, 12, Modulator::Cc(23, 20)), // bd decay
                        ModulatorConfig::new(tr6s_port_name, 12, Modulator::Cc(28, 10)), // sd decay
                        ModulatorConfig::new(tr6s_port_name, 12, Modulator::Cc(47, 10)), // lt decay
                        ModulatorConfig::new(tr6s_port_name, 12, Modulator::Cc(59, 32)), // hc decay
                        // row 2
                        ModulatorConfig::new(tr6s_port_name, 12, Modulator::Cc(96, 0)), // bd ctrl
                        ModulatorConfig::new(tr6s_port_name, 12, Modulator::Cc(97, 0)), // sd ctrl
                        ModulatorConfig::new(tr6s_port_name, 12, Modulator::Cc(102, 0)), // lt ctrl
                        ModulatorConfig::new(tr6s_port_name, 12, Modulator::Cc(106, 0)), // hc ctrl
                        // row 3
                        ModulatorConfig::new(tr6s_port_name, 12, Modulator::Cc(20, 64)), // bd pitch
                        ModulatorConfig::new(tr6s_port_name, 12, Modulator::Cc(17, 64)), // delay time
                        ModulatorConfig::new(tr6s_port_name, 12, Modulator::Cc(62, 32)), // ch decay
                        ModulatorConfig::new(tr6s_port_name, 12, Modulator::Cc(81, 64)), // oh decay
                        // row 4
                        ModulatorConfig::new(tr6s_port_name, 12, Modulator::Cc(91, 0)), // reverb amount
                        ModulatorConfig::new(tr6s_port_name, 12, Modulator::Cc(18, 40)), // delay feedback
                        ModulatorConfig::new(tr6s_port_name, 12, Modulator::Cc(107, 0)), // ch ctrl
                        ModulatorConfig::new(tr6s_port_name, 12, Modulator::Cc(108, 0)), // oh ctrl
                    ],
                },
                ControllerConfig::DuckOutput {
                    modulators: vec![ModulatorConfig::new(rig_port_name, 2, Modulator::InvertMaxCc(8, 100, 0))]
                },
                ControllerConfig::ClockPulse { output: MidiPortConfig::new(rig_port_name, 12), divider: 12 }
                // ControllerConfig::LaunchpadTempo {
                //     daw_port_name: String::from("Launchpad Pro MK3 PORT 3"),
                // },
            ],
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct Config {
    pub chunks: Vec<ChunkConfig>,
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
        default_mapping: Vec<(u8, u8, u8)>,
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
        modulators: Vec<ModulatorConfig>,
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
    DuckDecay(u8),
    Swing(u8),
}

#[derive(Serialize, Deserialize, Clone)]
pub enum Modulator {
    Cc(u8, u8),
    InvertCc(u8, u8),
    InvertMaxCc(u8, u8, u8),
    MaxCc(u8, u8, u8),
    PolarCcSwitch {
        cc_low: Option<u8>,
        cc_high: Option<u8>,
        cc_switch: Option<u8>,
        default: u8,
    },
    PitchBend(f64),
    PositivePitchBend(f64),
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
