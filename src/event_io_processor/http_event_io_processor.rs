//! I/O Processor implementation for type "http://www.w3.org/TR/scxml/#BasicHTTPEventProcessor".
//! Included if feature "BasicHttpEventIOProcessor" is enabled.\
//! See [W3C:SCXML - Basic HTTP Event I/O Processor](/doc/W3C_SCXML_2024_07_13/index.html#BasicHTTPEventProcessor).

use rocket::response::content::RawHtml;
use rocket::{Config, routes};
use rocket::{post, Shutdown};
use rocket::{route, Request, Response};

use std::collections::HashMap;
use std::fmt::Debug;
use std::net::IpAddr;
#[cfg(test)]
use std::{println as debug, println as info, println as error};

#[cfg(not(test))]
use log::{debug, error, info};
use rocket::http::ContentType;
use rocket::response::Responder;

use crate::datamodel::{GlobalDataArc, BASIC_HTTP_EVENT_PROCESSOR, Data};
use crate::event_io_processor::{EventIOProcessor, ExternalQueueContainer};
use crate::fsm::{Event, ParamPair, SessionId};
use crate::fsm_executor::ExecutorStateArc;

pub const SCXML_EVENT_NAME: &str = "_scxmleventname";

/// IO Processor to server basic http request. \
/// See /doc/W3C_SCXML_2024_07_13/index.html#BasicHTTPEventProcessor \
/// If the feature is active, this IO Processor is automatically added by FsmExecutor.
#[derive(Debug, Clone)]
pub struct BasicHTTPEventIOProcessor {
    pub shutdown_guard: Shutdown,
    pub location: String,
    pub queues: ExternalQueueContainer,
    pub executor_state: ExecutorStateArc,
}

/// The parsed payload of a http request
#[derive(Debug, Clone)]
#[allow(unused)]
struct Message {
    pub event: String,
    pub session: SessionId,
}

#[post("/scxml/<sessionid>", data = "<params>")]
fn rocket_receive_event(
    sessionid: u32,
    params: rocket::form::Form<HashMap<String, String>>,
    executor_state: &rocket::State<ExecutorStateArc>,
) -> (rocket::http::Status, String) {
    let form_data = params.into_inner();

    let event_name = match form_data.get(SCXML_EVENT_NAME) {
        None => {
            return (
                rocket::http::Status::BadRequest,
                format!("Missing argument {}", SCXML_EVENT_NAME),
            );
        }
        Some(v) => v.clone(),
    };


    match executor_state.arc.lock() {
        Ok(state) => match state.sessions.get(&sessionid) {
            None => (
                rocket::http::Status::BadRequest,
                format!("Session {} not found", sessionid),
            ),
            Some(scxml_session) => {
                let mut event = Event::new_external();

                for (name, value) in form_data {
                    match name.as_str() {
                        SCXML_EVENT_NAME => {
                            event.name = value;
                        }
                        _ => {
                            if event.param_values.is_none() {
                                event.param_values = Some(Vec::new());
                            }
                            let pair = ParamPair{ name, value: Data::String(value)};
                            event.param_values.as_mut().unwrap().push(pair);
                        }
                    }
                }
                debug!("Sending HTTP Event '{}' [{:?}]", event, event.param_values);
                match scxml_session.sender.send(Box::new(event)) {
                    Ok(_) => {
                        (rocket::http::Status::Ok, "Event send".to_string())
                    }
                    Err(err) => {
                        error!("Failed to Send Event {}. {}", event_name, err);
                        (
                            rocket::http::Status::InternalServerError,
                            "Can't send".to_string(),
                        )
                    }
                }
            }
        },
        Err(_) => {
            error!("Can't send {} because lock failed.", event_name);
            (
                rocket::http::Status::InternalServerError,
                "Can't lock".to_string(),
            )
        }
    }
}

