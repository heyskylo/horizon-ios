//! A guest's `horizon:perf` requests (docs/perf-platform.md §4.2): who may profile this server, the one capture at a time, and the log lines about it.
use super::Game;
use super::integrated::capture::{Live, Target};
use super::lan::LanHost;
use super::profile_capture::{reports_dir, shown};
use horizon_client::perf_net::{
    CHANNEL, Held, OPERATOR_LEVEL, Refusal, Reply, Request, Verdict, Wanted, judge,
};
use horizon_perf_format::v1::{StopReason, Trigger};
use horizon_protocol::packets::{configuration::CustomPayload, play};

/// The guest whose capture runs on this server.
pub(in crate::game) struct Requester {
    conn: u32,
    name: String,
}

/// A request that passed the checks and waits for the tick loop that owns the capture state.
pub(in crate::game) enum Asked {
    Start {
        conn: u32,
        name: String,
        wanted: Wanted,
    },
    Stop,
}

/// What a guest has asked of this server, for the tick loop.
#[derive(Default)]
pub(in crate::game) struct Served {
    requester: Option<Requester>,
    asked: Vec<Asked>,
}

/// How a server capture's file ended.
pub(in crate::game) enum Done {
    Saved { path: String, removed: Vec<String> },
    Failed,
}

fn payload(reply: &Reply) -> play::Clientbound {
    play::Clientbound::CustomPayload(CustomPayload {
        channel: CHANNEL.into(),
        data: reply.encode(),
    })
}

fn window(wanted: &Wanted) -> String {
    wanted
        .seconds
        .map_or_else(|| "until stopped".to_owned(), |s| format!("{s} s"))
}

impl Game<'_> {
    /// A `horizon:perf` payload from `conn`.
    pub(super) fn guest_perf(&mut self, host: &mut LanHost, conn: u32, data: &[u8]) {
        let Some(guest) = host.hub.guest(conn) else {
            return;
        };
        let name = guest.name.clone();
        let Ok(request) = Request::decode(data) else {
            log::debug!("Horizon: a bad server profile request from {name}");
            return;
        };
        let served = &mut self.profile.served;
        let held = match &served.requester {
            Some(r) if r.conn == conn => Held::Asker,
            Some(_) => Held::Someone,
            None if served
                .asked
                .iter()
                .any(|a| matches!(a, Asked::Start { .. })) =>
            {
                Held::Someone
            }
            None => Held::Nothing,
        };
        match judge(&guest.brand, guest.permission, &request, held) {
            Verdict::Ignore => {}
            Verdict::Refuse(why) => {
                if why == Refusal::Denied {
                    log::warn!(
                        "Horizon: {name} asked for a server profile and was refused: operator level {OPERATOR_LEVEL} is required"
                    );
                }
                guest.send(&payload(&Reply::Refused(why)));
            }
            Verdict::Accept => served.asked.push(match request {
                Request::Start(wanted) => Asked::Start { conn, name, wanted },
                Request::Stop => Asked::Stop,
            }),
        }
    }

    /// Sends `reply` to the guest on `conn`, if it is still here.
    pub(in crate::game) fn perf_reply(&self, conn: u32, reply: &Reply) {
        if let Some(guest) = self.lan.as_ref().and_then(|h| h.hub.guest(conn)) {
            guest.send(&payload(reply));
        }
    }

    pub(in crate::game) fn take_perf_asked(&mut self) -> Vec<Asked> {
        std::mem::take(&mut self.profile.served.asked)
    }

    /// The capture `name` asked for runs, in the file at `path`.
    pub(in crate::game) fn perf_started(
        &mut self,
        conn: u32,
        name: String,
        wanted: &Wanted,
        path: &str,
    ) {
        log::info!(
            "Horizon: {name} started a server profile ({}, samples {})",
            window(wanted),
            if wanted.samples { "on" } else { "off" }
        );
        self.profile.served.requester = Some(Requester { conn, name });
        self.perf_reply(
            conn,
            &Reply::Started {
                path: path.to_owned(),
                seconds: wanted.seconds,
                samples: wanted.samples,
            },
        );
    }

    /// The capture `conn` asked for could not begin.
    pub(in crate::game) fn perf_refused(&self, conn: u32, why: Refusal) {
        self.perf_reply(conn, &Reply::Refused(why));
    }

    /// Server captures whose files are done: the guest that asked is told, and the log names it.
    pub(in crate::game) fn perf_captures_done(&mut self, done: Vec<Done>) {
        for done in done {
            let Some(Requester { conn, name }) = self.profile.served.requester.take() else {
                continue;
            };
            match done {
                Done::Saved { path, removed } => {
                    log::info!("Horizon: {name}'s server profile saved to {path}");
                    self.perf_reply(conn, &Reply::Finished { path, removed });
                }
                Done::Failed => {
                    log::error!("Horizon: {name}'s server profile could not be written");
                    self.perf_refused(conn, Refusal::Failed);
                }
            }
        }
    }

    /// On the server thread, between two ticks: begins and ends the captures guests asked for.
    pub(super) fn serve_perf_asked(&mut self) {
        for asked in self.take_perf_asked() {
            match asked {
                Asked::Start { conn, name, wanted } => {
                    if self.server_capture_busy() {
                        self.perf_refused(conn, Refusal::AlreadyRunning);
                        continue;
                    }
                    let target = Target::Own {
                        options: wanted.options(),
                        trigger: Trigger::Remote,
                        dir: reports_dir(),
                    };
                    match self.start_server_capture(target, Live::new()) {
                        Ok(path) => self.perf_started(conn, name, &wanted, &shown(&path)),
                        Err(e) => {
                            log::error!("Horizon: {name}'s server profile did not start: {e}");
                            self.perf_refused(conn, Refusal::Failed);
                        }
                    }
                }
                Asked::Stop => self.stop_server_capture(StopReason::Normal, None),
            }
        }
    }
}
