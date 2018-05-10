#[derive(Ord, PartialOrd, Debug, Eq, PartialEq, Copy, Clone)]
pub enum OutputValue {
    // Insert offs after ons when sorting
    On(u8), Off
}

impl OutputValue {
    pub fn is_on (&self) -> bool {
        match self {
            &OutputValue::Off => false,
            &OutputValue::On(_) => true
        }
    }

    pub fn value (&self) -> u8 {
        match self {
            &OutputValue::Off => 0,
            &OutputValue::On(value) => value
        }
    }
}