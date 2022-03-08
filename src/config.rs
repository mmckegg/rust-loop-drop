use crate::devices::MidiTrigger;
use chunk::{Coords, RepeatMode, Shape};
use serde::{Deserialize, Serialize};
use serde_json::{json, to_writer_pretty};
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
        let bbx_port_name = tr6s_port_name; // sampler
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
                    device: DeviceConfig::RootSelect {
                        output_modulators: vec![ModulatorConfig::rx(
                            bbx_port_name,
                            12,
                            Modulator::PitchBend(0.0),
                        )],
                    },
                    coords: Coords::new(6 + 8, 0),
                    shape: Shape::new(2, 8),
                    color: 35, // soft green
                    channel: None,
                    repeat_mode: RepeatMode::OnlyQuant,
                },
                // SCALE MODE SELECTOR
                ChunkConfig {
                    device: DeviceConfig::ScaleSelect,
                    coords: Coords::new(16, 0),
                    shape: Shape::new(1, 8),
                    color: 0, // black
                    channel: None,
                    repeat_mode: RepeatMode::OnlyQuant,
                },
                // TR6S
                ChunkConfig {
                    device: DeviceConfig::MidiTriggers {
                        output: MidiPortConfig::new(tr6s_port_name, 10),
                        velocity_map: Some(vec![60, 80, 80, 80, 80, 127]),
                        trigger_ids: vec![36, 38, 43, 39, 42, 46],
                        sidechain_output: None,
                    },
                    coords: Coords::new(0, 0),
                    shape: Shape::new(1, 6),
                    color: 8, // warm white
                    channel: Some(0),
                    repeat_mode: RepeatMode::NoCycle,
                },
                // DFAM
                ChunkConfig {
                    device: DeviceConfig::CcTriggers {
                        output: MidiPortConfig::new(rig_port_name, 2),
                        triggers: vec![
                            MidiTrigger::Note(2, 0, 127),
                            MidiTrigger::Note(2, 127, 127),
                        ],
                    },
                    coords: Coords::new(0, 6),
                    shape: Shape::new(1, 2),
                    color: 15, // yellow
                    channel: Some(0),
                    repeat_mode: RepeatMode::NoCycle,
                },
                // SAMPLER
                ChunkConfig {
                    device: DeviceConfig::MidiTriggers {
                        output: MidiPortConfig::new(bbx_port_name, 11),
                        velocity_map: Some(vec![80, 100, 100, 100, 100, 127]),
                        trigger_ids: vec![48, 49, 50, 51, 44, 45, 46, 47],
                        sidechain_output: None,
                    },
                    coords: Coords::new(1, 0),
                    shape: Shape::new(1, 8),
                    color: 9, // orange
                    channel: Some(2),
                    repeat_mode: RepeatMode::OnlyQuant,
                },
                // BASSLINE
                ChunkConfig {
                    device: DeviceConfig::MonoMidiKeys {
                        output: MidiPortConfig::new(rig_port_name, 3),
                        velocity_map: None,
                        offset_id: String::from("bass"),
                        note_offset: -4,
                        octave_offset: -2,
                    },
                    coords: Coords::new(2, 0),
                    shape: Shape::new(6, 4),
                    color: 43, // blue
                    channel: Some(4),
                    repeat_mode: RepeatMode::Global,
                },
                // PLAITS
                ChunkConfig {
                    device: DeviceConfig::MonoMidiKeys {
                        output: MidiPortConfig::new(rig_port_name, 4),
                        velocity_map: None,
                        offset_id: String::from("keys"),
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
                    color: 125, // gross
                    channel: Some(6),
                    repeat_mode: RepeatMode::Global,
                    device: DeviceConfig::multi(vec![DeviceConfig::MidiKeys {
                        output: MidiPortConfig::new(launchpad_output_name, 5),
                        velocity_map: Some(vec![90, 100, 127]),
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
                    modulators: vec![
                        // row 1
                        ModulatorConfig::new(tr6s_port_name, 10, Modulator::Cc(19, 64)), // drum filter
                        ModulatorConfig::new(rig_port_name, 2, Modulator::InvertCc(11, 0)), // drum fx
                        ModulatorConfig::new(rig_port_name, 2, Modulator::MaxCc(9, 64, 0)), // mod a
                        ModulatorConfig::new(rig_port_name, 2, Modulator::MaxCc(10, 64, 0)), // mod b
                        // row 2
                        ModulatorConfig::new(tr6s_port_name, 10, Modulator::Cc(23, 32)), // bd decay
                        ModulatorConfig::new(tr6s_port_name, 10, Modulator::Cc(62, 32)), // ch decay
                        ModulatorConfig::new(bbx_port_name, 11, Modulator::Cc(1, 64)), // sampler filter
                        ModulatorConfig::new(rig_port_name, 2, Modulator::InvertCc(12, 16)), // sampler fx
                        // row 3
                        ModulatorConfig::new(rig_port_name, 2, Modulator::Cc(5, 64)), // bass filter
                        ModulatorConfig::new(rig_port_name, 2, Modulator::InvertCc(1, 0)), // bass fx
                        ModulatorConfig::new(rig_port_name, 2, Modulator::Cc(6, 64)), // plaits filter
                        ModulatorConfig::new(rig_port_name, 2, Modulator::InvertCc(2, 16)), // plaits fx
                        // row 4
                        ModulatorConfig::new(rig_port_name, 2, Modulator::Cc(7, 64)), // poly filter
                        ModulatorConfig::new(rig_port_name, 2, Modulator::InvertCc(3, 16)), // poly fx
                        ModulatorConfig::new(rig_port_name, 2, Modulator::Cc(8, 64)), // main filter
                        ModulatorConfig::new(rig_port_name, 2, Modulator::InvertCc(4, 0)), // main fx
                    ],
                },
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
        note_offset: i32,
        velocity_map: Option<Vec<u8>>,
        octave_offset: i32,
    },
    MonoMidiKeys {
        output: MidiPortConfig,
        offset_id: String,
        note_offset: i32,
        velocity_map: Option<Vec<u8>>,
        octave_offset: i32,
    },
    OffsetChunk {
        id: String,
    },
    PitchOffsetChunk {
        output: MidiPortConfig,
    },
    RootSelect {
        output_modulators: Vec<Option<ModulatorConfig>>,
    },
    ScaleSelect,
    MidiTriggers {
        output: MidiPortConfig,
        trigger_ids: Vec<u8>,
        velocity_map: Option<Vec<u8>>,
        sidechain_output: Option<SidechainOutput>,
    },
    CcTriggers {
        output: MidiPortConfig,
        triggers: Vec<MidiTrigger>,
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
        modulators: Vec<Option<ModulatorConfig>>,
    },
    ModTwister {
        port_name: String,
        modulators: Vec<Option<ModulatorConfig>>,
    },
    Umi3 {
        port_name: String,
    },
    VT4Key {
        output: MidiPortConfig,
    },
    ClockPulse {
        output: MidiPortConfig,
        divider: i32,
    },
    LaunchpadTempo {
        daw_port_name: String,
    },
    Init {
        modulators: Vec<Option<ModulatorConfig>>,
    },
}

#[derive(Serialize, Deserialize, Clone)]
pub struct ModulatorConfig {
    pub port: MidiPortConfig,
    pub rx_port: Option<MidiPortConfig>,
    pub modulator: Modulator,
}

#[derive(Serialize, Deserialize, Clone)]
pub enum Modulator {
    Cc(u8, u8),
    InvertCc(u8, u8),
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
    pub fn new(port_name: &str, port_number: u8, modulator: Modulator) -> Option<Self> {
        Some(ModulatorConfig {
            port: MidiPortConfig::new(port_name, port_number),
            rx_port: None,
            modulator,
        })
    }
    pub fn rx(port_name: &str, port_number: u8, modulator: Modulator) -> Option<Self> {
        Some(ModulatorConfig {
            port: MidiPortConfig::new(port_name, port_number),
            rx_port: Some(MidiPortConfig::new(port_name, port_number)),
            modulator,
        })
    }
}
