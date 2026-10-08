//! Layer application, filesystem differences, efficiency analysis, and supplied rules.

use std::io;

pub fn analyze(
    _lowest_efficiency: Option<&str>,
    _highest_wasted_bytes: Option<&str>,
    _highest_user_wasted_percent: Option<&str>,
) -> io::Result<()> {
    Ok(())
}
