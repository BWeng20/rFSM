//! I/O Processor implementation for type "http://www.w3.org/TR/scxml/#BasicHTTPEventProcessor".
//! Included if feature "BasicHttpEventIOProcessor" is enabled.\
//! See [W3C:SCXML - Basic HTTP Event I/O Processor](/doc/W3C_SCXML_2024_07_13/index.html#BasicHTTPEventProcessor).

use std::borrow::Cow;
use std::collections::HashMap;
use std::convert::Infallible;
use std::fmt::Debug;
use std::net::SocketAddr;
use std::net::{IpAddr, TcpStream};
use std::ops::Deref;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::sync::mpsc::channel;
use std::sync::{atomic::AtomicBool, Arc, Mutex};
use std::thread;
#[cfg(test)]
use std::{println as debug, println as info, println as error};

use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response};
use hyper_util::rt::TokioIo;
#[cfg(not(test))]
use log::{debug, error, info};
use tokio::net::TcpListener;

use crate::datamodel::{GlobalDataAccess, BASIC_HTTP_EVENT_PROCESSOR};
use crate::event_io_processor::{EventIOProcessor, EventIOProcessorHandle};
use crate::fsm::{Event, SessionId};

pub const SCXML_EVENT_NAME: &str = "_scxmleventname";

/// IO Processor to server basic http request. \
/// See /doc/W3C_SCXML_2024_07_13/index.html#BasicHTTPEventProcessor \
/// If the feature is active, this IO Processor is automatically added by FsmExecutor.
#[derive(Debug, Clone)]
pub struct BasicHTTPEventIOProcessor {
    pub terminate_flag: Arc<AtomicBool>,
    pub state: Arc<Mutex<BasicHTTPEventIOProcessorServerData>>,
    pub handle: EventIOProcessorHandle,
}

#[derive(Debug, Clone)]
pub struct BasicHTTPEventIOProcessorServerData {
    pub location: String,
    pub local_adr: SocketAddr,
}

/// The parsed payload of a http request
#[derive(Debug, Clone)]
struct Message {
    pub event: String,
    pub session: String,
}

/// Event processed by the message thread of the processor.
#[derive(Debug, Clone)]
enum BasicHTTPEvent {
    /// A http request was parsed and shall to be executed by the target fsm.
    Message(Message),
}

impl BasicHTTPEvent {
    /// Parse a Http request and created  the resulting message to the message thread.
    pub async fn from_request(
        request: hyper::Request<hyper::body::Incoming>,
    ) -> Result<BasicHTTPEvent, hyper::StatusCode> {
        let (parts, body) = request.into_parts();
        debug!("Method {:?}", parts.method);
        debug!("Header {:?}", parts.headers);
        debug!("Uri {:?}", parts.uri);

        let mut path = parts.uri.path().to_string();

        // Path without leading "/" addresses the session to notify.
        if path.starts_with("/") {
            path.remove(0);
        }
        debug!("Path {:?}", path);
        if path.is_empty() {
            error!("Missing Session Path");
            return Err(hyper::StatusCode::BAD_REQUEST);
        }

        let query_params: HashMap<Cow<str>, Cow<str>>;
        let db;

        match parts.method {
            hyper::Method::POST => {
                // Mandatory POST implementation
                match body.collect().await {
                    Ok(data) => {
                        db = data.to_bytes();
                        query_params = form_urlencoded::parse(db.as_ref()).collect();
                    }
                    Err(_e) => {
                        return Err(hyper::StatusCode::BAD_REQUEST);
                    }
                }
            }
            hyper::Method::GET => {
                // Optional GET implementation
                query_params = match parts.uri.query() {
                    None => HashMap::new(),
                    Some(query_s) => form_urlencoded::parse(query_s.as_bytes()).collect(),
                };
            }
            _ => {
                return Err(hyper::StatusCode::BAD_REQUEST);
            }
        }

        debug!("Query Parameters {:?}", query_params);

        let event_name = match query_params.get(SCXML_EVENT_NAME) {
            None => "",
            Some(event_name) => {
                debug!("Event Name {:?}", event_name);
                event_name
            }
        };

        let msg = Message {
            event: event_name.to_string(),
            session: path,
        };
        Ok(BasicHTTPEvent::Message(msg))
    }
}

