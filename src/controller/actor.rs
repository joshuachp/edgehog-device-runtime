//! Trait to generalize one task on the runtime.

use async_trait::async_trait;
use log::{debug, trace};
use stable_eyre::eyre::Context;
use tokio::sync::mpsc;

#[async_trait]
pub trait Actor: Sized {
    type Msg: Send + 'static;

    fn task() -> &'static str;

    async fn init(&mut self) -> stable_eyre::Result<()>;

    async fn handle(&mut self, msg: Self::Msg) -> stable_eyre::Result<()>;

    async fn spawn(mut self, mut channel: mpsc::Receiver<Self::Msg>) -> stable_eyre::Result<()> {
        self.init()
            .await
            .wrap_err_with(|| format!("init for {} task failed", Self::task()))?;

        while let Some(msg) = channel.recv().await {
            trace!("message received for {} task", Self::task());

            self.handle(msg)
                .await
                .wrap_err_with(|| format!("handle for {} task failed", Self::task()))?;
        }

        debug!("task {} disconnected, closing", Self::task());

        Ok(())
    }
}