fn escape_html(text: &str) -> String {
    // Possibly not the optimized way, but easy to understand and without any dependencies

    let mut etxt = String::with_capacity(text.len() * 2);

    for c in text.chars() {
        match c {
            '&' => etxt.push_str("&amp;"),
            '>' => etxt.push_str("&gt;"),
            '<' => etxt.push_str("&lt;"),
            // Additional for attribute content:
            '"' => etxt.push_str("&quot"),
            '\'' => etxt.push_str("&#39;"),
            _ => etxt.push(c),
        };
    }
    etxt
}

struct ImageResponse {
    content_type: ContentType,
    data: &'static [u8],
}

impl<'r> Responder<'r, 'static> for ImageResponse {
    fn respond_to(self, _r: &'r Request<'_>) -> rocket::response::Result<'static> {
        Response::build()
            .header(self.content_type)
            .sized_body(self.data.len(), std::io::Cursor::new(self.data))
            .ok()
    }
}

#[route(GET, uri = "/favicon.svg")]
fn rocket_get_favicon() -> ImageResponse {
    let facicon = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"no\"?>
<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"64\" height=\"64\" version=\"1.1\">
<rect x=\"0\" y=\"0\" width=\"64\" height=\"64\" fill=\"#F2E292\"/>
<g><text font-family=\"Arial\" font-size=\"37\" x=\"3\" y=\"38\" stroke=\"lightgray\" fill=\"#60605D\" \
stroke-width=\"0.2\">fsm</text></g></svg>";
    ImageResponse {
        content_type: ContentType::SVG,
        data: facicon.as_bytes(),
    }
}

