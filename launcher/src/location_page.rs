//! Settings' two-step data move. Only the separate worker copies data, after this process exits.
use super::*;

fn env_value(key: &str) -> Option<String> {
    std::env::var_os(key)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_string_lossy().into_owned())
}

pub(super) fn startup_error() -> Option<String> {
    horizon_launcher::data_location::startup_error()
        .map(|error| {
            trf(
                "Could not open the data folder: {error}",
                &[("error", &error)],
            )
        })
        .or_else(|| {
            env_value("HORIZON_RELOCATION_ERROR")
                .map(|error| trf("Data move failed: {error}", &[("error", &error)]))
        })
}

fn open_error(error: &str) -> String {
    trf(
        "Could not open the data folder: {error}",
        &[("error", error)],
    )
}

pub(super) fn startup_note() -> Option<String> {
    env_value("HORIZON_RELOCATION_BACKUP").map(|paths| {
        trf(
            "Data moved. Old folders still use disk space: {paths}. Verify your worlds and instances before removing them yourself.",
            &[("paths", &paths)],
        )
    })
}

fn environment_blocked() -> bool {
    ["HORIZON_LAUNCHER_HOME", "HORIZON_HOME"]
        .iter()
        .any(|key| std::env::var_os(key).is_some_and(|value| !value.is_empty()))
}

impl Launcher {
    fn location_busy(&self) -> bool {
        self.installing.is_some()
            || !self.removing.is_empty()
            || self.importing_build
            || self.native_step.is_some()
            || self.preparing
            || self.client.is_some()
            || self.starting_client.is_some()
            || self.starting_pack.is_some()
            || self.pack_busy
            || self.mods.busy.is_some()
            || !self.auth_busy.is_empty()
            || (self.sign_in.is_some() && !matches!(self.sign_in_state, Some(SignInState::Done(_))))
            || matches!(self.self_update, SelfUpdate::Downloading(..))
            || self.servers.location_busy()
            || self.importing.location_busy()
            || self.github.busy()
            || self.worlds.location_busy()
            || self.instances.location_busy()
            || self.internet.location_busy()
            || self.export.as_ref().is_some_and(|p| p.location_busy())
    }

    fn location_blocked(&self) -> Option<String> {
        if environment_blocked() {
            Some(tr("Data folders are set by HORIZON_LAUNCHER_HOME or HORIZON_HOME. Remove these environment overrides and restart the launcher before moving data here.").into())
        } else if self.location_busy() {
            Some(tr("Close the game and servers, and wait for downloads, builds, imports and sign-in to finish before moving data.").into())
        } else {
            None
        }
    }

