//! Linux: logind tells us the computer is about to sleep
//! (`PrepareForSleep`) and the session is to lock (`Lock`, or `LockedHint`
//! turning true), over the system bus (D66 §6). Without logind, sleep is
//! still caught on waking by the clock check in `app.rs`.

use std::sync::mpsc::Sender;

use futures_util::StreamExt;

use super::Event;

const LOGIN1: &str = "org.freedesktop.login1";

async fn watch_sleep(conn: zbus::Connection, events: Sender<Event>) -> zbus::Result<()> {
    let manager = zbus::Proxy::new(
        &conn,
        LOGIN1,
        "/org/freedesktop/login1",
        "org.freedesktop.login1.Manager",
    )
    .await?;
    let mut signals = manager.receive_signal("PrepareForSleep").await?;
    while let Some(signal) = signals.next().await {
        // `true` before sleeping, `false` after waking.
        if signal.body().deserialize::<bool>().unwrap_or(false) {
            let _ = events.send(Event::Sleeping);
        }
    }
    Ok(())
}

async fn watch_lock(conn: zbus::Connection, events: Sender<Event>) -> zbus::Result<()> {
    let session = zbus::Proxy::new(
        &conn,
        LOGIN1,
        "/org/freedesktop/login1/session/auto",
        "org.freedesktop.login1.Session",
    )
    .await?;
    let mut locks = session.receive_signal("Lock").await?;
    let mut hints = session.receive_property_changed::<bool>("LockedHint").await;
    loop {
        tokio::select! {
            Some(_) = locks.next() => {
                let _ = events.send(Event::SessionLocked);
            }
            Some(change) = hints.next() => {
                if change.get().await.unwrap_or(false) {
                    let _ = events.send(Event::SessionLocked);
                }
            }
            else => return Ok(()),
        }
    }
}

/// Starts watching on a thread with its own runtime; events arrive on `events`.
pub fn watch(events: Sender<Event>) {
    std::thread::spawn(move || {
        let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        else {
            return;
        };
        runtime.block_on(async move {
            let Ok(conn) = zbus::Connection::system().await else {
                return;
            };
            let _ = tokio::join!(
                watch_sleep(conn.clone(), events.clone()),
                watch_lock(conn, events),
            );
        });
    });
}
