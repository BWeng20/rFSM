//! Implementation of the SCXML I/O Event Processor.\
//! I/O Processor implementation for type "<http://www.w3.org/TR/scxml/#SCXMLEventProcessor>" (or short-cut "scxml").
//! See [W3C:SCXML - SCXML Event I/O Processor](/doc/W3C_SCXML_2024_07_13/index.html#/#SCXMLEventProcessor).

use std::fmt::Debug;

#[cfg(feature = "Debug")]
use crate::common::debug;
use crate::common::error;
use crate::datamodel::{GlobalDataArc, GlobalDataLock, SCXML_EVENT_PROCESSOR};
use crate::event_io_processor::{EventIOProcessor, ExternalQueueContainer};
use crate::fsm::{Event, EventType, SessionId};

/// SCXML Processors specific target:\
/// If the target is the special term '#_internal', the Processor must add the event to the internal event queue of the sending session.
pub const SCXML_TARGET_INTERNAL: &str = "#_internal";

/// SCXML Processors specific target:\
/// If the target is the special term '#_scxml_sessionid', where sessionid is the id of an SCXML session that is accessible to the Processor,
/// the Processor must add the event to the external queue of that session. The set of SCXML sessions that are accessible to a given SCXML Processor is platform-dependent.
pub const SCXML_TARGET_SESSION_ID_PREFIX: &str = "#_scxml_";

/// SCXML Processors specific target:\
/// If the target is the special term '#_parent', the Processor must add the event to the external event queue of the SCXML session that
/// invoked the sending session, if there is one.
pub const SCXML_TARGET_PARENT: &str = "#_parent";

/// SCXML Processors specific target:\
/// If the target is the special term '#_invokeid', where invokeid is the invokeid of an SCXML session that the sending session has created by &lt;invoke\>,
/// the Processor must add the event to the external queue of that session.\
/// This value is prefix of the other SCXML targets and need special care.
pub const SCXML_TARGET_INVOKE_ID_PREFIX: &str = "#_";

/// Shortcut for SCXML I/O Processors type
pub const SCXML_EVENT_PROCESSOR_SHORT_TYPE: &str = "scxml";

#[derive(Debug, Default)]
pub struct ScxmlEventIOProcessor {
    pub location: String,
    pub handle: ExternalQueueContainer,
}

impl ScxmlEventIOProcessor {
    pub fn new() -> ScxmlEventIOProcessor {
        #[cfg(feature = "Debug")]
        debug!("Scxml Event Processor starting");

        ScxmlEventIOProcessor {
            location: SCXML_TARGET_SESSION_ID_PREFIX.to_string(),
            handle: ExternalQueueContainer::new(),
        }
    }

    fn send_to_session(&mut self, global_data_lock: &mut GlobalDataLock, session_id: SessionId, event: Event) -> bool {
        match &global_data_lock.executor {
            None => {
                panic!("Executor not available");
            }
            Some(executor) => {
                #[cfg(feature = "Debug")]
                debug!("Send '{}' to Session #{}", event, session_id);
                match executor.send_to_session(session_id, event.clone()) {
                    Ok(_) => {
                        // TODO
                        true
                    }
                    Err(error) => {
                        error!("Can't send to session {}. {}", session_id, error);
                        global_data_lock.enqueue_internal(Event::error_communication(&event));
                        false
                    }
                }
            }
        }
    }
}

const TYPES: &[&str] = &[SCXML_EVENT_PROCESSOR, SCXML_EVENT_PROCESSOR_SHORT_TYPE];

impl EventIOProcessor for ScxmlEventIOProcessor {
    fn get_location(&self, id: SessionId) -> String {
        format!("{}{}", self.location, id)
    }

    /// Returns the type of this processor.
    fn get_types(&self) -> &[&str] {
        TYPES
    }

    fn get_external_queues(&mut self) -> &mut ExternalQueueContainer {
        &mut self.handle
    }

    fn get_copy(&self) -> Box<dyn EventIOProcessor> {
        let b = ScxmlEventIOProcessor {
            location: self.location.clone(),
            handle: self.handle.clone(),
        };
        Box::new(b)
    }

