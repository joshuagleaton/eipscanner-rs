/// Description of a PowerFlex 525 fault code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FaultDescription {
    /// Fault type as listed in the PowerFlex 525 user manual.
    pub fault_type: u8,
    pub text: &'static str,
    pub description: &'static str,
}

/// Looks up a fault code; `None` if the code is not in the PowerFlex 525 table.
pub fn fault_description(code: u16) -> Option<&'static FaultDescription> {
    FAULTS
        .binary_search_by_key(&code, |(c, _)| *c)
        .ok()
        .map(|i| &FAULTS[i].1)
}

/// Sorted by code.
static FAULTS: &[(u16, FaultDescription)] = &[
    (
        0,
        FaultDescription {
            fault_type: 0,
            text: "No Fault",
            description: "No fault present.",
        },
    ),
    (
        2,
        FaultDescription {
            fault_type: 1,
            text: "Heatsink OvrTmp",
            description: "Heatsink/Power Module temperature exceeds a predefined value.",
        },
    ),
    (
        3,
        FaultDescription {
            fault_type: 2,
            text: "Power Loss",
            description: "Single phase operation detected with excessive load.",
        },
    ),
    (
        4,
        FaultDescription {
            fault_type: 1,
            text: "UnderVoltage",
            description: "DC bus voltage fell below the minimum value.",
        },
    ),
    (
        5,
        FaultDescription {
            fault_type: 1,
            text: "OverVoltage",
            description: "DC bus voltage exceeded maximum value.",
        },
    ),
    (
        6,
        FaultDescription {
            fault_type: 1,
            text: "Motor Stalled",
            description: "Drive is unable to accelerate or decelerate motor.",
        },
    ),
    (
        7,
        FaultDescription {
            fault_type: 1,
            text: "Motor Overload",
            description: "Internal electronic overload trip.",
        },
    ),
    (
        8,
        FaultDescription {
            fault_type: 1,
            text: "Heatsink OvrTmp",
            description: "Heatsink/Power Module temperature exceeds a predefined value.",
        },
    ),
    (
        9,
        FaultDescription {
            fault_type: 1,
            text: "CC OvrTmp",
            description: "Control module temperature exceeds a predefined value.",
        },
    ),
    (
        12,
        FaultDescription {
            fault_type: 2,
            text: "HW OverCurrent",
            description: "The drive output current has exceeded the hardware current limit.",
        },
    ),
    (
        13,
        FaultDescription {
            fault_type: 1,
            text: "Ground Fault",
            description: "A current path to earth ground has been detected at one or more of the drive output terminals.",
        },
    ),
    (
        15,
        FaultDescription {
            fault_type: 2,
            text: "Load Loss",
            description: "The output torque current is below the value programmed in A490 [Load Loss Level] for a time period greater than the time programmed in A491 [Load Loss Time].",
        },
    ),
    (
        21,
        FaultDescription {
            fault_type: 1,
            text: "Output Ph Loss",
            description: "Output Phase Loss (if enabled). Configure with A557 [Output Phas Loss En]",
        },
    ),
    (
        29,
        FaultDescription {
            fault_type: 1,
            text: "Analog In Loss",
            description: "An analog input is configured to fault on signal loss. A signal loss has occurred. Configure with t094 [Anlg In V Loss] or t097 [Anlg In mA Loss].",
        },
    ),
    (
        33,
        FaultDescription {
            fault_type: 2,
            text: "Auto Rstrt Tries",
            description: "Drive unsuccessfully attempted to reset a fault and resume running for the programmed number of A541 [Auto Rstrt Tries].",
        },
    ),
    (
        38,
        FaultDescription {
            fault_type: 2,
            text: "Phase U to Gnd",
            description: "A phase to ground fault has been detected between the drive and motor in this phase.",
        },
    ),
    (
        39,
        FaultDescription {
            fault_type: 2,
            text: " Phase V to Gnd",
            description: "A phase to ground fault has been detected between the drive and motor in this phase.",
        },
    ),
    (
        40,
        FaultDescription {
            fault_type: 2,
            text: " Phase W to Gnd",
            description: "A phase to ground fault has been detected between the drive and motor in this phase.",
        },
    ),
    (
        41,
        FaultDescription {
            fault_type: 2,
            text: "Phase UV Short",
            description: "Excessive current has been detected between these two output terminals",
        },
    ),
    (
        42,
        FaultDescription {
            fault_type: 2,
            text: "Phase UW Short",
            description: "Excessive current has been detected between these two output terminals",
        },
    ),
    (
        43,
        FaultDescription {
            fault_type: 2,
            text: "Phase VW Short",
            description: "Excessive current has been detected between these two output terminals",
        },
    ),
    (
        48,
        FaultDescription {
            fault_type: 1,
            text: "Params Defaulted",
            description: " The drive was commanded to write default values to EEPROM.",
        },
    ),
    (
        59,
        FaultDescription {
            fault_type: 1,
            text: "Safety Open",
            description: "Both of the safety inputs (Safety 1, Safety 2) are not enabled. Configure with t105 [Safety Open En].",
        },
    ),
    (
        63,
        FaultDescription {
            fault_type: 1,
            text: "SW OverCurrent",
            description: "Programmed A486, A488 [Shear Pinx Level] has been exceeded for a time period greater than the time programmed in A487, A489 [Shear Pin x Time].",
        },
    ),
    (
        64,
        FaultDescription {
            fault_type: 2,
            text: "Drive Overload",
            description: "Drive overload rating has been exceeded.",
        },
    ),
    (
        70,
        FaultDescription {
            fault_type: 2,
            text: "Power Unit",
            description: "Failure has been detected in the drive power section.",
        },
    ),
    (
        71,
        FaultDescription {
            fault_type: 2,
            text: "DSI Net Loss",
            description: "Control over the Modbus or DSI communications link has been interrupted.",
        },
    ),
    (
        72,
        FaultDescription {
            fault_type: 2,
            text: "Opt Net Loss",
            description: "Control over the network option card’s remote network has been interrupted.",
        },
    ),
    (
        73,
        FaultDescription {
            fault_type: 2,
            text: "EN Net Loss",
            description: "Control through the embedded EtherNet/IP adapter has been interrupted.",
        },
    ),
    (
        80,
        FaultDescription {
            fault_type: 2,
            text: "Autotune Failure",
            description: "The autotune function was either cancelled by the user or failed.",
        },
    ),
    (
        81,
        FaultDescription {
            fault_type: 2,
            text: "DSI Comm Loss",
            description: "Communications between the drive and the Modbus or DSI master device have been interrupted.",
        },
    ),
    (
        82,
        FaultDescription {
            fault_type: 2,
            text: "Opt Comm Loss",
            description: "Communications between the drive and the network option card have been interrupted.",
        },
    ),
    (
        83,
        FaultDescription {
            fault_type: 2,
            text: "EN Comm Loss",
            description: "Internal communications between the drive and the embedded EtherNet/IP adapter have been interrupted.",
        },
    ),
    (
        91,
        FaultDescription {
            fault_type: 2,
            text: "Encoder Loss",
            description: "Requires differential encoder. One of the 2 encoder channel signals is missing",
        },
    ),
    (
        94,
        FaultDescription {
            fault_type: 2,
            text: "Function Loss",
            description: "'Freeze-Fire' (Function Loss) input is inactive, input to the programmed terminal is open.",
        },
    ),
    (
        100,
        FaultDescription {
            fault_type: 2,
            text: "Parameter Chksum",
            description: "Drive parameter non-volatile storage is corrupted.",
        },
    ),
    (
        101,
        FaultDescription {
            fault_type: 2,
            text: "External Storage",
            description: "External non-volatile storage has failed.",
        },
    ),
    (
        105,
        FaultDescription {
            fault_type: 2,
            text: "C Connect Err",
            description: "Control module was disconnected while drive was powered.",
        },
    ),
    (
        106,
        FaultDescription {
            fault_type: 2,
            text: "Incompat C-P",
            description: "The PowerFlex 525 control module does not support power modules with 0.25 HP power rating",
        },
    ),
    (
        107,
        FaultDescription {
            fault_type: 2,
            text: "Replaced C-P",
            description: "The control module could not recognize the power module. Hardware failure.",
        },
    ),
    (
        109,
        FaultDescription {
            fault_type: 2,
            text: "Mismatch C-P",
            description: "The control module was mounted to a different drive type power module",
        },
    ),
    (
        110,
        FaultDescription {
            fault_type: 2,
            text: "Keypad Membrane",
            description: "Keypad membrane failure / disconnected.",
        },
    ),
    (
        111,
        FaultDescription {
            fault_type: 2,
            text: "Safety Hardware",
            description: "Safety input enable hardware malfunction. One of the safety inputs is not enabled.",
        },
    ),
    (
        114,
        FaultDescription {
            fault_type: 2,
            text: "uC Failure",
            description: "Microprocessor failure.",
        },
    ),
    (
        122,
        FaultDescription {
            fault_type: 2,
            text: "I/O Board Fail",
            description: "Failure has been detected in the drive control and I/O section.",
        },
    ),
    (
        125,
        FaultDescription {
            fault_type: 2,
            text: "Flash Update Req",
            description: "The firmware in the drive is corrupt, mismatched, or incompatible with the hardware.",
        },
    ),
    (
        126,
        FaultDescription {
            fault_type: 2,
            text: "NonRecoverablErr",
            description: "A non-recoverable firmware or hardware error was detected. The drive was automatically stopped and reset.",
        },
    ),
    (
        127,
        FaultDescription {
            fault_type: 2,
            text: "DSIFlashUpdatReq",
            description: "A critical problem with the firmware was detected and the drive is running using backup firmware that only supports DSI communications.",
        },
    ),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_is_sorted() {
        assert!(FAULTS.windows(2).all(|w| w[0].0 < w[1].0));
    }

    #[test]
    fn lookup() {
        assert_eq!(fault_description(4).unwrap().text, "UnderVoltage");
        assert_eq!(fault_description(0).unwrap().text, "No Fault");
        assert!(fault_description(1).is_none());
    }
}
