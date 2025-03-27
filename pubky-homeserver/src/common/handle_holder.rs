use std::sync::Arc;
use tokio::{sync::Mutex, task::JoinHandle};

/// A helper struct to hold a join handle and abort it when the struct is dropped.
pub(crate) struct InnerHandleHolder<T> {
    pub(crate) handle: Option<JoinHandle<T>>,
}

impl<T> InnerHandleHolder<T> {
    pub fn new(handle: JoinHandle<T>) -> Self {
        Self { handle: Some(handle) }
    }
    /// Destroys the holder and returns the handle.
    pub fn get(mut self) -> Option<JoinHandle<T>> {
        self.handle.take()
    }
}

impl<T> Drop for InnerHandleHolder<T> {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            handle.abort();
        }
    }
}

impl<T> std::fmt::Debug for InnerHandleHolder<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "HandleHolder")
    }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum HandleHolderError {
    #[error("The handle has already been extracted.")]
    AlreadyJoined,

}

/// A helper struct to hold a join handle and abort it when the struct is dropped.
/// It is cloneable.
#[derive(Debug, Clone)]
pub(crate) struct HandleHolder<T> {
    handle: Arc<Mutex<Option<InnerHandleHolder<T>>>>,
}

impl<T> HandleHolder<T> {
    /// Creates a new handle holder from a join handle.
    /// 
    /// If the last of the `HandleHolder` is dropped, the handle will be aborted.
    /// This makes sure that the handle is not leaked.
    pub fn new(handle: JoinHandle<T>) -> Self {
        Self { handle: Arc::new(Mutex::new(Some(InnerHandleHolder::new(handle)))) }
    }

    /// Tries to get the handle. If the handle has already been dissolved
    /// by a copy of the `HandleHolder`, it will return an error.
    pub async fn dissolve(self) -> Result<JoinHandle<T>, HandleHolderError> {
        let mut opt = self.handle.lock().await;
        if let Some(handle) = opt.take() {
            let inner_handle_opt = handle.get();
            match inner_handle_opt {
                Some(inner_handle) => Ok(inner_handle),
                None => Err(HandleHolderError::AlreadyJoined), // This should never happen.
            }
        } else {
            Err(HandleHolderError::AlreadyJoined)
        }
    }

    /// Checks if the handle has already been dissolved.
    /// This may happen if a copy of the `HandleHolder` has been called with `dissolve()`.
    pub async fn is_dissolved(&self) -> bool {
        let opt = self.handle.lock().await;
        opt.is_none()
    }
}