    /// W3C: (only the relevant parts)\
    /// Generated Events: <ul>
    /// <li>The 'origin' field of the event raised in the receiving session must match the value of the
    /// 'location' field inside the entry for the SCXML Event I/O Processor in the _ioprocessors
    ///  system variable in the sending session.</li>
    /// <li>The 'origintype' field of the event raised in the receiving session must have the value "scxml".</li>
    /// </ul>
    /// SCXML Processors must support the following special targets for &lt;send\>:<ul>
    /// <li>#_internal. If the target is the special term '#_internal', the Processor must add the event to the internal event queue of the sending session.</li>
    /// <li>#_scxml_sessionid. If the target is the special term '#_scxml_sessionid', where sessionid is the id of an SCXML session that is accessible to the Processor, the Processor must add the event to the external queue of that session. The set of SCXML sessions that are accessible to a given SCXML Processor is platform-dependent.</li>
    /// <li>#_parent. If the target is the special term '#_parent', the Processor must add the event to the external event queue of the SCXML session that invoked the sending session, if there is one. See 6.4 &lt;invoke\> for details.</li>
    /// <li>#_invokeid. If the target is the special term '#_invokeid', where invokeid is the invokeid of an SCXML session that the sending session has created by &lt;invoke\>, the Processor must add the event to the external queue of that session. See 6.4 &lt;invoke\> for details.</li>
    /// <li>If neither the 'target' nor the 'targetexpr' attribute is specified, the SCXML Processor must add the event to the external event queue of the sending session.</li>
    /// </ul>
    fn send(&mut self, global: &GlobalDataArc, target: &str, mut event: Event) -> bool {
        let mut global_lock = global.lock().unwrap();
        event.origin_type = Some(SCXML_EVENT_PROCESSOR.to_string());
        if event.origin.is_none() {
            event.origin = Some(self.get_location(global_lock.session_id).to_string());
        }
        // For SCXMLEventProcessor: Target is an SCXML session.

        match target {
            "" => {
                global_lock.externalQueue.enqueue(Box::new(event));
                true
            }
            SCXML_TARGET_INTERNAL => {
                event.etype = EventType::internal;
                global_lock.enqueue_internal(event);
                true
            }
            SCXML_TARGET_PARENT => {
                let sid = global_lock.parent_session_id.unwrap();
                self.send_to_session(&mut global_lock, sid, event)
            }
            _ => {
                // W3C: If the sending SCXML session specifies a session that does not exist or is inaccessible,
                //      the SCXML Processor must place the error "error.communication" on the internal event queue of the sending session.
                if target.starts_with(SCXML_TARGET_SESSION_ID_PREFIX) {
                    match target.get(SCXML_TARGET_SESSION_ID_PREFIX.len()..) {
                        None => {
                            error!("Send target '{}' has wrong format.", target);
                            global_lock.enqueue_internal(Event::error_communication(&event));
                            false
                        }
                        Some(session_id_s) => match session_id_s.parse::<SessionId>() {
                            Ok(session_id) => self.send_to_session(&mut global_lock, session_id, event),
                            Err(_err) => {
                                error!("Send target '{}' has wrong format.", target);
                                global_lock.enqueue_internal(Event::error_communication(&event));
                                false
                            }
                        },
                    }
                } else if target.starts_with(SCXML_TARGET_INVOKE_ID_PREFIX) {
                    match target.get(SCXML_TARGET_INVOKE_ID_PREFIX.len()..) {
                        None => {
                            error!("Send target '{}' has wrong format.", target);
                            global_lock.enqueue_internal(Event::error_communication(&event));
                            false
                        }
                        Some(invokeid) => {
                            let session_id = match global_lock.child_sessions.get(invokeid) {
                                None => {
                                    error!(
                                        "InvokeId of target {} '{}' is not available.",
                                        invokeid, target
                                    );
                                    global_lock.enqueue_internal(Event::error_communication(&event));
                                    return false;
                                }
                                Some(session) => session.session_id,
                            };
                            self.send_to_session(&mut global_lock, session_id, event)
                        }
                    }
                } else {
                    // W3C says:
                    // If the value ... is not supported or invalid, the Processor MUST place the
                    // error error.execution on the internal event queue.
                    global_lock.enqueue_internal(Event::error_execution(&event.sendid, &event.invoke_id));
                    false
                }
            }
        }
    }

    /// This processor doesn't really need a shutdown.
    /// The implementation does nothing.
    fn shutdown(&mut self) {
        #[cfg(feature = "Debug")]
        debug!("Scxml Event IO Processor shutdown...");
        self.handle.shutdown();
    }
}
