// mod k_mix;
mod mother_32;
mod sp404;
mod tr08;
mod twister;
mod volca_bass;
mod volca_keys;
mod k_board;
mod offset;
mod sp404_offset;
mod root_select;
mod scale_select;
mod choke_switch;

// mod volca_sample;

// pub use self::k_mix::KMix;
pub use self::mother_32::Mother32;
pub use self::sp404::SP404;
pub use self::sp404::SP404Choke;
pub use self::sp404::SP404VelocityMap;
pub use self::sp404_offset::SP404Offset;
pub use self::choke_switch::ChokeSwitch;
pub use self::tr08::TR08;
pub use self::twister::Twister;
pub use self::volca_bass::VolcaBass;
pub use self::volca_keys::VolcaKeys;
pub use self::k_board::KBoard;
pub use self::offset::OffsetChunk;
pub use self::root_select::RootSelect;
pub use self::scale_select::ScaleSelect;


// pub use self::volca_sample::VolcaSample;