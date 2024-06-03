extern crate core;

use std::net::{IpAddr, Ipv4Addr};
use std::sync::{Arc, Mutex};
use std::sync::mpsc::Sender;
use std::thread::JoinHandle;

use log::info;

use crate::{fsm, reader};

#[cfg(feature = "BasicHttpEventIOProcessor")]
use crate::basic_http_event_io_processor::BasicHTTPEventIOProcessor;

use crate::event_io_processor::EventIOProcessor;
use crate::fsm::Event;
use crate::scxml_event_io_processor::ScxmlEventIOProcessor;
use crate::tracer::TraceMode;

pub struct ExecuterState {
    pub processors: Vec<Box<dyn EventIOProcessor>>,

}

impl ExecuterState {
    pub fn new() -> ExecuterState {
        let e = ExecuterState {
            processors: Vec::new(),
        };
        e
    }
}

/// Executed FSM in separate threads.
/// This class maintains IO Processors used by the FSMs.
pub struct FsmExecutor {
    pub state: Arc<Mutex<ExecuterState>>,
}

impl FsmExecutor {
    pub fn add_processor(&mut self, processor: Box<dyn EventIOProcessor>) {
        self.state.lock().unwrap().processors.push(processor);
    }

    pub fn new() -> FsmExecutor {
        let mut e = FsmExecutor {
            state: Arc::new(Mutex::new(ExecuterState::new())),
        };
        #[cfg(feature = "BasicHttpEventIOProcessor")]
        {
            let w = Box::new(BasicHTTPEventIOProcessor::new(
                IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), "localhost", 5555));
            e.add_processor(w);
        }
        e.add_processor(Box::new(ScxmlEventIOProcessor::new()));
        e
    }

    /// Shutdown of all FSMs and IO-Processors.
    pub fn shutdown(&mut self) {
        let mut guard = self.state.lock().unwrap();
        while !guard.processors.is_empty() {
            let p = guard.processors.pop();
            match p {
                Some(mut pp) => {
                    pp.shutdown();
                }
                None => {}
            }
        }
    }

    /// Loads and starts the specified FSM.
    pub fn execute(&mut self, file_path: &str, trace: TraceMode) -> Result<(JoinHandle<()>, Sender<Box<Event>>), String> {
        info!("Loading FSM from {}", file_path);

        // Use reader to parse the scxml file:
        let sm = reader::read_from_xml_file(file_path.to_string());
        match sm {
            Ok(mut fsm) => {
                fsm.tracer.enable_trace(trace);
                let th = fsm::start_fsm(fsm, &self.state);
                Ok(th)
            }
            Err(message) => { return Err(message); }
        }
    }
}


