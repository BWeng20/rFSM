//! Helper module to maintain FSM sessions.\

extern crate core;

use crate::fsm::Fsm;
use std::collections::HashMap;
#[cfg(feature = "serializer")]
use std::fs::File;
#[cfg(feature = "serializer")]
use std::io::BufReader;

use std::path::PathBuf;
use std::sync::mpsc::{SendError, Sender};
use std::sync::{Arc, Mutex};

#[cfg(feature = "Debug")]
use log::debug;

use crate::actions::ActionWrapper;
#[cfg(feature = "BasicHttpEventIOProcessor")]
use crate::basic_http_event_io_processor::BasicHTTPEventIOProcessor;
use crate::datamodel::DATAMODEL_OPTION_PREFIX;
use crate::event_io_processor::EventIOProcessor;
use crate::fsm;
use crate::fsm::{Event, FinishMode, InvokeId, ParamPair, ScxmlSession, SessionId};
use crate::scxml_event_io_processor::ScxmlEventIOProcessor;
#[cfg(feature = "xml")]
use crate::scxml_reader;
#[cfg(feature = "xml")]
use crate::scxml_reader::include_path_from_arguments;
#[cfg(feature = "serializer")]
use crate::serializer::default_protocol_reader::DefaultProtocolReader;
#[cfg(feature = "serializer")]
use crate::serializer::fsm_reader::FsmReader;
#[cfg(feature = "Trace")]
use crate::tracer::TraceMode;
#[cfg(feature = "BasicHttpEventIOProcessor")]
use std::net::{IpAddr, Ipv4Addr};

#[derive(Default)]
pub struct ExecuteState {
    pub processors: Vec<Arc<Mutex<Box<dyn EventIOProcessor>>>>,
    pub sessions: HashMap<SessionId, ScxmlSession>,
    pub datamodel_options: HashMap<String, String>,
}

impl ExecuteState {
    pub fn new() -> ExecuteState {
        ExecuteState {
            processors: Vec::new(),
            sessions: HashMap::new(),
            datamodel_options: HashMap::new(),
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
        self.state
            .lock()
            .unwrap()
            .processors
            .push(Arc::new(Mutex::new(processor)));
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
                BasicHTTPEventIOProcessor::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), "localhost", 5555).await,
            );
            e.add_processor(w);
        }
        e.add_processor(Box::new(ScxmlEventIOProcessor::new()));
        e
    }

    #[cfg(feature = "xml")]
    pub fn set_include_paths_from_arguments(&mut self, named_arguments: &HashMap<&'static str, String>) {
        self.set_include_paths(&include_path_from_arguments(named_arguments));
    }

    pub fn set_global_options_from_arguments(&mut self, named_arguments: &HashMap<&str, String>) {
        let mut guard = self.state.lock().unwrap();
        // Currently only Datamodel options are relevant. Ignore all other stuff.
        for (name, value) in named_arguments {
            if let Some(datamodel_option) = name.strip_prefix(DATAMODEL_OPTION_PREFIX) {
                guard
                    .datamodel_options
                    .insert(datamodel_option.to_string(), value.clone());
            }
        }
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
            if let Some(pp) = guard.processors.pop() {
                pp.lock().unwrap().shutdown();
            }
        }
    }

    /// Loads and starts the specified FSM.
    pub fn execute(
        &mut self,
        uri: &str,
        actions: ActionWrapper,
        #[cfg(feature = "Trace")] trace: TraceMode,
    ) -> Result<ScxmlSession, String> {
        self.execute_with_data(
            uri,
            actions,
            &Vec::new(),
            None,
            &"".to_string(),
            #[cfg(feature = "Trace")]
            trace,
        )
    }

    /// Loads and starts the specified FSM with some data set.\
    /// Normally used if a child-FSM is started from a parent FSM.
    pub fn execute_with_data(
        &mut self,
        uri: &str,
        actions: ActionWrapper,
        data: &[ParamPair],
        parent: Option<SessionId>,
        invoke_id: &InvokeId,
        #[cfg(feature = "Trace")] trace: TraceMode,
    ) -> Result<ScxmlSession, String> {
        #[allow(unused_variables)]
        let extension = uri.rsplit('.').next().unwrap_or_default();

        #[allow(unused_variables)]
        #[allow(unused_mut)]
        let mut sm: Result<Box<Fsm>, String> = Err("".to_string());

        // Use reader to parse the scxml file:
        #[cfg(feature = "xml")]
        if extension.eq_ignore_ascii_case("scxml") || extension.eq_ignore_ascii_case("xml") {
            #[cfg(feature = "Debug")]
            debug!("Loading FSM from XML {}", uri);
            sm = scxml_reader::parse_from_uri(uri.to_string(), &self.include_paths);
        }

        #[cfg(feature = "serializer")]
        if extension.eq_ignore_ascii_case("rfsm") {
            #[cfg(feature = "Debug")]
            debug!("Loading FSM from binary {}", uri);
            sm = match File::open(uri) {
                Ok(f) => {
                    let protocol = DefaultProtocolReader::new(BufReader::new(f));
                    let mut reader = FsmReader::new(Box::new(protocol));
                    reader.read()
                }
                Err(err) => Err(err.to_string()),
            }
        }

        #[cfg(all(not(feature = "xml"), not(feature = "serializer")))]
        let sm = Ok(Box::new(Fsm::new()));

        match sm {
            Ok(mut fsm) => {
                #[cfg(feature = "Trace")]
                fsm.tracer.enable_trace(trace);
                fsm.caller_invoke_id = Some(invoke_id.clone());
                fsm.parent_session_id = parent;
                let session = fsm::start_fsm_with_data(fsm, actions, Box::new(self.clone()), data);
                Ok(session)
            }
            Err(message) => Err(message),
        }
    }

    /// Loads and starts the specified FSM with some data set.\
    /// Normally used if a child-FSM is started from a parent FSM, in this case via inline content.
    #[allow(clippy::too_many_arguments)]
    #[allow(unused_variables)]
    pub fn execute_with_data_from_xml(
        &mut self,
        xml: &str,
        actions: ActionWrapper,
        data: &[ParamPair],
        parent: Option<SessionId>,
        invoke_id: &InvokeId,
        finish_mode: FinishMode,
        #[cfg(feature = "Trace")] trace: TraceMode,
    ) -> Result<ScxmlSession, String> {
        #[cfg(feature = "Debug")]
        debug!("Loading FSM from XML");

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
                    actions.get_copy(),
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

    /// Gets a clone of the event-sender of the session.
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

    /// Sends some event to a session.
    pub fn send_to_session(&self, session_id: SessionId, event: Event) -> Result<(), SendError<Box<Event>>> {
        match self.get_session_sender(session_id) {
            None => {
                todo!("Handling of unknown session")
            }
            Some(sender) => sender.send(Box::new(event)),
        }
    }
}
