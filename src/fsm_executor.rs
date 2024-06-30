extern crate core;

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr};
use std::path::{Path, PathBuf};
#[cfg(test)]
use std::println as info;
use std::sync::{Arc, Mutex};
use std::sync::mpsc::Sender;
use std::thread::JoinHandle;

#[cfg(not(test))]
use log::info;

use crate::{ArgOption, fsm, reader};
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
#[derive(Clone)]
pub struct FsmExecutor {
    pub state: Arc<Mutex<ExecuterState>>,
    pub include_paths: Vec<PathBuf>,
}

pub static INCLUDE_PATH_ARGUMENT_OPTION: ArgOption = ArgOption {
    name: "includePaths",
    with_value: true,
    required: false,
};

pub fn include_path_from_arguments(named_arguments: &HashMap::<&'static str, String>) -> Vec<PathBuf> {
    let mut include_paths = Vec::new();
    match named_arguments.get(INCLUDE_PATH_ARGUMENT_OPTION.name) {
        None => {}
        Some(path) => {
            for pa in path.split(std::path::MAIN_SEPARATOR).filter(|&p| !p.is_empty()) {
                include_paths.push(Path::new(pa).to_owned());
            }
        }
    }
    include_paths
}

impl FsmExecutor {
    pub fn add_processor(&mut self, processor: Box<dyn EventIOProcessor>) {
        self.state.lock().unwrap().processors.push(processor);
    }

    pub fn new_without_io_processor() -> FsmExecutor {
        let mut e = FsmExecutor {
            state: Arc::new(Mutex::new(ExecuterState::new())),
            include_paths: Vec::new(),
        };
        e.add_processor(Box::new(ScxmlEventIOProcessor::new()));
        e
    }

    pub async fn new_with_io_processor() -> FsmExecutor {
        let mut e = FsmExecutor {
            state: Arc::new(Mutex::new(ExecuterState::new())),
            include_paths: Vec::new(),
        };
        #[cfg(feature = "BasicHttpEventIOProcessor")]
        {
            let w = Box::new(BasicHTTPEventIOProcessor::new(
                IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), "localhost", 5555).await);
            e.add_processor(w);
        }
        e.add_processor(Box::new(ScxmlEventIOProcessor::new()));
        e
    }

    pub fn set_include_paths_from_arguments(&mut self, named_arguments: &HashMap::<&'static str, String>) {
        self.set_include_paths(&include_path_from_arguments(named_arguments));
    }

    pub fn set_include_paths(&mut self, include_path: &Vec<PathBuf>) {
        for p in include_path {
            self.include_paths.push(p.clone());
        }
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
        let sm = reader::read_from_xml_file(file_path.to_string(), &self.include_paths);
        match sm {
            Ok(mut fsm) => {
                fsm.tracer.enable_trace(trace);
                fsm.executer = Some(Box::new(self.clone()));
                let th = fsm::start_fsm(fsm);
                Ok(th)
            }
            Err(message) => { return Err(message); }
        }
    }
}


