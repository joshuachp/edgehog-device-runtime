// This file is part of Edgehog.
//
// Copyright 2026 SECO Mind Srl
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.
//
// SPDX-License-Identifier: Apache-2.0

//! Working with job timestamps

use std::fmt::Display;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use eyre::{Context, OptionExt};
use tracing::debug;

/// A Unix timestamp.
///
/// Number of seconds elapsed from the [`UNIX_EPOCH`], this timestamp can only be after.
#[derive(Debug, Clone, Copy)]
pub struct Unix {
    ts: u64,
}

impl Unix {
    /// Maximum value for the Unix timestamp.
    ///
    /// This is because the timestamp need to be converted to i64.
    pub const MAX: u64 = i64::MAX as u64;

    /// Create a new Unix timestamp from the number of seconds
    ///
    /// This supports up till i64::MAX values.
    pub(crate) fn new(seconds: u64) -> Option<Self> {
        if seconds >= Self::MAX {
            None
        } else {
            Some(Self { ts: seconds })
        }
    }

    /// Returns the timestamp.
    pub(crate) fn as_u64(&self) -> u64 {
        self.ts
    }

    /// Returns the timestamp.
    pub(crate) fn as_i64(&self) -> i64 {
        self.ts as i64
    }

    /// Returns the instant of the timestamp if it's in the future.
    pub(crate) fn wait_until(&self) -> eyre::Result<Option<Instant>> {
        let sched_dur = Duration::from_secs(self.ts);
        let sched_sys = UNIX_EPOCH
            .checked_add(sched_dur)
            .ok_or_eyre("couldn't add to unix time")?;

        let now_sys = SystemTime::now();
        let now = Instant::now();

        // Calculate the difference (duration to wait) between now and the scheduled system time
        match sched_sys.duration_since(now_sys) {
            Ok(duration) => {
                let next = now
                    .checked_add(duration)
                    .ok_or_eyre("couldn't reppresent future instant")?;

                debug!(secs = duration.as_secs(), "scheduled task will wait");

                Ok(Some(next))
            }
            Err(error) => {
                debug!(%error, "scheduled task immediatelly");

                Ok(None)
            }
        }
    }
}

impl TryFrom<i64> for Unix {
    type Error = eyre::Report;

    fn try_from(value: i64) -> Result<Self, Self::Error> {
        u64::try_from(value)
            .ok()
            .and_then(Unix::new)
            .ok_or_eyre("invalid unix timestamp")
    }
}

impl Display for Unix {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}s", self.ts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eyre::Result;
    use pretty_assertions::assert_eq;

    #[test]
    fn new_and_value() {
        let ts = Unix::new(42).unwrap();
        assert_eq!(ts.ts, 42);
    }

    #[test]
    fn new_from_invalid() {
        assert!(Unix::new(u64::MAX).is_none());
    }

    #[test]
    fn try_from_valid() -> Result<()> {
        let ts: Unix = 100i64.try_into()?;
        assert_eq!(ts.ts, 100);
        Ok(())
    }

    #[test]
    fn try_from_invalid() {
        Unix::try_from(-1i64).unwrap_err();
    }

    #[test]
    fn next_instant() {
        let ts = SystemTime::now()
            .checked_add(Duration::from_secs(120))
            .unwrap()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let ts = Unix::new(ts);
        let instant_opt = ts.wait_until().unwrap();
        let now_s = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        if seconds > now_s {
            assert!(instant_opt.is_some());
            let dur = Duration::from_secs(seconds - now_s);
            let predicted = Instant::now() + dur;
            let calculated = instant_opt.unwrap();
            let diff = calculated.duration_since(predicted).as_micros();
            assert!(diff.abs() <= 100_000); // 100ms tolerance
        } else {
            assert!(instant_opt.is_none());
        }
    }
}
