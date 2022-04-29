mod cc_triggers;
mod midi_keys;
mod midi_triggers;
mod multi;
mod offset;
mod pitch_offset_chunk;
mod root_offset_chunk;
mod root_select;
mod scale_select;
mod sp404mk2;

pub use self::cc_triggers::CcTriggers;
pub use self::cc_triggers::MidiTrigger;
pub use self::midi_triggers::MidiTriggers;
pub use self::midi_triggers::SidechainOutput;
pub use self::multi::MultiChunk;
pub use self::sp404mk2::Sp404Mk2;

pub use self::midi_keys::MidiKeys;
pub use self::offset::OffsetChunk;
pub use self::pitch_offset_chunk::PitchOffsetChunk;
pub use self::root_offset_chunk::RootOffsetChunk;
pub use self::root_select::RootSelect;
pub use self::scale_select::ScaleDegreeToggle;

pub fn map_velocity(velocity_map: &Option<Vec<u8>>, velocity: u8) -> u8 {
    if let Some(velocity_map) = velocity_map {
        if velocity_map.len() > 0 {
            let group_size = 128 / velocity_map.len();
            let index = (velocity as usize / group_size).min(velocity_map.len() - 1);
            return velocity_map[index];
        }
    }

    velocity
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_map_velocity() {
        assert_eq!(map_velocity(&Some(vec![0, 100]), 0), 0);
        assert_eq!(map_velocity(&Some(vec![0, 100]), 63), 0);
        assert_eq!(map_velocity(&Some(vec![0, 100]), 64), 100);
        assert_eq!(map_velocity(&Some(vec![0, 100]), 127), 100);

        assert_eq!(map_velocity(&Some(vec![0, 100, 127]), 0), 0);
        assert_eq!(map_velocity(&Some(vec![0, 100, 127]), 41), 0);
        assert_eq!(map_velocity(&Some(vec![0, 100, 127]), 42), 100);
        assert_eq!(map_velocity(&Some(vec![0, 100, 127]), 83), 100);
        assert_eq!(map_velocity(&Some(vec![0, 100, 127]), 84), 127);
        assert_eq!(map_velocity(&Some(vec![0, 100, 127]), 127), 127);
    }
}
