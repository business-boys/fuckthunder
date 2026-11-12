//! Independent mailbox state for one open client tab.

use std::{sync::Arc, time::Duration};

use anyhow::{anyhow, Result};
use fuckthunder_gui::{
    parse_proxy, Account, MailSession, MessagePage, MessageView, SessionOptions, Store,
    StoredAccount, PAGE_SIZE,
};
use gpui::{div, prelude::*, px, rgb, uniform_list, Context, Entity, Subscription, Window};
use gpui_component::{
    button::{Button, ButtonVariants},
    input::{Input, InputEvent, InputState},
    resizable::{h_resizable, resizable_panel, ResizableState},
    spinner::Spinner,
    Disableable, Icon, IconName, Sizable,
};
use tokio::{runtime::Runtime, sync::Mutex, task::AbortHandle};

use super::form::{Form, Submit};

const BG: u32 = 0x17191d;
const PANEL: u32 = 0x1d2025;
const BORDER: u32 = 0x30343b;
const TEXT: u32 = 0xe7e9ee;
const MUTED: u32 = 0x9ca4b1;
const MINT: u32 = 0x75b9ff;
const HOVER: u32 = 0x242a33;
const SELECTED: u32 = 0x25364b;

fn folder_label(folder: &str) -> &str {
    if folder.eq_ignore_ascii_case("INBOX") {
        "Inbox"
    } else {
        folder
    }
}

fn sender_name(sender: &str) -> &str {
    sender.split_once(" <").map_or(sender, |(name, _)| name)
}

pub(super) struct Mailbox {
    runtime: Arc<Runtime>,
    store: Arc<Store>,
    form: Entity<Form>,
    _subscriptions: Vec<Subscription>,
    session: Arc<Mutex<Option<MailSession>>>,
    search: Entity<InputState>,
    query: String,
    accounts: Vec<StoredAccount>,
    active: Option<StoredAccount>,
    folders: Vec<String>,
    folder: Option<String>,
    page: MessagePage,
    detail: Option<MessageView>,
    local_title: Option<String>,
    selected_uid: Option<u32>,
    adding: bool,
    busy: bool,
    error: Option<String>,
    status: String,
    operation: Option<AbortHandle>,
    generation: u64,
    clear_form_password: bool,
    panels: Entity<ResizableState>,
    viewer: Entity<super::viewer::Viewer>,
}