async fn handle_request(
    req: Request<hyper::body::Incoming>,
    tx: mpsc::Sender<Box<BasicHTTPEvent>>,
) -> Result<Response<Full<Bytes>>, Infallible> {
    debug!("Serve {:?}", req);

    let rs = async { BasicHTTPEvent::from_request(req).await }.await;
    return match rs {
        Ok(event) => {
            let sr = tx.send(Box::new(event));
            match sr {
                Ok(_) => {
                    debug!("SendOk");
                    Ok(hyper::Response::builder()
                        .status(hyper::StatusCode::OK)
                        .body(Full::new(Bytes::from("Ok")))
                        .unwrap())
                }
                Err(error) => {
                    debug!("SendError {:?}", error);
                    Ok(hyper::Response::builder()
                        .status(hyper::StatusCode::INTERNAL_SERVER_ERROR)
                        .body(Full::new(Bytes::from(error.to_string())))
                        .unwrap())
                }
            }
        }
        Err(status) => Ok(hyper::Response::builder()
            .status(status.clone())
            .body(Full::new(Bytes::from("Error".to_string())))
            .unwrap()),
    };
}

impl BasicHTTPEventIOProcessor {
    pub async fn new(ip_addr: IpAddr, location_name: &str, port: u16) -> BasicHTTPEventIOProcessor {
        let terminate_flag = Arc::new(AtomicBool::new(false));

        let addr = SocketAddr::new(ip_addr, port);

        info!("HTTP server starting");

        let inner_terminate_flag = terminate_flag.clone();
        let (sender, receiver_server) = channel::<Box<BasicHTTPEvent>>();

        let _thread_message_server = thread::spawn(move || {
            let mut c = 0;
            debug!("Message server started");
            while !inner_terminate_flag.load(Ordering::Relaxed) {
                let event_opt = receiver_server.recv();
                c = c + 1;
                match event_opt {
                    Ok(event) => {
                        match event.deref() {
                            BasicHTTPEvent::Message(message) => {
                                debug!("BasicHTTPEvent:Message #{} {:?}", c, message);
                                // TODO: Sending event to session
                            }
                        }
                    }
                    Err(_err) => {
                        debug!("Message server channel disconnected");
                        break;
                    }
                }
            }
            debug!("Message server stopped");
        });

        let listener_result = TcpListener::bind(addr).await;

        let server = listener_result.unwrap();

        let _thread_server = tokio::task::spawn(async move {
            loop {
                let (stream, _addr) = server.accept().await.unwrap();
                let io = TokioIo::new(stream);

                let tx1 = sender.clone();

                tokio::task::spawn(async move {
                    let tx2 = tx1.clone();
                    let builder = http1::Builder::new();
                    let conn = builder.serve_connection(
                        io,
                        service_fn(move |request| handle_request(request, tx2.clone())),
                    );

                    let r = conn.await;
                    match r {
                        Ok(_) => {}
                        Err(err) => {
                            eprintln!("Error serving connection: {:?}", err);
                        }
                    }
                });
            }
        });

        debug!("BasicHTTPServer at {:?}", addr);

        let state = BasicHTTPEventIOProcessorServerData {
            location: format!("https://{}:{}", location_name, port),
            local_adr: addr,
        };
        let e = BasicHTTPEventIOProcessor {
            terminate_flag: terminate_flag,
            state: Arc::new(Mutex::new(state)),
            handle: EventIOProcessorHandle::new(),
        };
        e
    }
}

const TYPES: &[&str] = &[BASIC_HTTP_EVENT_PROCESSOR, "http"];

impl EventIOProcessor for BasicHTTPEventIOProcessor {
    fn get_location(&self, id: SessionId) -> String {
        format!("{}/{}", self.state.lock().unwrap().location, id)
    }

    /// Returns the type of this processor.
    fn get_types(&self) -> &[&str] {
        TYPES
    }

    fn get_handle(&mut self) -> &mut EventIOProcessorHandle {
        &mut self.handle
    }

    fn get_copy(&self) -> Box<dyn EventIOProcessor> {
        let b = BasicHTTPEventIOProcessor {
            terminate_flag: self.terminate_flag.clone(),
            state: self.state.clone(),
            handle: self.handle.clone(),
        };
        Box::new(b)
    }

    fn send(&mut self, _global: &GlobalDataAccess, _target: &String, _event: Event) {
        // W3C basic html processor:
        // If neither the 'target' nor the 'targetexpr' attribute is specified, the SCXML Processor must add the event error.communication to the internal event queue of the sending session.

        todo!()
    }

    fn shutdown(&mut self) {
        info!("HTTP Event IO Processor shutdown...");
        self.terminate_flag.as_ref().store(true, Ordering::Relaxed);
        let _ = TcpStream::connect(self.state.lock().unwrap().local_adr);
        self.handle.shutdown();
    }
}
