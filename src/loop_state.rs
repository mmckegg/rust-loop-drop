#[derive(Debug)]
pub struct Loop {
    pub length: f64,
    pub offset: f64,
    //transforms: Vec<LoopTransform>
}

#[derive(PartialEq, Debug)]
pub enum LoopTransform {
    On,
    None,
    Repeat(f64, f64),
    Hold(f64, f64),
    Suppress
}

pub struct LoopState {
    undos: Vec<Loop>,
    redos: Vec<Loop>,
    on_change: Box<FnMut(&Loop) + Send>
}

impl LoopState {
    pub fn new<F> (default_length: f64, on_change: F) -> LoopState
    where F: FnMut(&Loop) + Send + 'static  {
        let default_loop = Loop {
            offset: 0.0 - default_length,
            length: default_length
        };
        LoopState {
            undos: vec![default_loop],
            redos: Vec::new(),
            on_change: Box::new(on_change)
        }
    }

    pub fn get (&self) -> &Loop {
        &self.undos.last().unwrap()
    }

    pub fn set (&mut self, value: Loop) {
        self.undos.push(value);
        (self.on_change)(self.undos.last().unwrap());
    }

    pub fn undo (&mut self) {
        if self.undos.len() > 1 {
            match self.undos.pop() {
                Some(value) => {
                    self.redos.push(value);
                    (self.on_change)(self.undos.last().unwrap());
                },
                None => ()
            };
        }
    }

    pub fn redo (&mut self) {
        match self.redos.pop() {
            Some(value) => {
                self.undos.push(value);
                (self.on_change)(self.undos.last().unwrap());
            },
            None => ()
        };
    }
}