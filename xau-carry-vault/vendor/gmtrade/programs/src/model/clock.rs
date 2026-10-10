
pub(super) struct AsClock<'a> {
    last: &'a i64,
    current: i64,
}

impl AsClock<'_> {
    /// Passed in seconds.
    pub(super) fn passed_in_seconds(&mut self) -> gmsol_model::Result<u64> {
        let current = self.current;
        let duration = current.saturating_sub(*self.last);
        if duration > 0 {
            Ok(duration as u64)
        } else {
            Ok(0)
        }
    }
}

impl<'a> AsClock<'a> {
    pub(super) fn new(last: &'a i64, current: i64) -> Self {
        Self { last, current }
    }
}

/// Clock-related operations.
pub(super) struct AsClockMut<'a> {
    last: &'a mut i64,
    current: i64,
}

impl AsClockMut<'_> {
    /// Just passed in seconds.
    pub(super) fn just_passed_in_seconds(&mut self) -> gmsol_model::Result<u64> {
        let current = self.current;
        let duration = current.saturating_sub(*self.last);
        if duration > 0 {
            *self.last = current;
            Ok(duration as u64)
        } else {
            Ok(0)
        }
    }
}

impl<'a> AsClockMut<'a> {
    pub(super) fn new(last: &'a mut i64, current: i64) -> Self {
        Self { last, current }
    }
}
