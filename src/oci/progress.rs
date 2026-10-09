//! Loading updates and cooperative cancellation, independent of terminal state.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc::Sender,
};

use anyhow::{Result, ensure};

pub struct LoadProgress {
    updates: Sender<&'static str>,
    cancelled: Arc<AtomicBool>,
}

impl LoadProgress {
    pub fn new(updates: Sender<&'static str>, cancelled: Arc<AtomicBool>) -> Self {
        Self { updates, cancelled }
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }

    pub fn report(&self, stage: &'static str) -> Result<()> {
        ensure!(!self.is_cancelled(), "image loading cancelled");
        let _ = self.updates.send(stage);
        Ok(())
    }
}

pub(super) fn report(progress: Option<&LoadProgress>, stage: &'static str) -> Result<()> {
    if let Some(progress) = progress {
        progress.report(stage)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[test]
    fn updates_report_stages_and_cancellation_stops_loading() {
        let (sender, receiver) = mpsc::channel();
        let cancelled = Arc::new(AtomicBool::new(false));
        let progress = LoadProgress::new(sender, Arc::clone(&cancelled));
        progress.report("Exporting image archive").unwrap();
        assert_eq!(receiver.try_recv().unwrap(), "Exporting image archive");
        cancelled.store(true, Ordering::Relaxed);
        assert!(progress.report("Decoding image layers").is_err());
        assert!(receiver.try_recv().is_err());
        assert!(
            crate::oci::load_with_progress(
                "missing.tar",
                crate::oci::Source::DockerArchive,
                None,
                &progress
            )
            .unwrap_err()
            .to_string()
            .contains("cancelled")
        );
    }
}
