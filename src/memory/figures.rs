//! The figures the machine reports: device memory as a whole, and what one
//! child holds once it has loaded.
//!
//! Their own module because the probe that asks for them and the parser that
//! reads them out of a tool's text both name them, and neither should have to
//! name the other to do so.

/// What one machine's device memory looks like, in mebibytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeviceMemory {
    /// What every device the driver reports holds, together.
    pub total_mib: u64,
    /// What is in use right now, by anything on the machine.
    pub used_mib: u64,
}

impl DeviceMemory {
    /// What is left for a model to be started into.
    #[must_use]
    pub fn free_mib(&self) -> u64 {
        self.total_mib.saturating_sub(self.used_mib)
    }
}

/// What one child was found to hold once it had loaded, in mebibytes.
///
/// Either side may be unknown, independently: a machine without a device
/// probe still has a resident set to read, and a platform where the device
/// query prints nothing still reports the device's total.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Measurement {
    /// The process's resident set, or `None` when it could not be read.
    pub resident_mib: Option<u64>,
    /// What the process holds on the device, or `None` when it could not be
    /// read.
    pub device_mib: Option<u64>,
}

impl Measurement {
    /// Nothing could be read, which is what every child cost before this
    /// module existed.
    pub const UNKNOWN: Self = Self {
        resident_mib: None,
        device_mib: None,
    };

    /// The larger of the two sides, or `None` when neither is known.
    ///
    /// The larger rather than the sum, because weights mapped from a file
    /// count in the resident set *and*, once copied to the device, on the
    /// device -- so a sum would charge one model twice for the same bytes.
    /// The larger side is what the model costs on the memory it mostly lives
    /// in, which is what the budget's one number stands for.
    #[must_use]
    pub fn largest_mib(&self) -> Option<u64> {
        self.resident_mib.into_iter().chain(self.device_mib).max()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_largest_side_is_the_measurement_and_one_unknown_side_does_not_hide_the_other() {
        let both = Measurement {
            resident_mib: Some(4600),
            device_mib: Some(725),
        };
        assert_eq!(both.largest_mib(), Some(4600));

        let device_only = Measurement {
            resident_mib: None,
            device_mib: Some(725),
        };
        assert_eq!(device_only.largest_mib(), Some(725));
        assert_eq!(Measurement::UNKNOWN.largest_mib(), None);
    }
}
