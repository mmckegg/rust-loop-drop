mod cc_slicer;
mod cc_triggers;
mod midi_keys;
mod midi_slicer;
mod midi_triggers;
mod multi;
mod offset;
mod pitch_offset_chunk;
mod root_offset_chunk;
mod root_select;
mod scale_select;

pub use self::midi_keys::MidiKeys;
pub use self::midi_triggers::MidiTriggers;
pub use self::offset::OffsetChunk;
pub use self::scale_select::ScaleDegreeToggle;

pub fn map_velocity(velocity_map: &Option<Vec<u8>>, velocity: u8) -> u8 {
    if let Some(velocity_map) = velocity_map {
        if velocity_map.len() > 0 {
            return if velocity == 0 {
                *velocity_map.first().unwrap_or(&0)
            } else if velocity == 127 {
                *velocity_map.last().unwrap_or(&127)
            } else if velocity_map.len() <= 2 {
                if velocity < 64 {
                    *velocity_map.first().unwrap_or(&0)
                } else {
                    *velocity_map.last().unwrap_or(&127)
                }
            } else {
                let index = (((velocity - 1) as f32 / 126.0) * (velocity_map.len() - 2) as f32)
                    .round() as usize;
                velocity_map[index + 1]
            };
        }
    }

    velocity
}