impl Mailbox {
    pub(super) fn new(
        runtime: Arc<Runtime>,
        store: Arc<Store>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let form = cx.new(|cx| Form::new(window, cx));
        form.update(cx, |form, cx| form.focus(window, cx));
        let submit = cx.subscribe_in(&form, window, |this, _, event: &Submit, window, cx| {
            this.connect_form(event, window, cx);
        });
        let search = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Search this folder · Enter")
                .validate(|value, _| !value.contains(['\r', '\n']))
        });
        let search_subscription =
            cx.subscribe_in(&search, window, |this, _, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    if let Some(folder) = this.folder.clone() {
                        this.query = this.search.read(cx).value().trim().to_owned();
                        this.open_folder(folder, 0, cx);
                    }
                }
            });
        let (accounts, error) = match store.accounts() {
            Ok(accounts) => (accounts, None),
            Err(error) => (
                Vec::new(),
                Some(format!("Cannot read saved mailboxes: {error}")),
            ),
        };
        let viewer = cx.new(|cx| super::viewer::Viewer::new(runtime.clone(), cx));
        Self {
            runtime,
            store,
            form,
            _subscriptions: vec![submit, search_subscription],
            session: Arc::new(Mutex::new(None)),
            search,
            query: String::new(),
            accounts,
            active: None,
            folders: Vec::new(),
            folder: None,
            page: MessagePage::default(),
            detail: None,
            local_title: None,
            selected_uid: None,
            adding: true,
            busy: false,
            error,
            status: "Connect a mailbox to open your inbox".into(),
            operation: None,
            generation: 0,
            clear_form_password: false,
            viewer,
            panels: cx.new(|_| ResizableState::default()),
        }
    }

    pub(super) fn refresh_accounts(&mut self, cx: &mut Context<Self>) {
        match self.store.accounts() {
            Ok(accounts) => {
                if let Some(active) = &self.active {
                    if !accounts.iter().any(|account| account.id == active.id) {
                        self.cancel(cx);
                        self.active = None;
                        self.folders.clear();
                        self.folder = None;
                        self.page = MessagePage::default();
                        self.viewer
                            .update(cx, |viewer, cx| viewer.set_message(None, cx));
                        self.detail = None;
                        self.adding = true;
                    }
                }
                self.accounts = accounts;
            }
            Err(error) => self.error = Some(format!("Cannot read saved mailboxes: {error}")),
        }
        cx.notify();
    }

    pub(super) fn title(&self, cx: &gpui::App) -> String {
        if let Some(title) = &self.local_title {
            return title.clone();
        }
        self.active.as_ref().map_or_else(
            || {
                let email = self.form.read(cx).email(cx);
                if email.is_empty() {
                    "New mailbox".into()
                } else {
                    email
                }
            },
            |account| account.account.email.clone(),
        )
    }

    pub(super) fn is_adding(&self) -> bool {
        self.adding
    }

    pub(super) fn is_local_message(&self) -> bool {
        self.local_title.is_some()
    }

    pub(super) fn account_id(&self) -> Option<i64> {
        self.active.as_ref().map(|account| account.id)
    }

    pub(super) fn is_busy(&self) -> bool {
        self.busy
    }

    pub(super) fn has_error(&self) -> bool {
        self.error.is_some()
    }

    #[cfg(test)]
    pub(super) fn set_context(&mut self, query: &str, uid: u32) {
        self.query = query.to_owned();
        self.selected_uid = Some(uid);
    }

    pub(super) fn hide_webview(&self, cx: &gpui::App) {
        self.viewer.read(cx).hide_webview();
    }

    pub(super) fn open_local_message(&mut self, path: &std::path::Path, cx: &mut Context<Self>) {
        self.cancel(cx);
        let generation = self.generation;
        let path = path.to_owned();
        let title = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let executor = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let result = executor
                .spawn(async move {
                    std::fs::read(path)
                        .map_err(|error| error.to_string())
                        .and_then(|raw| {
                            fuckthunder_gui::decode_message(&raw).map_err(|error| error.to_string())
                        })
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.generation != generation {
                    return;
                }
                match result {
                    Ok(message) => {
                        this.local_title = Some(title);
                        this.viewer.update(cx, |viewer, cx| {
                            viewer.set_message(Some(message.clone()), cx)
                        });
                        this.detail = Some(message);
                        this.selected_uid = Some(0);
                        this.adding = false;
                        this.status = "Local message · read only".into();
                    }
                    Err(error) => this.error = Some(format!("Open message failed: {error}")),
                }
                cx.notify();
            });
        })
        .detach();
    }

    #[cfg(test)]
    pub(super) fn context(&self) -> (&str, Option<u32>) {
        (&self.query, self.selected_uid)
    }

    pub(super) fn focus(&self, window: &mut Window, cx: &mut gpui::App) {
        if self.adding {
            self.form.update(cx, |form, cx| form.focus(window, cx));
        }
    }

    fn options(&self, account: Option<&StoredAccount>) -> Result<SessionOptions> {
        let settings = self.store.workspace_settings()?;
        let proxy = match account.and_then(|account| account.proxy_id) {
            Some(id) => {
                let proxies = self.store.proxies()?;
                let proxy = proxies.iter().find(|proxy| proxy.id == id)
                    .ok_or_else(|| anyhow!("The saved proxy route is missing. Add the mailbox again to choose a route."))?;
                Some(parse_proxy(&proxy.url)?)
            }
            None => None,
        };
        Ok(SessionOptions {
            proxy,
            timeout: Duration::from_secs(settings.timeout_seconds.max(1)),
        })
    }

    fn begin(&mut self, status: &str, cx: &mut Context<Self>) -> u64 {
        self.cancel(cx);
        self.busy = true;
        self.error = None;
        self.status = status.into();
        self.form.update(cx, |form, cx| {
            form.set_error(None, cx);
            form.set_busy(true, cx);
        });
        cx.notify();
        self.generation
    }

    fn cancel(&mut self, cx: &mut Context<Self>) {
        let cancelled = self.operation.take().is_some_and(|operation| {
            operation.abort();
            true
        });
        self.generation = self.generation.wrapping_add(1);
        if cancelled {
            self.reset_session();
        }
        self.busy = false;
        self.form.update(cx, |form, cx| form.set_busy(false, cx));
    }

    fn finish(&mut self, generation: u64, cx: &mut Context<Self>) -> bool {
        if generation != self.generation {
            return false;
        }
        self.operation = None;
        self.busy = false;
        self.form.update(cx, |form, cx| form.set_busy(false, cx));
        true
    }

    fn reset_session(&mut self) {
        self.session = Arc::new(Mutex::new(None));
    }

    fn retry(&mut self, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        if let Some(uid) = self.selected_uid {
            self.open_message(uid, cx);
        } else if let Some(folder) = self.folder.clone() {
            self.open_folder(folder, self.page.page, cx);
        } else if let Some(account) = self.active.as_ref().map(|account| account.id) {
            self.open_saved(account, cx);
        }
    }

    fn fail(&mut self, error: impl std::fmt::Display, cx: &mut Context<Self>) {
        self.reset_session();
        let message = error.to_string();
        self.status = "Connection needs attention".into();
        self.error = Some(message.clone());
        if self.adding {
            self.form
                .update(cx, |form, cx| form.set_error(Some(message), cx));
        }
        cx.notify();
    }

    fn connect_form(&mut self, input: &Submit, _window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let account = match Account::new(
            &input.email,
            &input.password,
            input.host.as_deref(),
            input.port,
            input.starttls,
        ) {
            Ok(account) => account,
            Err(error) => {
                self.fail(error, cx);
                return;
            }
        };
        let mut options = match self.options(None) {
            Ok(options) => options,
            Err(error) => {
                self.fail(error, cx);
                return;
            }
        };
        options.proxy = match input.proxy.as_deref().map(parse_proxy).transpose() {
            Ok(proxy) => proxy,
            Err(error) => {
                self.fail(format!("Invalid proxy URL: {error}"), cx);
                return;
            }
        };
        self.connect(account, options, input.proxy.clone(), None, cx);
    }

    pub(super) fn open_saved(&mut self, id: i64, cx: &mut Context<Self>) {
        self.refresh_accounts(cx);
        let Some(account) = self
            .accounts
            .iter()
            .find(|account| account.id == id)
            .cloned()
        else {
            self.fail("This mailbox no longer exists in the local workspace", cx);
            return;
        };
        let options = match self.options(Some(&account)) {
            Ok(options) => options,
            Err(error) => {
                self.fail(error, cx);
                return;
            }
        };
        self.adding = false;
        self.connect(account.account.clone(), options, None, Some(account), cx);
    }

    fn connect(
        &mut self,
        account: Account,
        options: SessionOptions,
        proxy: Option<String>,
        saved: Option<StoredAccount>,
        cx: &mut Context<Self>,
    ) {
        self.local_title = None;
        let generation = self.begin("Connecting and opening inbox…", cx);
        self.folders.clear();
        self.folder = None;
        self.page = MessagePage::default();
        self.viewer
            .update(cx, |viewer, cx| viewer.set_message(None, cx));
        self.detail = None;
        self.selected_uid = None;
        self.active = saved.clone();
        let task = self.runtime.spawn(async move {
            let result = tokio::time::timeout(options.timeout, async {
                let mut session = MailSession::connect(&account, &options).await?;
                let folders = session.folders().await?;
                let inbox = folders
                    .iter()
                    .find(|folder| folder.name.eq_ignore_ascii_case("INBOX"))
                    .or_else(|| folders.first())
                    .map(|folder| folder.name.clone())
                    .ok_or_else(|| anyhow!("This mailbox has no selectable folders"))?;
                let page = session.page(&inbox, 0, "").await?;
                Ok::<_, anyhow::Error>((session, folders, inbox, page))
            })
            .await
            .map_err(|_| {
                anyhow!("Connection timed out. Check the server, proxy and app password.")
            })?;
            result.map(|data| (account, proxy, saved, data))
        });
        self.operation = Some(task.abort_handle());
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                if !this.finish(generation, cx) {
                    return;
                }
                match result {
                    Ok(Ok((account, proxy, saved, (session, folders, folder, page)))) => {
                        this.session = Arc::new(Mutex::new(Some(session)));
                        let id = match saved {
                            Some(saved) => Ok(saved.id),
                            None => this.store.save_connection(&account, proxy.as_deref()),
                        };
                        match id {
                            Ok(id) => {
                                this.refresh_accounts(cx);
                                this.active = this
                                    .accounts
                                    .iter()
                                    .find(|account| account.id == id)
                                    .cloned();
                                this.folders =
                                    folders.into_iter().map(|folder| folder.name).collect();
                                this.folder = Some(folder);
                                this.page = page;
                                this.adding = false;
                                this.status = "Connected · mailbox opened read-only".into();
                                this.clear_form_password = true;
                                cx.emit(());
                            }
                            Err(error) => this.fail(
                                format!("Connected, but could not save this mailbox: {error}"),
                                cx,
                            ),
                        }
                    }
                    Ok(Err(error)) => this.fail(error, cx),
                    Err(error) if error.is_cancelled() => {}
                    Err(error) => this.fail(error, cx),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn open_folder(&mut self, folder: String, page_number: usize, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let Some(active) = self.active.clone() else {
            return;
        };
        let options = match self.options(Some(&active)) {
            Ok(options) => options,
            Err(error) => {
                self.fail(error, cx);
                return;
            }
        };
        let generation = self.begin("Loading messages…", cx);
        self.viewer
            .update(cx, |viewer, cx| viewer.set_message(None, cx));
        self.detail = None;
        self.selected_uid = None;
        let session = self.session.clone();
        let query = self.query.clone();
        let task = self.runtime.spawn(async move {
            let requested_folder = folder.clone();
            tokio::time::timeout(options.timeout, async move {
                let mut guard = session.lock().await;
                match guard.as_mut() {
                    Some(session) => session.page(&folder, page_number, &query).await,
                    None => {
                        let mut connected = MailSession::connect(&active.account, &options).await?;
                        let result = connected.page(&folder, page_number, &query).await;
                        if result.is_ok() {
                            *guard = Some(connected);
                        }
                        result
                    }
                }
            })
            .await
            .map_err(|_| anyhow!("Message list timed out"))?
            .map(|page| (requested_folder, page))
            .map_err(anyhow::Error::from)
        });
        self.operation = Some(task.abort_handle());
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                if !this.finish(generation, cx) {
                    return;
                }
                match result {
                    Ok(Ok((folder, page))) => {
                        this.folder = Some(folder);
                        this.page = page;
                        this.status = "Messages up to date · read-only".into();
                    }
                    Ok(Err(error)) => this.fail(error, cx),
                    Err(error) if error.is_cancelled() => {}
                    Err(error) => this.fail(error, cx),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn open_message(&mut self, uid: u32, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let (Some(active), Some(folder)) = (self.active.clone(), self.folder.clone()) else {
            return;
        };
        let options = match self.options(Some(&active)) {
            Ok(options) => options,
            Err(error) => {
                self.fail(error, cx);
                return;
            }
        };
        let generation = self.begin("Loading message…", cx);
        self.selected_uid = Some(uid);
        self.viewer
            .update(cx, |viewer, cx| viewer.set_message(None, cx));
        self.detail = None;
        let session = self.session.clone();
        let task = self.runtime.spawn(async move {
            tokio::time::timeout(options.timeout, async move {
                let mut guard = session.lock().await;
                match guard.as_mut() {
                    Some(session) => session.message(&folder, uid).await,
                    None => {
                        let mut connected = MailSession::connect(&active.account, &options).await?;
                        let result = connected.message(&folder, uid).await;
                        if result.is_ok() {
                            *guard = Some(connected);
                        }
                        result
                    }
                }
            })
            .await
            .map_err(|_| anyhow!("Message download timed out"))?
            .map_err(anyhow::Error::from)
        });
        self.operation = Some(task.abort_handle());
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                if !this.finish(generation, cx) {
                    return;
                }
                match result {
                    Ok(Ok(message)) => {
                        this.viewer.update(cx, |viewer, cx| {
                            viewer.set_message(Some(message.clone()), cx)
                        });
                        this.detail = Some(message);
                        this.status = "Message opened · unread flag unchanged".into();
                    }
                    Ok(Err(error)) => this.fail(error, cx),
                    Err(error) if error.is_cancelled() => {}
                    Err(error) => this.fail(error, cx),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn folder_rail(&self, cx: &mut Context<Self>) -> gpui::Div {
        let folders = self.folders.clone();
        let selected = self.folder.clone();
        let entity = cx.entity().clone();
        div()
            .debug_selector(|| "folder-pane".into())
            .w_full()
            .flex_shrink_0()
            .h_full()
            .bg(rgb(PANEL))
            .border_r_1()
            .border_color(rgb(BORDER))
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(34.))
                    .px_3()
                    .flex()
                    .items_center()
                    .text_xs()
                    .text_color(rgb(MUTED))
                    .child("Folders"),
            )
            .child(
                uniform_list("mailbox-folders", folders.len(), move |range, _, _| {
                    range
                        .map(|index| {
                            let name = folders[index].clone();
                            let active = selected.as_ref() == Some(&name);
                            let entity = entity.clone();
                            div().h(px(34.)).px_1().child(
                                Button::new(("folder", index))
                                    .label(folder_label(&name).to_owned())
                                    .small()
                                    .ghost()
                                    .w_full()
                                    .bg(rgb(if active { SELECTED } else { PANEL }))
                                    .tooltip(name.clone())
                                    .on_click(move |_, window, cx| {
                                        entity.update(cx, |this, cx| {
                                            if this.folder.as_ref() != Some(&name) {
                                                this.query.clear();
                                                this.search.update(cx, |search, cx| {
                                                    search.set_value("", window, cx)
                                                });
                                            }
                                            this.open_folder(name.clone(), 0, cx);
                                        })
                                    }),
                            )
                        })
                        .collect()
                })
                .h_full(),
            )
    }

    fn toolbar(&self, wide: bool, cx: &mut Context<Self>) -> gpui::Div {
        let current_folder = self.folder.clone();
        let page = self.page.page;
        let start = (page * PAGE_SIZE + 1).min(self.page.total);
        let end = (page * PAGE_SIZE + self.page.messages.len()).min(self.page.total);
        let previous = current_folder.clone();
        div()
            .h(px(44.))
            .px_3()
            .gap_2()
            .flex()
            .items_center()
            .border_b_1()
            .border_color(rgb(BORDER))
            .child(
                div().text_sm().font_weight(gpui::FontWeight::MEDIUM).child(
                    folder_label(self.folder.as_deref().unwrap_or("Connecting…")).to_owned(),
                ),
            )
            .child(
                div().w(px(if wide { 280. } else { 224. })).child(
                    Input::new(&self.search)
                        .small()
                        .cleanable(true)
                        .disabled(self.busy)
                        .bg(rgb(PANEL))
                        .border_color(rgb(BORDER))
                        .text_color(rgb(TEXT)),
                ),
            )
            .child(div().flex_1())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(MUTED))
                            .whitespace_nowrap()
                            .child(if self.query.is_empty() {
                                format!("{start}–{end} of {}", self.page.total)
                            } else {
                                format!("{} results", self.page.total)
                            }),
                    )
                    .child(
                        Button::new("previous-mail")
                            .label("‹")
                            .xsmall()
                            .ghost()
                            .tooltip("Previous page")
                            .disabled(self.busy || page == 0 || self.page.total == 0)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(folder) = previous.clone() {
                                    this.open_folder(folder, page.saturating_sub(1), cx);
                                }
                            })),
                    )
                    .child(
                        Button::new("next-mail")
                            .label("›")
                            .xsmall()
                            .ghost()
                            .tooltip("Next page")
                            .disabled(
                                self.busy
                                    || self.page.total == 0
                                    || (page + 1) * PAGE_SIZE >= self.page.total,
                            )
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(folder) = current_folder.clone() {
                                    this.open_folder(folder, page + 1, cx);
                                }
                            })),
                    ),
            )
            .child(if self.busy {
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(Spinner::new().xsmall())
                    .child(
                        Button::new("cancel-client-operation")
                            .label("Cancel")
                            .small()
                            .ghost()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.cancel(cx);
                                this.status = "Connection cancelled".into();
                                cx.notify();
                            })),
                    )
                    .into_any_element()
            } else {
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        Button::new("refresh-mail")
                            .label("Refresh")
                            .xsmall()
                            .ghost()
                            .tooltip("Refresh (read-only)")
                            .disabled(self.active.is_none())
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(folder) = this.folder.clone() {
                                    this.open_folder(folder, this.page.page, cx);
                                } else if let Some(account) = &this.active {
                                    this.open_saved(account.id, cx);
                                }
                            })),
                    )
                    .child(
                        Button::new("disconnect-mail")
                            .label("Disconnect")
                            .xsmall()
                            .ghost()
                            .tooltip("Disconnect")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.cancel(cx);
                                this.reset_session();
                                this.active = None;
                                this.folders.clear();
                                this.folder = None;
                                this.page = MessagePage::default();
                                this.viewer
                                    .update(cx, |viewer, cx| viewer.set_message(None, cx));
                                this.detail = None;
                                this.selected_uid = None;
                                this.adding = true;
                                this.error = None;
                                this.status = "Disconnected · saved mailbox kept locally".into();
                                this.form.update(cx, |form, cx| form.focus(window, cx));
                                cx.notify();
                            })),
                    )
                    .into_any_element()
            })
    }

    fn notice(&self, cx: &mut Context<Self>) -> gpui::Div {
        let message = self.error.clone().unwrap_or_else(|| self.status.clone());
        div()
            .px_3()
            .py_2()
            .flex()
            .items_center()
            .gap_2()
            .bg(rgb(if self.error.is_some() {
                0x2a1d1a
            } else {
                PANEL
            }))
            .border_b_1()
            .border_color(rgb(BORDER))
            .text_sm()
            .child(Icon::new(IconName::TriangleAlert).small().text_color(rgb(
                if self.error.is_some() {
                    0xffc4ae
                } else {
                    MUTED
                },
            )))
            .child(div().flex_1().min_w_0().line_clamp(2).child(message))
            .child(
                Button::new("retry-mail-operation")
                    .label("Retry")
                    .small()
                    .ghost()
                    .on_click(cx.listener(|this, _, _, cx| this.retry(cx))),
            )
    }

    fn message_list(&self, stacked: bool, _wide: bool, cx: &mut Context<Self>) -> gpui::Div {
        let messages = self.page.messages.clone();
        let entity = cx.entity().clone();
        let selected = self.selected_uid;
        let list = uniform_list("client-messages", messages.len(), move |range, _, _| {
            range
                .map(|index| {
                    let message = &messages[index];
                    let uid = message.uid;
                    let entity = entity.clone();
                    div()
                        .id(("message", uid as u64))
                        .h(px(58.))
                        .px_3()
                        .py_2()
                        .border_b_1()
                        .border_l_2()
                        .border_color(rgb(BORDER))
                        .relative()
                        .when(selected == Some(uid), |row| {
                            row.child(
                                div()
                                    .absolute()
                                    .left_0()
                                    .top_0()
                                    .bottom_0()
                                    .w(px(2.))
                                    .bg(rgb(MINT)),
                            )
                        })
                        .bg(rgb(if selected == Some(uid) { SELECTED } else { BG }))
                        .hover(|row| {
                            row.bg(rgb(if selected == Some(uid) {
                                SELECTED
                            } else {
                                HOVER
                            }))
                        })
                        .cursor_pointer()
                        .on_click(move |_, _, cx| {
                            entity.update(cx, |this, cx| this.open_message(uid, cx))
                        })
                        .child(
                            div()
                                .flex()
                                .gap_2()
                                .items_center()
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .truncate()
                                        .text_sm()
                                        .font_weight(gpui::FontWeight::MEDIUM)
                                        .child(sender_name(&message.from).to_owned()),
                                )
                                .child(
                                    div()
                                        .flex_shrink_0()
                                        .text_xs()
                                        .text_color(rgb(MUTED))
                                        .child(
                                            chrono::DateTime::parse_from_rfc2822(&message.date)
                                                .map(|date| date.format("%d %b").to_string())
                                                .unwrap_or_else(|_| message.date.clone()),
                                        ),
                                ),
                        )
                        .child(
                            div()
                                .mt_1()
                                .truncate()
                                .text_xs()
                                .text_color(rgb(0xb9c1ce))
                                .child(message.subject.clone()),
                        )
                })
                .collect()
        })
        .h_full();
        let mut pane = div()
            .debug_selector(|| "message-pane".into())
            .h_full()
            .flex()
            .flex_col()
            .min_w_0()
            .border_r_1()
            .border_color(rgb(BORDER));
        if !stacked {
            pane = pane.w_full();
        } else {
            pane = pane.flex_1();
        }
        if self.busy && !self.page.messages.is_empty() {
            pane = pane.opacity(0.55);
        }
        pane.child(if self.page.messages.is_empty() {
            div()
                .flex_1()
                .p_5()
                .text_sm()
                .text_color(rgb(MUTED))
                .child(if self.busy {
                    "Connecting and loading messages…".into()
                } else if self.query.is_empty() {
                    format!(
                        "No messages in {}",
                        folder_label(self.folder.as_deref().unwrap_or("this folder"))
                    )
                } else {
                    format!("No results for \"{}\"", self.query)
                })
                .into_any_element()
        } else {
            div().flex_1().min_h_0().child(list).into_any_element()
        })
    }

    fn reader(&self, _: bool, _: &mut Window, _: &mut Context<Self>) -> gpui::Div {
        div()
            .debug_selector(|| "reader-pane".into())
            .size_full()
            .child(self.viewer.clone())
    }
}

