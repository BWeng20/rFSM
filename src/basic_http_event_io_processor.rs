use std::{
    io::{BufReader, prelude::*},
    net::TcpListener,
};
use std::collections::HashMap;
use std::fmt::Debug;
use std::net::{SocketAddr, TcpStream};
use std::sync::{Arc, atomic::AtomicBool, mpsc::Sender};
use std::sync::atomic::Ordering;
use std::thread;
use std::thread::JoinHandle;

use log::{debug, info, log_enabled};

use crate::datamodel::BASIC_HTTP_EVENT_PROCESSOR;
/// See https://www.w3.org/TR/scxml/#BasicHTTPEventProcessor

use crate::event_io_processor::{EventIOProcessor, EventIOProcessorHandle};
use crate::fsm::Event;

#[derive(Debug)]
pub struct BasicHTTPEventIOProcessor {
    pub location: String,
    pub server_thread: Option<JoinHandle<()>>,
    pub terminate_flag: Arc<AtomicBool>,
    pub local_adr: SocketAddr,
    pub fsms: HashMap<String, Sender<Box<Event>>>,
}

impl BasicHTTPEventIOProcessor {
    pub fn new(addr: &SocketAddr) -> BasicHTTPEventIOProcessor {
        let listener = TcpListener::bind(addr).unwrap();
        let local_addr = listener.local_addr().unwrap();
        let terminate_flag = Arc::new(AtomicBool::new(false));

        let inner_terminate_flag = terminate_flag.clone();

        let thread = thread::Builder::new().name("fsm_http_io_proc".to_string()).spawn(
            move || {
                info!("HTTP Event IO Processor starting...");
                {
                    loop {
                        let accepted = listener.accept();
                        match accepted {
                            Ok((stream, _addr)) => {
                                if inner_terminate_flag.load(Ordering::Relaxed)
                                {
                                    info!("Terminating HTTP Event IO Processor");
                                    break;
                                }
                                debug!("Connection from {:?}", stream.peer_addr());

                                let buf_reader = BufReader::new(stream);

                                if log_enabled!(log::Level::Debug) {
                                    debug!("Request:");
                                    for line in buf_reader.lines() {
                                        debug!(" {}", line.unwrap());
                                    }
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

        let e = BasicHTTPEventIOProcessor
        {
            location: "https://localhost:5555".to_string(),
            server_thread: Some(thread.unwrap()),
            terminate_flag: terminate_flag,
            fsms: HashMap::new(),
            local_adr: local_addr,
        };
        e
    }
}


impl EventIOProcessor for BasicHTTPEventIOProcessor {
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
        let b = BasicHTTPEventIOProcessor {
            location: self.location.clone(),
            server_thread: None,
            terminate_flag: self.terminate_flag.clone(),
            fsms: self.fsms.clone(),
            local_adr: self.local_adr.clone(),
        };
        Box::new(b)
    }

    fn shutdown(&mut self) {
        info!("HTTP Event IO Processor shutdown...");
        self.terminate_flag.as_ref().store(true, Ordering::Relaxed);
        let _ = TcpStream::connect(self.local_adr);
        let t = std::mem::replace(&mut self.server_thread, None);
        match t
        {
            Some(t) => {
                _ = t.join();
            }
            _ => {}
        }
    }
}