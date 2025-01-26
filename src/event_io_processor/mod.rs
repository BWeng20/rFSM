//! Event I/O processors Base.
//! For details about the concept see [W3C:The Event I/O Processors](doc/W3C_SCXML_2024_07_13/index.html#eventioprocessors).\
//! This module provides a trait to implement Event-I/O-Processors and contains some implementations.

use std::collections::HashMap;
use std::fmt::Debug;
use std::sync::mpsc::Sender;

#[cfg(feature = "Debug")]
use crate::common::debug;

use crate::datamodel::{Datamodel, GlobalDataArc, ToAny};
use crate::fsm::SessionId;
use crate::fsm::{Event, Fsm, EVENT_CANCEL_SESSION};

#[cfg(feature = "BasicHttpEventIOProcessor")]
pub mod http_event_io_processor;

pub mod scxml_event_io_processor;

pub const SYS_IO_PROCESSORS: &str = "_ioprocessors";

#[derive(Debug, Clone, Default)]
pub struct ExternalQueueContainer {
    /// The FSMs that are connected to this IO Processor
    pub fsms: HashMap<u32, Sender<Box<Event>>>,
}

impl ExternalQueueContainer {
    pub fn new() -> ExternalQueueContainer {
        ExternalQueueContainer {
            fsms: HashMap::new(),
        }
    }

    pub fn shutdown(&mut self) {
        let cancel_event = Event::new_simple(EVENT_CANCEL_SESSION);
        #[allow(unused_variables)]
        for (id, sender) in &self.fsms {
            #[cfg(feature = "Debug")]
            debug!("Send cancel to fsm #{}", id);
            let _ = sender.send(cancel_event.get_copy());
        }
    }
}

/// Trait for Event I/O Processors. \
/// See [../../../doc/W3C_SCXML_2024_07_13/index.html#eventioprocessors].
/// As the I/O Processors hold session related data, an instance of this trait must be bound to one session,
/// but may share backends with other sessions, e.g. a http server.
pub trait EventIOProcessor: ToAny + Debug + Send {
    /// Returns the location of this session and processor.
    fn get_location(&self, id: SessionId) -> String;

    /// Returns the type of this processor.
    fn get_types(&self) -> &[&str];

    fn get_external_queues(&mut self) -> &mut ExternalQueueContainer;

    fn add_fsm(&mut self, _fsm: &Fsm, datamodel: &mut dyn Datamodel) {
        let global = datamodel.global().lock().unwrap();
        self.get_external_queues()
            .fsms
            .insert(global.session_id, global.externalQueue.sender.clone());
    }

    fn get_copy(&self) -> Box<dyn EventIOProcessor>;

    fn send(&mut self, global: &GlobalDataArc, target: &str, event: Event) -> bool;

    fn shutdown(&mut self);
}
