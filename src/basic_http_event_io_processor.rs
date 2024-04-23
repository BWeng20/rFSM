use std::borrow::Cow;
use std::collections::HashMap;
use std::convert::Infallible;
use std::fmt::Debug;
use std::net::{SocketAddr, TcpStream};
use std::sync::{Arc, atomic::AtomicBool, mpsc::Sender};
use std::sync::atomic::Ordering;

use http_body_util::Full;
use hyper::{Request, Response};
use hyper::body::Bytes;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper_util::rt::TokioIo;
use log::{error, info};
use tokio::net::TcpListener;

use crate::datamodel::BASIC_HTTP_EVENT_PROCESSOR;
/// See https://www.w3.org/TR/scxml/#BasicHTTPEventProcessor

use crate::event_io_processor::{EventIOProcessor, EventIOProcessorHandle};
use crate::fsm::Event;

pub const SCXML_EVENT_NAME: &str = "_scxmleventname";

#[derive(Debug)]
pub struct BasicHTTPEventIOProcessor {
    pub location: String,
    pub terminate_flag: Arc<AtomicBool>,
    pub local_adr: SocketAddr,
    pub fsms: HashMap<String, Sender<Box<Event>>>,
}

async fn handle_request(request: Request<hyper::body::Incoming>) -> Result<Response<Full<Bytes>>, Infallible> {
    info!("Method {:?}", request.method() );
    info!("Header {:?}", request.headers() );
    info!("Body {:?}", request.body() );
    info!("Uri {:?}", request.uri() );

    // Path without leading "/" addresses the session to notify.
    let mut path = request.uri().path().to_string();
    if path.starts_with("/") {
        path.remove(0);
    }
    info!("Path {:?}", path );

    let mut query_params: HashMap<Cow<str>, Cow<str>> =
        match request.uri().query() {
            None => {
                HashMap::new()
            }
            Some(query_s) => {
                form_urlencoded::parse(query_s.as_bytes()).collect()
            }
        };
    info!("Query Parameters {:?}", query_params );

    let event_name =
        match query_params.get(SCXML_EVENT_NAME) {
            None => {
                ""
            }
            Some(event_name) => {
                event_name
            }
        };

    info!("Event Name {:?}", event_name );

    Ok(Response::new(Full::new(Bytes::from("Hello, World!"))))
}

impl BasicHTTPEventIOProcessor {
    pub fn new(addr: &SocketAddr) -> BasicHTTPEventIOProcessor {
        let terminate_flag = Arc::new(AtomicBool::new(false));
        let inner_terminate_flag = terminate_flag.clone();
        let inner_addr = addr.clone();

        info!("HTTP server starting");

        let thread =
            tokio::task::spawn(async move {
                match TcpListener::bind(inner_addr).await {
                    Ok(listener) => {
                        info!("HTTP Event IO Processor listening at {:?}", inner_addr );

                        loop {
                            match listener.accept().await {
                                Ok((stream, _)) => {
                                    if inner_terminate_flag.load(Ordering::Relaxed) {
                                        break;
                                    }
                                    info!("HTTP accept" );
                                    let io = TokioIo::new(stream);
                                    tokio::task::spawn(async move {
                                        if let Err(err) = http1::Builder::new()
                                            // `service_fn` converts our function in a `Service`
                                            .serve_connection(io, service_fn(handle_request))
                                            .await
                                        {
                                            error!("Error serving connection: {:?}", err);
                                        }
                                    });
                                }
                                Err(err) => {
                                    error!("Error accepting connection: {:?}", err);
                                }
                            }
                        }
                        info!("HTTP server finished");
                    }
                    Err(e) => {
                        error!("HTTP Event IO Processor error {:?} listening at {:?}", e, inner_addr );
                    }
                }
            });
        let e = BasicHTTPEventIOProcessor
        {
            location: "https://localhost:5555".to_string(),
            terminate_flag,
            fsms: HashMap::new(),
            local_adr: addr.clone(),
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
    }
}