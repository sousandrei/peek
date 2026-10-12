//! Loading stage updates and control of the active image-engine child.

use std::io;
use std::process::{Child, ExitStatus};
use std::sync::{
    Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc::Sender,
};

use anyhow::{Result, ensure};

pub struct LoadProgress {
    updates: Sender<&'static str>,
    cancelled: AtomicBool,
    child: Mutex<Option<Child>>,
}

impl LoadProgress {
    pub fn new(updates: Sender<&'static str>) -> Self {
        Self {
            updates,
            cancelled: AtomicBool::new(false),
            child: Mutex::new(None),
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }

    pub fn report(&self, stage: &'static str) -> Result<()> {
        self.check_cancelled()?;
        let _ = self.updates.send(stage);
        Ok(())
    }

    pub(super) fn check_cancelled(&self) -> Result<()> {
        ensure!(!self.is_cancelled(), "image loading cancelled");
        Ok(())
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
        if let Ok(mut child) = self.child.lock()
            && let Some(child) = child.as_mut()
        {
            let _ = child.kill();
        }
    }

    pub(super) fn set_child(&self, mut child: Child) -> Result<()> {
        let mut active = match self.child.lock() {
            Ok(active) => active,
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(anyhow::anyhow!("image child lock poisoned"));
            }
        };
        if self.is_cancelled() {
            let _ = child.kill();
            let _ = child.wait();
            return Err(anyhow::anyhow!("image loading cancelled"));
        }
        *active = Some(child);
        Ok(())
    }

    pub(super) fn try_wait_child(&self) -> io::Result<Option<ExitStatus>> {
        let mut active = self
            .child
            .lock()
            .map_err(|_| io::Error::other("image child lock poisoned"))?;
        active
            .as_mut()
            .map(Child::try_wait)
            .transpose()
            .map(Option::flatten)
    }

    pub(super) fn kill_child(&self) -> io::Result<()> {
        let mut active = self
            .child
            .lock()
            .map_err(|_| io::Error::other("image child lock poisoned"))?;
        if let Some(child) = active.as_mut() {
            child.kill()?;
        }
        Ok(())
    }

    pub(super) fn wait_child(&self) -> io::Result<ExitStatus> {
        let mut active = self
            .child
            .lock()
            .map_err(|_| io::Error::other("image child lock poisoned"))?;
        active
            .as_mut()
            .ok_or_else(|| io::Error::other("no active image child"))?
            .wait()
    }

    pub(super) fn clear_child(&self) {
        if let Ok(mut active) = self.child.lock()
            && let Some(mut child) = active.take()
        {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

pub(super) fn check(progress: Option<&LoadProgress>) -> Result<()> {
    if let Some(progress) = progress {
        progress.check_cancelled()?;
    }
    Ok(())
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
        let progress = LoadProgress::new(sender);
        progress.report("Exporting image archive").unwrap();
        assert_eq!(receiver.try_recv().unwrap(), "Exporting image archive");
        progress.cancel();
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

    #[cfg(unix)]
    #[test]
    fn cancellation_before_child_registration_kills_and_reaps_the_child() {
        let (sender, _) = mpsc::channel();
        let progress = LoadProgress::new(sender);
        let child = std::process::Command::new("sleep")
            .arg("10")
            .spawn()
            .unwrap();
        let pid = child.id();
        progress.cancel();
        assert!(
            progress
                .set_child(child)
                .unwrap_err()
                .to_string()
                .contains("cancelled")
        );
        assert!(progress.child.lock().unwrap().is_none());
        assert!(
            !std::process::Command::new("kill")
                .args(["-0", &pid.to_string()])
                .stderr(std::process::Stdio::null())
                .status()
                .unwrap()
                .success()
        );
    }

    #[cfg(unix)]
    #[test]
    fn clearing_an_active_child_kills_and_reaps_it() {
        let (sender, _) = mpsc::channel();
        let progress = LoadProgress::new(sender);
        let child = std::process::Command::new("sleep")
            .arg("10")
            .spawn()
            .unwrap();
        let pid = child.id();
        progress.set_child(child).unwrap();
        progress.clear_child();
        assert!(progress.child.lock().unwrap().is_none());
        assert!(
            !std::process::Command::new("kill")
                .args(["-0", &pid.to_string()])
                .stderr(std::process::Stdio::null())
                .status()
                .unwrap()
                .success()
        );
    }
}