    fn choose_location(&mut self, cx: &mut Context<Self>) {
        self.location_error = self.location_blocked();
        if self.location_error.is_some() {
            cx.notify();
            return;
        }
        self.location_target = None;
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(tr("Choose data folder").into()),
        });
        cx.spawn(async move |this, cx| {
            let result = match paths.await {
                Ok(Ok(paths)) => Ok(paths.and_then(|mut paths| paths.pop())),
                Ok(Err(error)) => Err(error.to_string()),
                Err(error) => Err(error.to_string()),
            };
            let _ = this.update(cx, |this, cx| {
                this.location_error = this.location_blocked();
                if this.location_error.is_none() {
                    match result {
                        Ok(Some(path)) => match std::fs::read_dir(&path) {
                            Ok(mut entries) => match entries.next() {
                                None => this.location_target = Some(path),
                                Some(Ok(_)) => {
                                    this.location_error = Some(
                                        tr("Choose an empty folder for Horizon's data.").into(),
                                    );
                                }
                                Some(Err(error)) => this.location_picker_error(error.to_string()),
                            },
                            Err(error) => this.location_picker_error(error.to_string()),
                        },
                        Ok(None) => {}
                        Err(error) => this.location_picker_error(error),
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// Try the configured folder again. A success restarts the launcher, because paths already
    /// resolved to the fallback folders cannot be swapped under a running window.
    fn retry_location(&mut self, cx: &mut Context<Self>) {
        match horizon_launcher::data_location::retry() {
            Ok(()) => {
                self.location_error = Some(backend::restart());
            }
            Err(error) => self.location_error = Some(open_error(&error)),
        }
        cx.notify();
    }

    /// Keep the default folders: the pointer is kept beside the old one as `location.json.old`,
    /// and nothing in the moved folders is deleted.
    fn use_default_location(&mut self, cx: &mut Context<Self>) {
        match horizon_launcher::data_location::use_default_folder() {
            Ok(()) => {
                self.location_error = Some(backend::restart());
            }
            Err(error) => self.location_error = Some(open_error(&error)),
        }
        cx.notify();
    }

    fn location_picker_error(&mut self, error: String) {
        self.location_error = Some(trf(
            "Could not choose data folder: {error}",
            &[("error", &error)],
        ));
    }

    fn confirm_location(&mut self, cx: &mut Context<Self>) {
        self.location_error = self.location_blocked();
        if self.location_error.is_some() {
            cx.notify();
            return;
        }
        if self.settings_save_error.is_some() && !self.save_settings() {
            cx.notify();
            return;
        }
        let Some(target) = self.location_target.as_ref() else {
            return;
        };
        let result = std::env::current_exe().and_then(|exe| {
            horizon_launcher::process::command(exe)
                .arg("--relocate-data")
                .arg(target)
                .arg("--restart")
                .spawn()
        });
        match result {
            Ok(_) => {
                // No installer while the worker copies the data folder its stage is in.
                horizon_launcher::update_handoff::forget();
                cx.quit()
            }
            Err(error) => {
                self.location_error = Some(trf(
                    "Could not start data move: {error}",
                    &[("error", &error.to_string())],
                ));
                cx.notify();
            }
        }
    }

    pub(super) fn location_row(&self, n: usize, cx: &mut Context<Self>) -> Div {
        let t = self.theme;
        let blocked = self.location_blocked();
        let move_button = button("move-data", "Move…", Kind::Secondary, &t);
        let move_button = if blocked.is_some() {
            disabled(move_button)
        } else {
            move_button.on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                this.choose_location(cx);
            }))
        };
        let mut body = div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(index_row(
                n,
                tr("Data folder"),
                Some(div().font_family(theme::MONO).child(short(&backend::data_dir())).into_any_element()),
                Some(move_button.into_any_element()),
                &t,
            ))
            .child(text(theme::CAPTION).text_color(t.muted).child(tr(
                "Moves all launcher, game and build data, including settings, worlds, instances and installed versions. Choose an empty folder with enough free space.",
            ).to_string()))
            .child(text(theme::CAPTION).text_color(t.muted).child(tr(
                "The launcher closes while your data is copied and verified, then reopens. Old folders are kept and still use disk space until you verify the move and remove them yourself.",
            ).to_string()))
            .children(blocked.map(|message| text(theme::CAPTION).text_color(t.warn).child(message)))
            .children(self.location_note.as_ref().map(|message| {
                text(theme::CAPTION).text_color(t.ink).child(message.clone())
            }))
            .children(self.location_error.as_ref().map(|message| {
                text(theme::CAPTION).text_color(t.danger).child(message.clone())
            }));
        if horizon_launcher::data_location::startup_error().is_some() {
            body = body.child(
                div()
                    .flex()
                    .gap(px(8.))
                    .child(
                        button("retry-data-location", "Retry", Kind::Primary, &t).on_click(
                            cx.listener(|this, _: &ClickEvent, _, cx| this.retry_location(cx)),
                        ),
                    )
                    .child(
                        button(
                            "use-default-data-location",
                            "Use the default folder",
                            Kind::Secondary,
                            &t,
                        )
                        .on_click(cx.listener(
                            |this, _: &ClickEvent, _, cx| {
                                this.use_default_location(cx);
                            },
                        )),
                    ),
            );
        }
        if let Some(target) = &self.location_target {
            let confirm = button("confirm-move-data", "Move and restart", Kind::Primary, &t);
            let confirm = if self.location_blocked().is_some() {
                disabled(confirm)
            } else {
                confirm.on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.confirm_location(cx);
                }))
            };
            body = body.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .p(px(12.))
                    .bg(t.well)
                    .child(text(theme::UI).child(trf(
                        "Move all data to {path}?",
                        &[("path", &target.display().to_string())],
                    )))
                    .child(div().flex().gap(px(8.)).child(confirm).child(
                        button("cancel-move-data", "Cancel", Kind::Secondary, &t).on_click(
                            cx.listener(|this, _: &ClickEvent, _, cx| {
                                this.location_target = None;
                                this.location_error = None;
                                cx.notify();
                            }),
                        ),
                    )),
            );
        }
        body.mb(px(16.))
    }
}
