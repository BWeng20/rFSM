use crate::common::ArgOption;
use crate::fsm::{Event, State};
use crate::tracer::{set_tracer_factory, TraceMode, Tracer, TracerFactory};
use std::fmt::{Debug, Display};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio::task;

pub static TRACE_SERVER_ARGUMENT_OPTION: ArgOption = ArgOption {
    name: "traceserver",
    with_value: true,
    required: false,
};

pub async fn run_trace_server(address: &str) -> tokio::task::JoinHandle<()> {
    let address_clone = address.to_string();

    set_tracer_factory(Box::new(RemoteTracerFactory {}));

    tokio::task::spawn(async move {
        let listener = TcpListener::bind(address_clone.as_str()).await;

        if let Ok(listener) = listener {
            println!("Trace Server runs on {}", address_clone);

            loop {
                match listener.accept().await {
                    Ok((socket, _)) => {
                        task::spawn(handle_connection(socket));
                    }
                    Err(err) => {
                        println!("Connection aborted: {}", err);
                        break;
                    }
                };
            }
        }
    })
}

async fn handle_connection(socket: TcpStream) {
    let (mut reader, mut writer) = socket.into_split();
    let (tx, mut rx) = mpsc::channel(32);

    // Task zum Lesen von Nachrichten vom Client
    let tx_clone = tx.clone();
    task::spawn(async move {
        let mut buf = vec![0; 1024];
        loop {
            match reader.read(&mut buf).await {
                Ok(0) => break, // Verbindung geschlossen
                Ok(n) => {
                    let msg = String::from_utf8_lossy(&buf[..n]);
                    println!("Empfangen: {}", msg);
                    tx_clone.send(msg.to_string()).await.unwrap();
                }
                Err(_) => break,
            }
        }
    });

    // Task zum Senden von Nachrichten an den Client
    task::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if writer.write_all(msg.as_bytes()).await.is_err() {
                break;
            }
        }
    });
}

#[derive(Debug)]
pub struct RemoteTrace {}

impl RemoteTrace {
    pub fn new() -> RemoteTrace {
        RemoteTrace {}
    }
}

impl Default for RemoteTrace {
    fn default() -> Self {
        Self::new()
    }
}

// TODO: Remove this if finished
#[allow(unused_variables)]
impl Tracer for RemoteTrace {
    fn trace(&self, msg: &str) {}

    /// Enter a sub-scope, e.g. by increase the log indentation.
    fn enter(&self) {}

    /// Leave the current sub-scope, e.g. by decrease the log indentation.
    fn leave(&self) {}

    /// Enable traces for the specified scope.
    fn enable_trace(&mut self, flag: TraceMode) {}

    /// Disable traces for the specified scope.
    fn disable_trace(&mut self, flag: TraceMode) {}

    /// Return true if the given scape is enabled.
    fn is_trace(&self, flag: TraceMode) -> bool {
        true
    }

    fn enter_method(&self, what: &str) {
        if self.is_trace(TraceMode::METHODS) {
            todo!()
        }
    }

    fn exit_method(&self, what: &str) {
        if self.is_trace(TraceMode::METHODS) {
            todo!()
        }
    }

    fn event_internal_send(&self, what: &Event) {
        if self.is_trace(TraceMode::EVENTS) {
            todo!()
        }
    }

    /// Called by FSM if an internal event is received
    fn event_internal_received(&self, what: &Event) {
        if self.is_trace(TraceMode::EVENTS) {
            todo!()
        }
    }

    /// Called by FSM if an external event is send
    fn event_external_send(&self, what: &Event) {
        if self.is_trace(TraceMode::EVENTS) {
            todo!()
        }
    }

    /// Called by FSM if an external event is received
    fn event_external_received(&mut self, what: &Event) {
        if self.is_trace(TraceMode::EVENTS) {
            todo!()
        }
    }

    /// Called by FSM if a state is entered or left.
    fn trace_state(&self, what: &str, s: &State) {
        if self.is_trace(TraceMode::STATES) {
            todo!()
        }
    }

    /// Called by FSM if a state is entered. Calls [Tracer::trace_state].
    fn trace_enter_state(&self, s: &State) {
        todo!()
    }

    /// Called by FSM if a state is left. Calls [Tracer::trace_state].
    fn trace_exit_state(&self, s: &State) {
        todo!()
    }

    /// Called by FSM for input arguments in methods.
    fn trace_argument(&self, what: &str, d: &dyn Display) {
        if self.is_trace(TraceMode::ARGUMENTS) {
            todo!()
        }
    }

    /// Called by FSM for results in methods.
    fn trace_result(&self, what: &str, d: &dyn Display) {
        if self.is_trace(TraceMode::RESULTS) {
            todo!()
        }
    }

    /// Get trace mode
    fn trace_mode(&self) -> TraceMode {
        TraceMode::ALL
    }
}

pub struct RemoteTracerFactory {}

impl TracerFactory for RemoteTracerFactory {
    fn create(&mut self) -> Box<dyn Tracer> {
        println!("Create RemoteTrace");
        Box::new(RemoteTrace::new())
    }
}
