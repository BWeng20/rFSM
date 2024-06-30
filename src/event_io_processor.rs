use std::collections::HashMap;
use std::fmt::Debug;
use std::sync::mpsc::Sender;

use log::info;

use crate::datamodel::{Datamodel, ToAny};
use crate::fsm::{Event, EVENT_CANCEL_SESSION, Fsm};

pub const SYS_IO_PROCESSORS: &str = "_ioprocessors";

#[derive(Debug, Clone)]
pub struct EventIOProcessorHandle {
    /// The FSMs that are connected to this IO Processor
    pub fsms: HashMap<u32, Sender<Box<Event>>>,
}

impl EventIOProcessorHandle {
    pub fn new() -> EventIOProcessorHandle {
        EventIOProcessorHandle {
            fsms: HashMap::new()
        }
    }
    pub fn shutdown(&mut self) {
        let cancel_event = Event::new_simple(EVENT_CANCEL_SESSION);
        for (id, sender) in &self.fsms {
            info!("Send cancel to fsm #{}", id);
            let _ = sender.send(cancel_event.get_copy());
        }
    }
}

/// Trait for Event I/O Processors. \
/// See https://www.w3.org/TR/scxml/#eventioprocessors
/// As the I/O Processors hold session related data, an instance of this trait must be bound to one session,
/// but may share backends with other sessions, e.g. a http server.
pub trait EventIOProcessor: ToAny + Debug + Send {
    /// Returns the location of this processor.
    fn get_location(&self) -> String;

    /// Returns the type of this processor.
    fn get_types(&self) -> &[&str];

    fn get_handle(&mut self) -> &mut EventIOProcessorHandle;

    fn add_fsm(&mut self, fsm: &Fsm, datamodel: &mut dyn Datamodel) {
        self.get_handle().fsms.insert(fsm.session_id, datamodel.global_s().lock().unwrap().externalQueue.sender.clone());
    }

    fn get_copy(&self) -> Box<dyn EventIOProcessor>;

    fn shutdown(&mut self);
}

