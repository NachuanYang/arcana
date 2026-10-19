use std::collections::HashMap;
use std::sync::Mutex;
use tokio::sync::watch;

#[derive(Default)]
pub struct ReadingTaskManager {
    active: Mutex<HashMap<(String, String), watch::Sender<bool>>>,
}

impl ReadingTaskManager {
    pub fn begin(
        &self,
        session_id: &str,
        request_id: &str,
    ) -> Result<watch::Receiver<bool>, String> {
        if session_id.trim().is_empty()
            || session_id.len() > 128
            || request_id.trim().is_empty()
            || request_id.len() > 128
        {
            return Err("Reading session or request ID is invalid".into());
        }
        let key = (session_id.to_owned(), request_id.to_owned());
        let mut active = self
            .active
            .lock()
            .map_err(|_| "Reading task registry is unavailable".to_string())?;
        if active.contains_key(&key) {
            return Err("This reading request is already running".into());
        }
        let (sender, receiver) = watch::channel(false);
        active.insert(key, sender);
        Ok(receiver)
    }

    pub fn cancel(&self, session_id: &str, request_id: &str) -> bool {
        self.active
            .lock()
            .ok()
            .and_then(|active| active.get(&(session_id.into(), request_id.into())).cloned())
            .is_some_and(|sender| sender.send(true).is_ok())
    }

    pub fn finish(&self, session_id: &str, request_id: &str) {
        if let Ok(mut active) = self.active.lock() {
            active.remove(&(session_id.into(), request_id.into()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ReadingTaskManager;

    #[test]
    fn cancellation_is_scoped_to_both_session_and_request() {
        let tasks = ReadingTaskManager::default();
        let first = tasks.begin("session-a", "request-1").unwrap();
        let second = tasks.begin("session-b", "request-1").unwrap();

        assert!(!tasks.cancel("session-b", "request-missing"));
        assert!(tasks.cancel("session-a", "request-1"));
        assert!(*first.borrow());
        assert!(!*second.borrow());
        tasks.finish("session-a", "request-1");
        assert!(tasks.begin("session-a", "request-1").is_ok());
    }
}
