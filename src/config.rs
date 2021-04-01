use ::serde::{Deserialize, Serialize};
use chunk::{Shape, Coords, ChunkMap, RepeatMode};
use serde_json::{json, to_writer_pretty};
use std::error::Error;
use std::fs::File;
use std::io::BufReader;

impl Config {
  pub fn read(filepath: &str) -> Result<Self, Box<Error>> {
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
    Config {
        chunks: vec![
            ChunkConfig {
                shape: Shape::new(1, 8),
                coords: Coords::new(0, 0),
                color: 22,
                channel: Some(1),
                repeat_mode: RepeatMode::Global,
                device: DeviceConfig::Multi {
                  devices: vec![
                    DeviceConfig::MidiKeys{
                      output: MidiPortConfig{
                          name: String::from("RK006 PORT 2"),
                          channel: 2
                      },
                      offset_id: String::from("SomeNameToUseItSomewhere"),
                      note_offset: 0,
                      octave_offset: -1
                    }, 
                    DeviceConfig::MidiKeys{
                      output: MidiPortConfig{
                          name: String::from("RK006 PORT 3"),
                          channel: 3
                      },
                      offset_id: String::from("SomeNameToUseItSomewhere"),
                      note_offset: 0,
                      octave_offset: -1
                    }
                  ]
                }
            },
            ChunkConfig {
                shape: Shape::new(1, 8),
                coords: Coords::new(1, 0),
                color: 44,
                channel: Some(2),
                repeat_mode: RepeatMode::Global,
                device: DeviceConfig::MidiTriggers {
                    output: MidiPortConfig{
                        name: String::from("RK006 PORT 3"),
                        channel: 1
                    },
                    sidechain_output: None,
                    trigger_ids: vec![48, 49, 50, 51, 44, 45, 46, 47]
                }
            },
            ChunkConfig {
                shape: Shape::new(1, 8),
                coords: Coords::new(2, 0),
                color: 66,
                channel: Some(3),
                repeat_mode: RepeatMode::Global,
                device: DeviceConfig::OffsetChunk {
                    id: String::from("SomeNameToUseItSomewhere")
                }
            },
            ChunkConfig {
                shape: Shape::new(1, 8),
                coords: Coords::new(3, 0),
                color: 88,
                channel: Some(4),
                repeat_mode: RepeatMode::Global,
                device: DeviceConfig::RootSelect
            }
        ],
        clock_input_port_name: String::from("TR-6S"),
        clock_output_port_names: vec![String::from("clock_output_port_names")],
        resync_port_names: vec![String::from("resync_port_names")],
        twister_main_output_port: None
    }
  }
}

#[derive(Serialize, Deserialize)]
pub struct Config {
    pub chunks: Vec<ChunkConfig>,
    pub clock_input_port_name: String,
    pub clock_output_port_names: Vec<String>,
    pub resync_port_names: Vec<String>,
    pub twister_main_output_port: Option<MidiPortConfig>
}

#[derive(Serialize, Deserialize)]
pub struct ChunkConfig {
    pub coords: Coords,
    pub shape: Shape,
    pub color: u8,
    pub channel: Option<u32>,
    pub repeat_mode: RepeatMode,
    pub device: DeviceConfig
}

#[derive(Serialize, Deserialize, Clone)]
pub struct MidiPortConfig {
    pub name: String,
    pub channel: u8
}

#[derive(Serialize, Deserialize, Clone)]
pub struct SidechainOutput {
    pub port: MidiPortConfig,
    pub trigger_id: u8,
    pub id: u32
}

#[derive(Serialize, Deserialize, Clone)]
pub enum DeviceConfig {
  Multi {
    devices: Vec<DeviceConfig>
  },
  MidiKeys { 
      output: MidiPortConfig, 
      offset_id: String,
      note_offset: i32,
      octave_offset: i32
  },
  OffsetChunk { id: String },
//   PitchOffsetChunk { output: MidiPortConfig },
  RootSelect,
//   ScaleSelect,
  MidiTriggers { 
    output: MidiPortConfig,
    trigger_ids: Vec<u8>,
    sidechain_output: Option<SidechainOutput>
  }
}

