//! Helper module to maintain FSM sessions.\

extern crate core;

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr};
use std::path::PathBuf;
#[cfg(test)]
use std::println as info;
use std::sync::mpsc::{SendError, Sender};
use std::sync::{Arc, Mutex};

#[cfg(not(test))]
use log::info;

#[cfg(feature = "BasicHttpEventIOProcessor")]
use crate::basic_http_event_io_processor::BasicHTTPEventIOProcessor;
use crate::event_io_processor::EventIOProcessor;
use crate::fsm;
use crate::fsm::{Event, FinishMode, InvokeId, ParamPair, ScxmlSession, SessionId};
use crate::scxml_event_io_processor::ScxmlEventIOProcessor;
#[cfg(feature = "xml")]
use crate::scxml_reader;
use crate::scxml_reader::include_path_from_arguments;
#[cfg(feature = "Trace")]
use crate::tracer::TraceMode;

#[derive(Default)]
pub struct ExecuteState {
    pub processors: Vec<Box<dyn EventIOProcessor>>,
    pub sessions: HashMap<SessionId, ScxmlSession>,
}

impl ExecuteState {
    pub fn new() -> ExecuteState {
        ExecuteState {
            processors: Vec::new(),
            sessions: HashMap::new(),
        }
    }
}

/// Executed FSM in separate threads.
/// This class maintains IO Processors used by the FSMs and running sessions.
#[derive(Clone)]
pub struct FsmExecutor {
    pub state: Arc<Mutex<ExecuteState>>,
    pub include_paths: Vec<PathBuf>,
}

impl FsmExecutor {
    pub fn add_processor(&mut self, processor: Box<dyn EventIOProcessor>) {
        self.state.lock().unwrap().processors.push(processor);
    }

    pub fn new_without_io_processor() -> FsmExecutor {
        let mut e = FsmExecutor {
            state: Arc::new(Mutex::new(ExecuteState::new())),
            include_paths: Vec::new(),
        };
        e.add_processor(Box::new(ScxmlEventIOProcessor::new()));
        e
    }

    pub async fn new_with_io_processor() -> FsmExecutor {
        let mut e = FsmExecutor {
            state: Arc::new(Mutex::new(ExecuteState::new())),
            include_paths: Vec::new(),
        };
        #[cfg(feature = "BasicHttpEventIOProcessor")]
        {
            let w = Box::new(
                BasicHTTPEventIOProcessor::new(
                    IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
                    "localhost",
                    5555,
                )
                .await,
            );
            e.add_processor(w);
        }
        e.add_processor(Box::new(ScxmlEventIOProcessor::new()));
        e
    }

    #[cfg(feature = "xml")]
    pub fn set_include_paths_from_arguments(
        &mut self,
        named_arguments: &HashMap<&'static str, String>,
    ) {
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
            if let Some(mut pp) = guard.processors.pop() {
                pp.shutdown();
            }
        }
    }

    /// Loads and starts the specified FSM.
    pub fn execute(
        &mut self,
        uri: &str,
        #[cfg(feature = "Trace")] trace: TraceMode,
    ) -> Result<ScxmlSession, String> {
        self.execute_with_data(
            uri,
            &Vec::new(),
            None,
            &"".to_string(),
            #[cfg(feature = "Trace")]
            trace,
        )
    }

    /// Loads and starts the specified FSM with some data set.
    pub fn execute_with_data(
        &mut self,
        uri: &str,
        data: &[ParamPair],
        parent: Option<SessionId>,
        invoke_id: &InvokeId,
        #[cfg(feature = "Trace")] trace: TraceMode,
    ) -> Result<ScxmlSession, String> {
        info!("Loading FSM from {}", uri);

        // Use reader to parse the scxml file:
        #[cfg(feature = "xml")]
        let sm = scxml_reader::parse_from_uri(uri.to_string(), &self.include_paths);

        #[cfg(not(feature = "xml"))]
        let sm = Ok(Box::new(Fsm::new()));

        match sm {
            Ok(mut fsm) => {
                #[cfg(feature = "Trace")]
                fsm.tracer.enable_trace(trace);
                fsm.caller_invoke_id = Some(invoke_id.clone());
                fsm.parent_session_id = parent;
                let session = fsm::start_fsm_with_data(fsm, Box::new(self.clone()), data);
                Ok(session)
            }
            Err(message) => Err(message),
        }
    }

    /// Loads and starts the specified FSM with some data set.
    pub fn execute_with_data_from_xml(
        &mut self,
        xml: &str,
        data: &[ParamPair],
        parent: Option<SessionId>,
        invoke_id: &InvokeId,
        finish_mode: FinishMode,
        #[cfg(feature = "Trace")] trace: TraceMode,
    ) -> Result<ScxmlSession, String> {
        info!("Loading FSM from XML");

        // Use reader to parse the XML:
        #[cfg(feature = "xml")]
        let sm = scxml_reader::parse_from_xml_with_includes(xml.to_string(), &self.include_paths);
        #[cfg(not(feature = "xml"))]
        let sm = Ok(Box::new(Fsm::new()));

        match sm {
            Ok(mut fsm) => {
                #[cfg(feature = "Trace")]
                fsm.tracer.enable_trace(trace);
                fsm.caller_invoke_id = Some(invoke_id.clone());
                fsm.parent_session_id = parent;
                let session = fsm::start_fsm_with_data_and_finish_mode(
                    fsm,
                    Box::new(self.clone()),
                    data,
                    finish_mode,
                );
                Ok(session)
            }
            Err(message) => Err(message),
        }
    }

    /// Called by FSM after session ends and FinishMode::DISPOSE.
    pub fn remove_session(&mut self, session_id: SessionId) {
        self.state.lock().unwrap().sessions.remove(&session_id);
    }

    pub fn get_session_sender(&self, session_id: SessionId) -> Option<Sender<Box<Event>>> {
        Some(
            self.state
                .lock()
                .unwrap()
                .sessions
                .get(&session_id)?
                .sender
                .clone(),
        )
    }

    pub fn send_to_session(
        &self,
        session_id: SessionId,
        event: Event,
    ) -> Result<(), SendError<Box<Event>>> {
        match self.get_session_sender(session_id) {
            None => {
                todo!("Handling of unknown session")
            }
            Some(sender) => sender.send(Box::new(event)),
        }
    }
}
