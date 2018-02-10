#[derive(Ord, PartialOrd, Debug, Eq, PartialEq, Copy, Clone)]
pub enum OutputValue {
    Off, On(u8)
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