#[route(GET, uri = "/")]
fn rocket_welcome(execute_state: &rocket::State<ExecutorStateArc>) -> RawHtml<String> {
    let mut sessions = String::with_capacity(100);

    if let Ok(es) =execute_state.lock() {
        for k in es.sessions.keys() {
            sessions.push_str("<option value='");
            sessions.push_str(&k.to_string());
            sessions.push_str("'>");
            match es.sessions.get(k).unwrap().global_data.try_lock() {
                Ok(gd) => {
                    if let Some(s) = &gd.source {
                        sessions.push_str(&escape_html(s.as_str()));
                        sessions.push_str("</option>");
                    }
                }
                Err(_) => {
                    // Ignore
                }
            };
        }
    }

    let mut page_source = String::new();

    page_source.push_str("\
  <html><head>
  <title>Finite State Machine - Basic HTTP IO Processor</title>
  <link rel='shortcut icon' href='/favicon.svg' type='image/svg+xml'>
  </head>
  <style>body{font-family:Helvetica;}.x{ font-size: 1.2em;}</style>
  <script>
    async function submitEvent(event)
    {
        event.preventDefault();
        const formData = new URLSearchParams();
        const eventName = document.getElementById('eventName').value.trim();
        if (eventName.length == 0) {
           document.getElementById('eventName').style.backgroundColor = '#AA0000';
        }
        const sessionId = document.getElementById('sessionId').value.trim();
        if (sessionId.length == 0) {
           document.getElementById('sessionId').style.backgroundColor = '#AA0000';
        }

        if (sessionId.length > 0 && eventName.length > 0) {
            document.getElementById('eventName').style.backgroundColor = null;
            document.getElementById('sessionId').style.backgroundColor = null;
            formData.append('_scxmleventname', eventName );
            const url = '/scxml/' + encodeURIComponent(sessionId);
            try {
                let response = await fetch( url, {
                   method: 'POST',
                   headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
                   body: formData.toString() }
                );
                let result = await response.text();
                document.getElementById('responseField').value = result;
            } catch (error) {
               document.getElementById('responseField').value = 'Error: '+error;
            }
        }
    };
  </script>
  <body>
    <h3>I am a BasicHTTPEventIOProcessor</h3>
    <p class='x'>Please send some event to my FSMs!</p>
    <form onsubmit='submitEvent(event)'><table>
     <tr><td><label for='eventName'>Name of Event</label></td><td><input class='x' type='text' id='eventName' name='eventName' value='leave'><br/></td></tr>
     <tr><td><label for='sessionId'>Id of FSM-Session</label></td>
       <td><input class='x' type='text' list='sessions' id='sessionId' name='sessionId'><datalist id='sessions'>");

    page_source.push_str(sessions.as_str());
    page_source.push_str(
        "</datalist></td></tr>
      <tr><td colspan='2'><br><button type='submit'>Send Event</button></td></tr>
    </table></form>
    <h3>Response from BasicHTTPEventIOProcessor:</h3>
    <textarea id='responseField' rows='10' cols='50'></textarea>
    </body></html>",
    );

    RawHtml(page_source)
}

impl BasicHTTPEventIOProcessor {
    pub async fn new(
        ip_addr: IpAddr,
        location_name: &str,
        port: u16,
        execute_state: ExecutorStateArc,
    ) -> BasicHTTPEventIOProcessor {
        let es_clone = execute_state.clone();

        let figment = rocket::Config::figment();
        #[cfg(feature = "Debug")]
        let figment = figment.merge(Config::debug_default());
        #[cfg(not(feature = "Debug"))]
        let figment = figment.merge(Config::release_default());

        let figment = figment
            .merge(("port", 5555))
            .merge(("shutdown.ctrlc", false));

        let server = rocket::custom(figment)
            .manage(es_clone)
            .mount("/", routes![rocket_welcome, rocket_receive_event, rocket_get_favicon])
            .ignite().await
            .expect("server to launch");
        let shutdown = server.shutdown();

        tokio::spawn(async move { server.launch().await });
        info!("HTTP server started at {}:{}", ip_addr, port);

        BasicHTTPEventIOProcessor {
            shutdown_guard: shutdown,
            // The base uri for requests.
            location: format!("http://{}:{}/scxml/", location_name, port),
            queues: ExternalQueueContainer::new(),
            executor_state: execute_state,
        }
    }
}

const TYPES: &[&str] = &[BASIC_HTTP_EVENT_PROCESSOR, "basichttp"];

impl EventIOProcessor for BasicHTTPEventIOProcessor {
    fn get_location(&self, id: SessionId) -> String {
        format!("{}{}", self.location, id)
    }

    /// Returns the type of this processor.
    fn get_types(&self) -> &[&str] {
        TYPES
    }

    fn get_external_queues(&mut self) -> &mut ExternalQueueContainer {
        &mut self.queues
    }

    fn get_copy(&self) -> Box<dyn EventIOProcessor> {
        let b = BasicHTTPEventIOProcessor {
            shutdown_guard: self.shutdown_guard.clone(),
            location: self.location.clone(),
            queues: self.queues.clone(),
            executor_state: self.executor_state.clone(),
        };
        Box::new(b)
    }

    fn send(&mut self, _global: &GlobalDataArc, target: &str, event: Event) -> bool {

        #[cfg(feature = "Debug")]
        debug!("Send HTTP Event {}", event.name);

        let mut data = Vec::new();
        data.push((SCXML_EVENT_NAME, event.name));
        if let Some(parameters) = &event.param_values {
            for e in parameters {
                data.push((e.name.as_str(), e.value.to_string()));
            }
        }
        // TODO: no other way to convert?
        let form_data: Vec<(&str, &str)> = data.iter()
            .map(|(name, value)| (*name, value.as_str()))
            .collect();

        let r = ureq::post(target).send_form(form_data.as_slice());

        match r {
            Ok(_) => {
            }
            Err(err) => {
                error!("Failed to send to {}. {}", target, err);
            }
        }
        true
    }

    fn shutdown(&mut self) {
        info!("HTTP Event IO Processor shutdown...");
        self.shutdown_guard.clone().notify();
        // Shutdown all FSMs
        self.queues.shutdown();
    }
}
