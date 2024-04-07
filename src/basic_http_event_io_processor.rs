use std::{
    io::{BufReader, prelude::*},
    net::TcpListener,
};
use std::collections::HashMap;
use std::fmt::Debug;
use std::net::SocketAddr;
use std::sync::{Arc, atomic::AtomicBool, mpsc::Sender};
use std::sync::atomic::Ordering;
use std::thread;
use std::thread::JoinHandle;

use log::info;

use crate::datamodel::BASIC_HTTP_EVENT_PROCESSOR;
/// See https://www.w3.org/TR/scxml/#BasicHTTPEventProcessor

use crate::event_io_processor::{EventIOProcessor, EventIOProcessorHandle};
use crate::fsm::Event;

#[derive(Debug)]
pub struct BasicHTTPEventProcessor {
    pub location: String,
    pub server_thread: Option<JoinHandle<()>>,
    pub terminate_flag: Arc<AtomicBool>,

    pub fsms: HashMap<String, Sender<Box<Event>>>,
}

impl BasicHTTPEventProcessor {

    pub fn terminate(&mut self) {
        self.terminate_flag.as_ref().store(true, Ordering::Relaxed);
    }

    pub fn new() -> BasicHTTPEventProcessor {
        let thread = thread::Builder::new().name("fsm_http_io_proc".to_string()).spawn(
            move || {
                info!("HTTP server starting...");
                {
                    let addr = SocketAddr::from(([127, 0, 0, 1], 5555));
                    let listener = TcpListener::bind(addr).unwrap();

                    loop {
                        let accepted = listener.accept();
                        match accepted {
                            Ok((stream, _addr)) => {
                                println!("Connection from {:?}", stream.peer_addr());

                                let buf_reader = BufReader::new(stream);
                                println!("Request:");
                                for line in buf_reader.lines() {
                                    println!(" {}", line.unwrap());
                                }
                            }
                            Err(_) => {
                                break;
                            }
                        }
                    }
                }
                info!("HTTP server finished");
            });

        let e = BasicHTTPEventProcessor
        {
            location: "https://localhost:5555".to_string(),
            server_thread: Some(thread.unwrap()),
            terminate_flag: Arc::new(AtomicBool::new(false)),
            fsms: HashMap::new(),
        };
        e
    }
}


impl EventIOProcessor for BasicHTTPEventProcessor {
    fn get_location(&self) -> &str {
        self.location.as_str()
    }

    /// Returns the type of this processor.
    fn get_type(&self) -> &str {
        BASIC_HTTP_EVENT_PROCESSOR
    }

    fn get_handle(&mut self) -> &mut EventIOProcessorHandle {
        todo!()
    }

    fn get_copy(&self) -> Box<dyn EventIOProcessor> {
        let b = BasicHTTPEventProcessor {
            location: self.location.clone(),
            server_thread: None,
            terminate_flag: self.terminate_flag.clone(),
            fsms: self.fsms.clone(),
        };
        Box::new(b)
    }
}