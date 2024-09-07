//! Implementation of the SCXML I/O Event Processor.\
//! I/O Processor implementation for type "http://www.w3.org/TR/scxml/#SCXMLEventProcessor" (or short-cut "scxml").
//! See [W3C:SCXML - SCXML Event I/O Processor](/doc/W3C_SCXML_2024_07_13/index.html#/#SCXMLEventProcessor).

use log::error;
use std::fmt::Debug;
#[cfg(test)]
use std::println as info;

#[cfg(not(test))]
use log::info;

use crate::datamodel::{GlobalDataAccess, GlobalDataLock, SCXML_EVENT_PROCESSOR};
use crate::event_io_processor::{EventIOProcessor, EventIOProcessorHandle};
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
/// If the target is the special term '#_invokeid', where invokeid is the invokeid of an SCXML session that the sending session has created by \<invoke\>,
/// the Processor must add the event to the external queue of that session.\
/// This value is prefix of the other SCXML targets and need special care.
pub const SCXML_TARGET_INVOKE_ID_PREFIX: &str = "#_";

/// Shortcut for SCXML I/O Processors type
pub const SCXML_EVENT_PROCESSOR_SHORT_TYPE: &str = "scxml";

#[derive(Debug, Default)]
pub struct ScxmlEventIOProcessor {
    pub location: String,
    pub handle: EventIOProcessorHandle,
}

impl ScxmlEventIOProcessor {
    pub fn new() -> ScxmlEventIOProcessor {
        info!("Scxml Event Processor starting");

        ScxmlEventIOProcessor {
            location: "scxml-processor".to_string(),
            handle: EventIOProcessorHandle::new(),
        }
    }

    fn send_to_session(
        &mut self,
        global_data_lock: &mut GlobalDataLock,
        session_id: SessionId,
        event: Event,
    ) {
        match &global_data_lock.executor {
            None => {
                panic!("Executor not available");
            }
            Some(executor) => {
                info!("Send '{}' to Session #{}", event, session_id);
                match executor.send_to_session(session_id, event.clone()) {
                    Ok(_) => {
                        // TODO
                    }
                    Err(error) => {
                        error!("Can't send to session {}. {}", session_id, error);
                        global_data_lock.enqueue_internal(Event::error_communication(&event));
                    }
                }
            }
        }
    }
}

const TYPES: &[&str] = &[SCXML_EVENT_PROCESSOR, SCXML_EVENT_PROCESSOR_SHORT_TYPE];

impl EventIOProcessor for ScxmlEventIOProcessor {
    fn get_location(&self, id: SessionId) -> String {
        format!("{}/{}", self.location, id)
    }

    /// Returns the type of this processor.
    fn get_types(&self) -> &[&str] {
        TYPES
    }

    fn get_handle(&mut self) -> &mut EventIOProcessorHandle {
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
    /// SCXML Processors must support the following special targets for \<send\>:<ul>
    /// <li>#_internal. If the target is the special term '#_internal', the Processor must add the event to the internal event queue of the sending session.</li>
    /// <li>#_scxml_sessionid. If the target is the special term '#_scxml_sessionid', where sessionid is the id of an SCXML session that is accessible to the Processor, the Processor must add the event to the external queue of that session. The set of SCXML sessions that are accessible to a given SCXML Processor is platform-dependent.</li>
    /// <li>#_parent. If the target is the special term '#_parent', the Processor must add the event to the external event queue of the SCXML session that invoked the sending session, if there is one. See 6.4 <invoke> for details.</li>
    /// <li>#_invokeid. If the target is the special term '#_invokeid', where invokeid is the invokeid of an SCXML session that the sending session has created by <invoke>, the Processor must add the event to the external queue of that session. See 6.4 <invoke> for details.</li>
    /// <li>If neither the 'target' nor the 'targetexpr' attribute is specified, the SCXML Processor must add the event to the external event queue of the sending session.</li>
    /// </ul>
    fn send(&mut self, global: &GlobalDataAccess, target: &str, mut event: Event) {
        let mut global_lock = global.lock();
        event.origin_type = Some(SCXML_EVENT_PROCESSOR_SHORT_TYPE.to_string());
        if event.origin.is_none() {
            event.origin = Some(self.get_location(global_lock.session_id).to_string());
        }
        // For SCXMLEventProcessor: Target is an SCXML session.

        match target {
            "" => {
                global_lock.externalQueue.enqueue(Box::new(event));
            }
            SCXML_TARGET_INTERNAL => {
                event.etype = EventType::internal;

                global_lock.enqueue_internal(event);
            }
            SCXML_TARGET_PARENT => {
                let sid = global_lock.parent_session_id.unwrap();
                self.send_to_session(&mut global_lock, sid, event);
            }
            _ => {
                // W3C: If the sending SCXML session specifies a session that does not exist or is inaccessible,
                //      the SCXML Processor must place the error "error.communication" on the internal event queue of the sending session.
                if target.starts_with(SCXML_TARGET_SESSION_ID_PREFIX) {
                    match target.get(SCXML_TARGET_SESSION_ID_PREFIX.len()..) {
                        None => {
                            error!("Send target '{}' has wrong format.", target);
                            global_lock.enqueue_internal(Event::error_communication(&event));
                        }
                        Some(session_id_s) => match session_id_s.parse::<SessionId>() {
                            Ok(session_id) => {
                                self.send_to_session(&mut global_lock, session_id, event);
                            }
                            Err(_err) => {
                                error!("Send target '{}' has wrong format.", target);
                                global_lock.enqueue_internal(Event::error_communication(&event));
                            }
                        },
                    }
                } else if target.starts_with(SCXML_TARGET_INVOKE_ID_PREFIX) {
                    match target.get(SCXML_TARGET_INVOKE_ID_PREFIX.len()..) {
                        None => {
                            error!("Send target '{}' has wrong format.", target);
                            global_lock.enqueue_internal(Event::error_communication(&event));
                        }
                        Some(invokeid) => {
                            let session_id = match global_lock.child_sessions.get(invokeid) {
                                None => {
                                    error!(
                                        "InvokeId of target {} '{}' is not available.",
                                        invokeid, target
                                    );
                                    global_lock
                                        .enqueue_internal(Event::error_communication(&event));
                                    return;
                                }
                                Some(session) => session.session_id,
                            };
                            self.send_to_session(&mut global_lock, session_id, event);
                        }
                    }
                } else {
                    // TODO: Clarify the case, that the format is illegal.
                    global_lock.enqueue_internal(Event::error_communication(&event));
                }
            }
        }
    }

    /// This processor doesn't really need a shutdown.
    /// The implementation does nothing.
    fn shutdown(&mut self) {
        info!("Scxml Event IO Processor shutdown...");
        self.handle.shutdown();
    }
}