impl gpui::EventEmitter<()> for Mailbox {}

impl Drop for Mailbox {
    fn drop(&mut self) {
        if let Some(operation) = self.operation.take() {
            operation.abort();
        }
    }
}

impl Render for Mailbox {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.clear_form_password {
            self.form
                .update(cx, |form, cx| form.clear_password(window, cx));
            self.clear_form_password = false;
        }
        if self.adding {
            return div()
                .size_full()
                .flex()
                .flex_col()
                .bg(rgb(BG))
                .text_color(rgb(TEXT))
                .child(
                    div()
                        .flex_1()
                        .flex()
                        .items_center()
                        .justify_center()
                        .p_6()
                        .child(self.form.clone()),
                )
                .child(
                    div()
                        .h(px(38.))
                        .px_4()
                        .flex()
                        .items_center()
                        .justify_between()
                        .border_t_1()
                        .border_color(rgb(BORDER))
                        .child(
                            div()
                                .text_xs()
                                .text_color(rgb(MUTED))
                                .child(self.status.clone()),
                        )
                        .child(if self.busy {
                            Button::new("cancel-login")
                                .label("Cancel")
                                .small()
                                .ghost()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.cancel(cx);
                                    this.status = "Connection cancelled".into();
                                    cx.notify();
                                }))
                                .into_any_element()
                        } else {
                            div()
                                .text_xs()
                                .text_color(rgb(MINT))
                                .child("READ ONLY")
                                .into_any_element()
                        }),
                );
        }

        if self.active.is_none() && self.detail.is_some() {
            return div()
                .size_full()
                .flex()
                .flex_col()
                .bg(rgb(BG))
                .text_color(rgb(TEXT))
                .child(self.reader(false, window, cx));
        }
        let wide = window.viewport_size().width >= px(1280.);

        let mut content = div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(BG))
            .text_color(rgb(TEXT))
            .child(self.toolbar(wide, cx));
        if self.error.is_some()
            || (self.status == "Connection cancelled" && self.folders.is_empty())
        {
            content = content.child(self.notice(cx));
        }
        let panes = h_resizable(("mail-panels", cx.entity_id()))
            .with_state(&self.panels)
            .child(
                resizable_panel()
                    .size(px(144.))
                    .size_range(px(110.)..px(300.))
                    .child(self.folder_rail(cx)),
            )
            .child(
                resizable_panel()
                    .size(px(280.))
                    .size_range(px(180.)..px(650.))
                    .child(self.message_list(false, wide, cx)),
            )
            .child(
                resizable_panel()
                    .size_range(px(260.)..px(3000.))
                    .child(self.reader(false, window, cx)),
            );
        content.child(div().flex_1().min_h_0().min_w_0().child(panes))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{TestAppContext, VisualTestContext};
    use gpui_component::{Root, Theme, ThemeMode};

    #[gpui::test]
    async fn failed_server_keeps_saved_mailbox_and_allows_retry(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        cx.update(|cx| {
            gpui_component::init(cx);
            Theme::change(ThemeMode::Dark, None, cx);
        });
        let runtime = Arc::new(Runtime::new().unwrap());
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = {
            let _context = runtime.enter();
            tokio::net::TcpListener::from_std(listener).unwrap()
        };
        runtime.spawn(async move {
            for _ in 0..2 {
                let (mut socket, _) = server.accept().await.unwrap();
                use tokio::io::AsyncWriteExt;
                socket.write_all(b"not a TLS server\r\n").await.unwrap();
            }
        });
        let store = Arc::new(Store::open_in_memory().unwrap());
        let account = Account::new(
            "person@example.com",
            "app-password",
            Some("127.0.0.1"),
            port,
            false,
        )
        .unwrap();
        let id = store.save_connection(&account, None).unwrap();
        let deadline_runtime = runtime.clone();
        let mut mailbox = None;
        let window = cx.add_window(|window, cx| {
            let view = cx.new(|cx| Mailbox::new(runtime, store.clone(), window, cx));
            mailbox = Some(view.clone());
            Root::new(view, window, cx)
        });
        let mailbox = mailbox.unwrap();
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        for _ in 0..2 {
            visual.update(|_, cx| mailbox.update(cx, |mailbox, cx| mailbox.open_saved(id, cx)));
            // GPUI's condition uses a virtual timer, but Tokio drives this socket in real time.
            let mut notifications = visual.notifications(&mailbox);
            let completion = async {
                use futures::StreamExt as _;
                while visual.read(|cx| mailbox.read(cx).is_busy()) {
                    notifications
                        .next()
                        .await
                        .expect("mailbox dropped during connection");
                }
            };
            let deadline = deadline_runtime.spawn(async {
                tokio::time::sleep(Duration::from_secs(3)).await;
            });
            futures::pin_mut!(completion);
            assert!(
                matches!(
                    futures::future::select(completion, deadline).await,
                    futures::future::Either::Left(_)
                ),
                "connection did not finish within real-time deadline"
            );
            visual.read(|cx| {
                let mailbox = mailbox.read(cx);
                assert!(mailbox.has_error());
                assert_eq!(mailbox.title(cx), "person@example.com");
                assert_eq!(mailbox.account_id(), Some(id));
            });
        }
        assert_eq!(store.accounts().unwrap()[0].id, id);
    }
    // A divider drag must change panel widths without overflowing the available window.
    #[gpui::test]
    fn resizes_mail_panels_within_window_bounds(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
            Theme::change(ThemeMode::Dark, None, cx);
        });
        let runtime = Arc::new(Runtime::new().unwrap());
        let store = Arc::new(Store::open_in_memory().unwrap());
        let mut mailbox = None;
        let window = cx.add_window(|window, cx| {
            let view = cx.new(|cx| {
                let mut mailbox = Mailbox::new(runtime, store, window, cx);
                mailbox.adding = false;
                mailbox.folders = vec!["INBOX".into(), "Sent".into()];
                mailbox
            });
            mailbox = Some(view.clone());
            Root::new(view, window, cx)
        });
        let mailbox = mailbox.unwrap();
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        visual.simulate_resize(gpui::size(px(980.), px(680.)));
        visual.run_until_parked();
        let before = visual.read(|cx| mailbox.read(cx).panels.read(cx).sizes().clone());
        assert_eq!(before.len(), 3);
        let folders = visual
            .debug_bounds("folder-pane")
            .expect("folder pane laid out");
        let divider = gpui::point(
            folders.origin.x + folders.size.width,
            folders.origin.y + px(80.),
        );
        visual.simulate_mouse_down(divider, gpui::MouseButton::Left, gpui::Modifiers::none());
        visual.simulate_mouse_move(
            divider + gpui::point(px(20.), px(0.)),
            gpui::MouseButton::Left,
            gpui::Modifiers::none(),
        );
        visual.simulate_mouse_move(
            divider + gpui::point(px(60.), px(0.)),
            gpui::MouseButton::Left,
            gpui::Modifiers::none(),
        );
        visual.run_until_parked();
        visual.update(|window, _| window.refresh());
        visual.run_until_parked();
        visual.simulate_mouse_move(
            divider + gpui::point(px(60.), px(0.)),
            gpui::MouseButton::Left,
            gpui::Modifiers::none(),
        );
        visual.simulate_mouse_up(
            divider + gpui::point(px(60.), px(0.)),
            gpui::MouseButton::Left,
            gpui::Modifiers::none(),
        );
        visual.run_until_parked();
        let after = visual.read(|cx| mailbox.read(cx).panels.read(cx).sizes().clone());
        assert!(
            after[0] > before[0],
            "divider did not move: {before:?} -> {after:?}"
        );
        assert!(after.iter().map(|value| f32::from(*value)).sum::<f32>() <= 981.);
        visual.simulate_resize(gpui::size(px(800.), px(600.)));
        visual.run_until_parked();
        let folders = visual.debug_bounds("folder-pane").unwrap();
        let messages = visual.debug_bounds("message-pane").unwrap();
        let reader = visual.debug_bounds("reader-pane").unwrap();
        assert!(folders.size.width >= px(110.));
        assert!(messages.size.width >= px(180.));
        assert!(reader.size.width >= px(260.));
        assert!(reader.origin.x + reader.size.width <= px(801.));
    }
}
