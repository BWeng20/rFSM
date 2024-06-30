use std::fmt::Debug;
#[cfg(test)]
use std::println as info;

#[cfg(not(test))]
use log::info;

use crate::datamodel::SCXML_EVENT_PROCESSOR;
/// See https://www.w3.org/TR/scxml/#SCXMLEventProcessor

use crate::event_io_processor::{EventIOProcessor, EventIOProcessorHandle};

#[derive(Debug)]
pub struct ScxmlEventIOProcessor {
    pub location: String,
    pub handle: EventIOProcessorHandle,
}

impl ScxmlEventIOProcessor {
    pub fn new() -> ScxmlEventIOProcessor {
        info!("Scxml Event Processor starting");

        let e = ScxmlEventIOProcessor
        {
            location: "scxml-processor".to_string(),
            handle: EventIOProcessorHandle::new(),
        };
        e
    }
}

const TYPES: &[&str] = &[SCXML_EVENT_PROCESSOR, "scxml"];

impl EventIOProcessor for ScxmlEventIOProcessor {
    fn get_location(&self) -> String {
        self.location.clone()
    }

    /// Returns the type of this processor.
    fn get_types(&self) -> &[&str] { TYPES }

    fn get_handle(&mut self) -> &mut EventIOProcessorHandle {
        &mut self.handle
    }

    fn get_copy(&self) -> Box<dyn EventIOProcessor> {
        let b = ScxmlEventIOProcessor {
            location: self.location.clone(),
            handle: self.handle.clone(),
        };
        Box::new(b)
    }

    /// This processor doesn't really need a shutdown.
    /// The implementation does nothing.
    fn shutdown(&mut self) {
        info!("Scxml Event IO Processor shutdown...");
        self.handle.shutdown();
    }
}
