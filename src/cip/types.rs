wire_code! {
    /// CIP elementary data type codes.
    CipDataType(u8) {
        ANY = 0x00,
        BOOL = 0xC1,
        SINT = 0xC2,
        INT = 0xC3,
        DINT = 0xC4,
        LINT = 0xC5,
        USINT = 0xC6,
        UINT = 0xC7,
        UDINT = 0xC8,
        ULINT = 0xC9,
        REAL = 0xCA,
        LREAL = 0xCB,
        STIME = 0xCC,
        DATE = 0xCD,
        TIME_OF_DAY = 0xCE,
        DATE_AND_TIME = 0xCF,
        STRING = 0xD0,
        BYTE = 0xD1,
        WORD = 0xD2,
        DWORD = 0xD3,
        LWORD = 0xD4,
        STRING2 = 0xD5,
        FTIME = 0xD6,
        LTIME = 0xD7,
        ITIME = 0xD8,
        STRINGN = 0xD9,
        SHORT_STRING = 0xDA,
        TIME = 0xDB,
        EPATH = 0xDC,
        ENG_UNIT = 0xDD,
        USINT_USINT = 0xA0,
        USINT6 = 0xA2,
        MEMBER_LIST = 0xA3,
        BYTE_ARRAY = 0xA4,
    }
}

wire_code! {
    /// CIP service codes common to all objects.
    ServiceCode(u8) {
        NONE = 0x00,
        GET_ATTRIBUTE_ALL = 0x01,
        SET_ATTRIBUTE_ALL = 0x02,
        GET_ATTRIBUTE_LIST = 0x03,
        SET_ATTRIBUTE_LIST = 0x04,
        RESET = 0x05,
        START = 0x06,
        STOP = 0x07,
        CREATE_OBJECT_INSTANCE = 0x08,
        DELETE_OBJECT_INSTANCE = 0x09,
        MULTIPLE_SERVICE_PACKET = 0x0A,
        APPLY_ATTRIBUTES = 0x0D,
        GET_ATTRIBUTE_SINGLE = 0x0E,
        SET_ATTRIBUTE_SINGLE = 0x10,
        FIND_NEXT_OBJECT_INSTANCE = 0x11,
        ERROR_RESPONSE = 0x14,
        RESTORE = 0x15,
        SAVE = 0x16,
        NO_OPERATION = 0x17,
        GET_MEMBER = 0x18,
        SET_MEMBER = 0x19,
        INSERT_MEMBER = 0x1A,
        REMOVE_MEMBER = 0x1B,
        GROUP_SYNC = 0x1C,
    }
}

wire_code! {
    /// CIP general status codes returned in message router responses.
    GeneralStatusCode(u8) {
        SUCCESS = 0x00,
        CONNECTION_FAILURE = 0x01,
        RESOURCE_UNAVAILABLE = 0x02,
        INVALID_PARAMETER_VALUE = 0x03,
        PATH_SEGMENT_ERROR = 0x04,
        PATH_DESTINATION_UNKNOWN = 0x05,
        PARTIAL_TRANSFER = 0x06,
        CONNECTION_LOST = 0x07,
        SERVICE_NOT_SUPPORTED = 0x08,
        INVALID_ATTRIBUTE_VALUE = 0x09,
        ATTRIBUTE_LIST_ERROR = 0x0A,
        ALREADY_IN_REQUESTED_MODE_OR_STATE = 0x0B,
        OBJECT_STATE_CONFLICT = 0x0C,
        OBJECT_ALREADY_EXISTS = 0x0D,
        ATTRIBUTE_NOT_SETTABLE = 0x0E,
        PRIVILEGE_VIOLATION = 0x0F,
        DEVICE_STATE_CONFLICT = 0x10,
        REPLY_DATA_TOO_LARGE = 0x11,
        FRAGMENTATION_OF_PRIMITIVE_VALUE = 0x12,
        NOT_ENOUGH_DATA = 0x13,
        ATTRIBUTE_NOT_SUPPORTED = 0x14,
        TOO_MUCH_DATA = 0x15,
        OBJECT_DOES_NOT_EXIST = 0x16,
        SVCFRAG_SEQNC_NOT_IN_PROGRESS = 0x17,
        NO_STORED_ATTRIBUTE_DATA = 0x18,
        STORE_OPERATION_FAILURE = 0x19,
        ROUTING_FAILURE_REQUEST_SIZE = 0x1A,
        ROUTING_FAILURE_RESPONSE_SIZE = 0x1B,
        MISSING_ATTRIBUTE_LIST_ENTRY = 0x1C,
        INVALID_ATTRIBUTE_LIST = 0x1D,
        EMBEDDED_SERVICE_ERROR = 0x1E,
        VENDOR_SPECIFIC = 0x1F,
        INVALID_PARAMETER = 0x20,
        WRITE_ONCE_WRITTEN = 0x21,
        INVALID_REPLY_RECEIVED = 0x22,
        KEY_FAILURE_IN_PATH = 0x25,
        PATH_SIZE_INVALID = 0x26,
        UNEXPECTED_ATTRIBUTE = 0x27,
        INVALID_MEMBER_ID = 0x28,
        MEMBER_NOT_SETTABLE = 0x29,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_and_unknown_codes_format() {
        assert_eq!(
            GeneralStatusCode::PATH_DESTINATION_UNKNOWN.to_string(),
            "PATH_DESTINATION_UNKNOWN(0x5)"
        );
        assert_eq!(
            GeneralStatusCode(0x7f).to_string(),
            "GeneralStatusCode(0x7f)"
        );
        assert_eq!(ServiceCode::from(0x0E), ServiceCode::GET_ATTRIBUTE_SINGLE);
    }
}
