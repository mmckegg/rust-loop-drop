use ::serde::{Deserialize, Serialize};
use ::serde_json::Result;
use chunk::{Shape, Coords, ChunkMap, RepeatMode};

#[derive(Serialize, Deserialize)]
pub struct Config {
    pub chunks: Vec<ChunkConfig>,
    pub clock_input_port_name: String,
    pub clock_output_port_names: Vec<String>,
    pub resync_port_names: Vec<String>
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

#[derive(Serialize, Deserialize)]
pub struct MidiPortConfig {
    pub name: String,
    pub channel: u8
}

#[derive(Serialize, Deserialize)]
pub enum DeviceConfig {
//   MultiChunk,
  MidiKeys { 
      outputs: Vec<MidiPortConfig>, 
      offset_id: String,
      note_offset: i32,
      octave_offset: i32
  },
//   BlackboxSlicer { output: MidiPortConfig },
  OffsetChunk { id: String },
//   PitchOffsetChunk { output: MidiPortConfig },
  RootSelect,
//   ScaleSelect,
//   TR6s { output: MidiPortConfig },
//   BlackboxPerc { output: MidiPortConfig },
  BlackboxSample { output: MidiPortConfig }
}

