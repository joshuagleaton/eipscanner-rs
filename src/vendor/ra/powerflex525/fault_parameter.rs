use bytes::Buf;

use super::fault_code::{FaultDescription, fault_description};
use crate::cip::{EPath, ServiceCode};
use crate::objects::ParameterObject;
use crate::session::Session;
use crate::{Error, MessageRouter, Result};

/// Fault queue parameter numbers, index 0 = fault 1.
const CODE_PARAMS: [u16; 10] = [7, 8, 9, 604, 605, 606, 607, 608, 609, 610];
const FREQUENCY_PARAMS: [u16; 10] = [631, 632, 633, 634, 635, 636, 637, 638, 639, 640];
const CURRENT_PARAMS: [u16; 10] = [641, 642, 643, 644, 645, 646, 647, 648, 649, 650];
const BUS_VOLTS_PARAMS: [u16; 10] = [651, 652, 653, 654, 655, 656, 657, 658, 659, 660];

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct FaultDetails {
    /// Position in the fault queue, 1-10.
    pub fault_number: u8,
    pub fault_code: u16,
    /// Volts; 0 unless details were requested.
    pub bus_voltage: f64,
    /// Amps; 0 unless details were requested.
    pub current: f64,
    /// Hz; 0 unless details were requested.
    pub frequency: f64,
}

/// One entry of the drive's fault queue, read from parameters.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct DpiFaultParameter {
    pub fault_details: FaultDetails,
    /// `None` if the code is not in the PowerFlex 525 table.
    pub fault_description: Option<&'static FaultDescription>,
}

impl DpiFaultParameter {
    /// Reads fault `fault_number` (1-10) of the queue. With `get_details`, also
    /// reads the bus voltage, current, and frequency at the time of the fault.
    pub fn read(
        session: &dyn Session,
        router: &MessageRouter,
        fault_number: u8,
        get_details: bool,
    ) -> Result<Self> {
        let i = fault_number
            .checked_sub(1)
            .filter(|&i| (i as usize) < CODE_PARAMS.len())
            .ok_or_else(|| {
                Error::Unsupported(format!("fault number {fault_number} must be 1-10"))
            })? as usize;

        let fault_code = read_param(session, router, CODE_PARAMS[i])?;
        let mut details = FaultDetails {
            fault_number,
            fault_code,
            ..Default::default()
        };
        if fault_code != 0 && get_details {
            details.bus_voltage = scale(read_param(session, router, BUS_VOLTS_PARAMS[i])?, 0);
            details.current = scale(read_param(session, router, CURRENT_PARAMS[i])?, 2);
            details.frequency = scale(read_param(session, router, FREQUENCY_PARAMS[i])?, 2);
        }
        Ok(Self {
            fault_details: details,
            fault_description: fault_description(fault_code),
        })
    }
}

fn read_param(session: &dyn Session, router: &MessageRouter, parameter: u16) -> Result<u16> {
    let reply = router
        .send_request(
            session,
            ServiceCode::GET_ATTRIBUTE_SINGLE,
            &EPath::attribute(ParameterObject::CLASS_ID, parameter, 1),
            &[],
        )?
        .check(format!("read fault parameter {parameter}"))?;
    Ok((&reply.data[..]).try_get_u16_le()?)
}

fn scale(raw: u16, precision: u8) -> f64 {
    raw as f64 / 10f64.powi(precision as i32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::MockSession;

    #[test]
    fn read_with_details() {
        let s = MockSession::new()
            .reply(&[4, 0])
            .reply(&[0x40, 0x01])
            .reply(&[0xE8, 0x03])
            .reply(&[0x70, 0x17]);
        let p = DpiFaultParameter::read(&s, &MessageRouter::new(), 4, true).unwrap();
        assert_eq!(p.fault_details.fault_code, 4);
        assert_eq!(p.fault_details.bus_voltage, 320.0);
        assert_eq!(p.fault_details.current, 10.0);
        assert_eq!(p.fault_details.frequency, 60.0);
        assert_eq!(p.fault_description.unwrap().text, "UnderVoltage");
        let params: Vec<_> = s
            .requests()
            .iter()
            .map(|r| r.path.instance_id().unwrap())
            .collect();
        assert_eq!(params, [604, 654, 644, 634]);
    }

    #[test]
    fn no_fault_skips_details() {
        let s = MockSession::new().reply(&[0, 0]);
        let p = DpiFaultParameter::read(&s, &MessageRouter::new(), 1, true).unwrap();
        assert_eq!(p.fault_details.fault_code, 0);
        assert_eq!(s.requests().len(), 1);
    }

    #[test]
    fn fault_number_out_of_range() {
        let s = MockSession::new();
        assert!(DpiFaultParameter::read(&s, &MessageRouter::new(), 0, false).is_err());
        assert!(DpiFaultParameter::read(&s, &MessageRouter::new(), 11, false).is_err());
    }
}
