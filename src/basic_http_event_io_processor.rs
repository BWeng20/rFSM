#[cfg(test)]
use std::{println as debug, println as info, println as error};
use std::borrow::Cow;
use std::collections::HashMap;
use std::convert::Infallible;
use std::fmt::Debug;
use std::net::{IpAddr, TcpStream};
use std::net::SocketAddr;
use std::ops::Deref;
use std::sync::{Arc, atomic::AtomicBool, Mutex};
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::sync::mpsc::channel;
use std::thread;

use http_body_util::{BodyExt, Full};
use hyper::{Request, Response};
use hyper::body::Bytes;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper_util::rt::TokioIo;
#[cfg(not(test))]
use log::{debug, error, info};
use tokio::net::TcpListener;

use crate::datamodel::BASIC_HTTP_EVENT_PROCESSOR;
use crate::event_io_processor::{EventIOProcessor, EventIOProcessorHandle};
use crate::fsm::EventSender;

pub const SCXML_EVENT_NAME: &str = "_scxmleventname";

/// IO Processor to server basic http request. \
/// See https://www.w3.org/TR/scxml/#BasicHTTPEventProcessor \
/// If the feature is active, this IO Processor is automatically added by FsmExecutor.
#[derive(Debug, Clone)]
pub struct BasicHTTPEventIOProcessor {
    pub terminate_flag: Arc<AtomicBool>,
    pub state: Arc<Mutex<BasicHTTPEventIOProcessorServerData>>,
}

#[derive(Debug, Clone)]
pub struct BasicHTTPEventIOProcessorServerData {
    pub location: String,
    pub local_adr: SocketAddr,
    pub fsms: HashMap<String, EventSender>,
}

/// The parsed payload of a http request
#[derive(Debug)]
struct Message {
    pub event: String,
    pub session: String,
}

/// Event processed by the message thread of the processor.
#[derive(Debug)]
enum BasicHTTPEvent {
    /// A http request was parsed and shall to be executed by the target fsm.
    Message(Message),

    /// Informs the message thread about a new FSM in the system.
    NewFsm(String, EventSender),
}

impl BasicHTTPEvent {
    /// Parse a Http request and created  the resulting message to the message thread.
    pub async fn from_request(request: hyper::Request<hyper::body::Incoming>) -> Result<BasicHTTPEvent, hyper::StatusCode> {
        let (parts, body) = request.into_parts();
        debug!("Method {:?}", parts.method);
        debug!("Header {:?}", parts.headers );
        debug!("Uri {:?}", parts.uri );

        let mut path = parts.uri.path().to_string();

        // Path without leading "/" addresses the session to notify.
        if path.starts_with("/") {
            path.remove(0);
        }
        debug!("Path {:?}", path );
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
                query_params =
                    match parts.uri.query() {
                        None => {
                            HashMap::new()
                        }
                        Some(query_s) => {
                            form_urlencoded::parse(query_s.as_bytes()).collect()
                        }
                    };
            }
            _ => {
                return Err(hyper::StatusCode::BAD_REQUEST);
            }
        }

        debug!("Query Parameters {:?}", query_params );

        let event_name =
            match query_params.get(SCXML_EVENT_NAME) {
                None => {
                    ""
                }
                Some(event_name) => {
                    debug!("Event Name {:?}", event_name );
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

async fn handle_request(req: Request<hyper::body::Incoming>, tx: mpsc::Sender<Box<BasicHTTPEvent>>) -> Result<Response<Bytes>, Infallible> {
    debug!("Serve {:?}", req );

    let rs =
        async {
            BasicHTTPEvent::from_request(req).await
        }.await;
    return match rs {
        Ok(event) => {
            let sr = tx.send(Box::new(event));
            match sr {
                Ok(_) => {
                    debug!("SendOk");
                    Ok(hyper::Response::builder().status(hyper::StatusCode::OK).body(Bytes::from("Ok")).unwrap())
                }
                Err(error) => {
                    debug!("SendError {:?}", error);
                    Ok(hyper::Response::builder().status(hyper::StatusCode::INTERNAL_SERVER_ERROR).body(Bytes::from(error.to_string())).unwrap())
                }
            }
        }
        Err(status) => {
            Ok(hyper::Response::builder().status(status.clone()).body(Bytes::from("Error".to_string())).unwrap())
        }
    };
}

async fn hello(_: Request<hyper::body::Incoming>) -> Result<Response<Full<Bytes>>, Infallible> {
    Ok(Response::new(Full::new(Bytes::from("Hello, World!"))))
}

impl BasicHTTPEventIOProcessor {
    pub async fn new(ip_addr: IpAddr, location_name: &str, port: u16) -> BasicHTTPEventIOProcessor {
        let terminate_flag = Arc::new(AtomicBool::new(false));

        let addr = SocketAddr::new(ip_addr, port);

        info!("HTTP server starting");

        let inner_terminate_flag = terminate_flag.clone();
        let (_sender, receiver_server) = channel::<Box<BasicHTTPEvent>>();

        let _thread_message_server =
            thread::spawn(move || {
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
                                BasicHTTPEvent::NewFsm(name, sender) => {
                                    debug!("BasicHTTPEvent:NewFsm #{} {} {:?}", c, name, sender);
                                    // TODO: Adding fsm to list
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

        let _thread_server =
            tokio::task::spawn(async move {
                loop {
                    let (stream, _addr) = server.accept().await.unwrap();
                    let io = TokioIo::new(stream);


                    tokio::task::spawn(async move {
                        /*
                        let mut serv = service_fn(
                            move |request| {
                                let tx = sender.clone();
                                handle_request(request, tx)
                            }
                        );
*/
                        // Finally, we bind the incoming connection to our `hello` service
                        if let Err(err) = http1::Builder::new().serve_connection(io, service_fn(hello))
                            .await
                        {
                            eprintln!("Error serving connection: {:?}", err);
                        }
                    });
                }
            });

        debug!("BasicHTTPServer at {:?}", addr );

        let state = BasicHTTPEventIOProcessorServerData {
            location: format!("https://{}:{}", location_name, port),
            local_adr: addr,
            fsms: HashMap::new(),
        };
        let e = BasicHTTPEventIOProcessor
        {
            terminate_flag: terminate_flag,
            state: Arc::new(Mutex::new(state)),
        };
        e
    }
}

const TYPES: &[&str] = &[BASIC_HTTP_EVENT_PROCESSOR, "http"];

impl EventIOProcessor for BasicHTTPEventIOProcessor {
    fn get_location(&self) -> String {
        self.state.lock().unwrap().location.clone()
    }

    /// Returns the type of this processor.
    fn get_types(&self) -> &[&str] { TYPES }

    fn get_handle(&mut self) -> &mut EventIOProcessorHandle {
        todo!()
    }

    fn get_copy(&self) -> Box<dyn EventIOProcessor> {
        let b = BasicHTTPEventIOProcessor {
            terminate_flag: self.terminate_flag.clone(),
            state: self.state.clone(),
        };
        Box::new(b)
    }

    fn shutdown(&mut self) {
        info!("HTTP Event IO Processor shutdown...");
        self.terminate_flag.as_ref().store(true, Ordering::Relaxed);
        let _ = TcpStream::connect(self.state.lock().unwrap().local_adr);
    }
}
