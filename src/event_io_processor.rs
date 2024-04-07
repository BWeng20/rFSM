use std::collections::HashMap;
use std::fmt::Debug;
use std::sync::mpsc::Sender;

use crate::datamodel::{Datamodel, ToAny};
use crate::fsm::{Event, Fsm};

pub const SYS_IO_PROCESSORS: &str = "_ioprocessors";

pub struct EventIOProcessorHandle {
    /// The FSMs that are connected to this IO Processor
    pub fsms: HashMap<u32, Sender<Box<Event>>>,
}

/// Trait for Event I/O Processors. \
/// See https://www.w3.org/TR/scxml/#eventioprocessors
/// As the I/O Processors hold session related data, an instance of this trait must be bound to one session,
/// but may share backends with other sessions, e.g. a http server.
pub trait EventIOProcessor: ToAny + Debug + Send {
    /// Returns the location of this processor.
    fn get_location(&self) -> &str;

    /// Returns the type of this processor.
    fn get_type(&self) -> &str;

    fn get_handle(&mut self) -> &mut EventIOProcessorHandle;

    fn add_fsm(&mut self, fsm: &Fsm, datamodel: &mut dyn Datamodel) {
        self.get_handle().fsms.insert(fsm.session_id, datamodel.global_s().externalQueue.sender.clone());
    }

    fn get_copy(&self) -> Box<dyn EventIOProcessor>;

    fn shutdown(&self);
}

