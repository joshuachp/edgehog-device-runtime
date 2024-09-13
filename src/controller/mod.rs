use actor::Actor;
use astarte_device_sdk::{client::RecvError, FromEvent};
use log::{error, info};
use message::{LedEvent, RuntimeEvent, TelemetryEvent};
use stable_eyre::eyre::Error;
use tokio::{
    sync::{mpsc, RwLock},
    task::JoinSet,
};

use crate::{
    commands::execute_command,
    data::{Publisher, Subscriber},
    error::DeviceManagerError,
    led_behavior::LedBlink,
    ota::ota_handler::OtaHandler,
    telemetry::Telemetry,
    DeviceManagerOptions,
};

pub mod actor;
pub mod message;

#[derive(Debug)]
struct Runtime<T> {
    client: T,
    ota_handler: OtaHandler,
    led_tx: mpsc::Sender<LedEvent>,
    telemetry_tx: mpsc::Sender<TelemetryEvent>,

    #[cfg(feature = "forwarder")]
    forwarder: crate::forwarder::Forwarder<T>,
}

impl<T> Runtime<T> {
    pub async fn new(
        tasks: &mut JoinSet<stable_eyre::Result<()>>,
        opts: DeviceManagerOptions,
        client: T,
    ) -> Result<Self, DeviceManagerError>
    where
        T: Publisher + Send + Sync + Clone + 'static,
    {
        #[cfg(feature = "systemd")]
        crate::systemd_wrapper::systemd_notify_status("Initializing");

        info!("Initializing");

        let ota_handler = OtaHandler::start(tasks, client.clone(), &opts).await?;

        let (led_tx, led_rx) = mpsc::channel(8);
        tasks.spawn(LedBlink.spawn(led_rx));

        let (telemetry_tx, telemetry_rx) = mpsc::channel(8);

        let telemetry = Telemetry::from_config(
            client.clone(),
            &opts.telemetry_config.unwrap_or_default(),
            opts.store_directory.clone(),
        )
        .await;

        tasks.spawn(telemetry.spawn(telemetry_rx));

        #[cfg(feature = "forwarder")]
        // Initialize the forwarder instance
        let forwarder = crate::forwarder::Forwarder::init(client.clone()).await?;

        Ok(Self {
            client,
            ota_handler,
            led_tx,
            telemetry_tx,
            #[cfg(feature = "forwarder")]
            forwarder,
        })
    }

    pub async fn run(&mut self) -> Result<(), DeviceManagerError>
    where
        T: Subscriber + Publisher + Clone + Send + Sync + 'static,
    {
        #[cfg(feature = "systemd")]
        crate::systemd_wrapper::systemd_notify_status("Running");

        info!("Running");

        loop {
            let event = match self.client.recv().await {
                Ok(event) => RuntimeEvent::from_event(event)?,
                Err(RecvError::Disconnected) => {
                    error!("the Runtime was diconnected");

                    return Ok(());
                }
                Err(err) => {
                    error!("error received: {}", Error::from(err));

                    continue;
                }
            };

            self.handle_event(event);
        }
    }

    async fn handle_event(&mut self, event: RuntimeEvent)
    where
        T: Publisher + Clone + Send + Sync + 'static,
    {
        match event {
            RuntimeEvent::Ota(ota) => {
                if let Err(err) = self.ota_handler.handle_event(ota).await {
                    error!(
                        "error while processing ota envent {}",
                        stable_eyre::Report::new(err)
                    );
                }
            }
            RuntimeEvent::Command(cmd) => {
                if cmd.is_reboot() && self.ota_handler.in_progress() {
                    error!("cannot reboot during OTA update");

                    return;
                }

                if let Err(err) = execute_command(cmd).await {
                    error!(
                        "command failed to execute: {}",
                        stable_eyre::Report::new(err)
                    );
                }
            }
            RuntimeEvent::Telemetry(event) => {
                if self.telemetry_tx.send(event).await.is_err() {
                    error!("couldn't send the telemetry event");
                }
            }
            RuntimeEvent::Led(event) => {
                if self.led_tx.send(event).await.is_err() {
                    error!("couldn't send the led event");
                }
            }
            #[cfg(feature = "forwarder")]
            RuntimeEvent::Session(event) => {
                self.forwarder.handle_sessions(event);
            }
        }
    }
}
