use tracing::info;

use super::fault_object::DpiFaultObject;
use super::fault_parameter::DpiFaultParameter;
use crate::cip::{EPath, ServiceCode};
use crate::session::Session;
use crate::{MessageRouter, Result};

const FAULT_COMMAND_WRITE: u16 = 3;
const MAX_FAULT_NUMBER: u8 = 10;

wire_code! {
    /// Commands written to the DPI Fault object class attribute 3.
    DpiFaultCommand(u8) {
        NO_OPERATION = 0,
        CLEAR_FAULT = 1,
        CLEAR_FAULT_QUEUE = 2,
        RESET_DEVICE = 3,
    }
}

pub type NewFaultHandler = Box<dyn FnMut(&DpiFaultParameter) + Send>;

/// Reads the drive's fault queue and optionally clears it.
pub struct DpiFaultManager {
    clear_faults: bool,
    get_fault_details: bool,
    on_new_fault: Option<NewFaultHandler>,
}

impl Default for DpiFaultManager {
    /// Clears the queue after reading it; doesn't read fault details.
    fn default() -> Self {
        Self::new(true, false)
    }
}

impl DpiFaultManager {
    pub fn new(clear_faults: bool, get_fault_details: bool) -> Self {
        Self {
            clear_faults,
            get_fault_details,
            on_new_fault: None,
        }
    }

    pub fn on_new_fault(mut self, f: impl FnMut(&DpiFaultParameter) + Send + 'static) -> Self {
        self.on_new_fault = Some(Box::new(f));
        self
    }

    /// Reads faults 1-10 until an empty entry, calling the new-fault handler
    /// for each, then clears the queue if configured to. Returns the faults.
    pub fn handle_fault_parameters(
        &mut self,
        session: &dyn Session,
        router: &MessageRouter,
    ) -> Result<Vec<DpiFaultParameter>> {
        let mut faults = Vec::new();
        for number in 1..=MAX_FAULT_NUMBER {
            let fault = DpiFaultParameter::read(session, router, number, self.get_fault_details)?;
            if fault.fault_details.fault_code == 0 {
                break;
            }
            if let Some(f) = &mut self.on_new_fault {
                f(&fault);
            }
            faults.push(fault);
        }
        if !faults.is_empty() {
            info!(count = faults.len(), "read faults from the queue");
            if self.clear_faults {
                Self::write_command(DpiFaultCommand::CLEAR_FAULT_QUEUE, session, router)?;
            }
        }
        Ok(faults)
    }

    pub fn write_command(
        command: DpiFaultCommand,
        session: &dyn Session,
        router: &MessageRouter,
    ) -> Result<()> {
        router
            .send_request(
                session,
                ServiceCode::SET_ATTRIBUTE_SINGLE,
                &EPath::attribute(DpiFaultObject::CLASS_ID, 0, FAULT_COMMAND_WRITE),
                &[command.0],
            )?
            .check(format!("write DPI fault command {command}"))?;
        Ok(())
    }
}

impl std::fmt::Debug for DpiFaultManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DpiFaultManager")
            .field("clear_faults", &self.clear_faults)
            .field("get_fault_details", &self.get_fault_details)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::test_support::MockSession;

    #[test]
    fn reads_until_empty_then_clears() {
        let s = MockSession::new()
            .reply(&[2, 0])
            .reply(&[3, 0])
            .reply(&[0, 0])
            .reply(&[]);
        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen2 = seen.clone();
        let mut mgr = DpiFaultManager::new(true, false).on_new_fault(move |f| {
            seen2.lock().unwrap().push(f.fault_details.fault_code);
        });
        let faults = mgr
            .handle_fault_parameters(&s, &MessageRouter::new())
            .unwrap();
        assert_eq!(faults.len(), 2);
        assert_eq!(*seen.lock().unwrap(), [2, 3]);

        let clear = s.requests().pop().unwrap();
        assert_eq!(clear.service, ServiceCode::SET_ATTRIBUTE_SINGLE);
        assert_eq!(clear.path, EPath::attribute(0x97, 0, 3));
        assert_eq!(clear.data, [2]);
    }

    #[test]
    fn empty_queue_is_not_cleared() {
        let s = MockSession::new().reply(&[0, 0]);
        let faults = DpiFaultManager::default()
            .handle_fault_parameters(&s, &MessageRouter::new())
            .unwrap();
        assert!(faults.is_empty());
        assert_eq!(s.requests().len(), 1);
    }
}
