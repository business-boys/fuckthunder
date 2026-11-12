//! Persistent GPUI desktop client for read-only IMAP checking and mail viewing.

mod client;

use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use anyhow::{Context as _, Result};
use chrono::{DateTime, Local};
use clap::Parser;
use fuckthunder_gui::{
    check_account, parse_proxy, search_account_mail, Account, ApiServer, CheckStatus,
    ImportSummary, MailHit, ProxyRecord, RunSummary, SessionOptions, Store, StoredAccount,
    WorkspaceSettings,
};
use futures::{stream, StreamExt};
use gpui::{
    bounce, div, ease_in_out, prelude::*, px, relative, rgb, size, uniform_list, Animation,
    AnimationExt as _, Application, Bounds, ClipboardItem, Context, FontWeight, KeyBinding,
    KeyDownEvent, Menu, MenuItem, MouseButton, PathPromptOptions, SharedString, Window,
    WindowBounds, WindowOptions,
};
use gpui_component::{
    button::{Button, ButtonVariants},
    Root, Theme, ThemeMode,
};
use tokio::{runtime::Runtime, sync::mpsc, task::AbortHandle};

gpui::actions!(hakus, [Quit]);

const fn rgba(hex: u32) -> gpui::Rgba {
    gpui::Rgba {
        r: ((hex >> 16) & 0xff) as f32 / 255.0,
        g: ((hex >> 8) & 0xff) as f32 / 255.0,
        b: (hex & 0xff) as f32 / 255.0,
        a: 1.0,
    }
}

const BG: gpui::Rgba = rgba(0x0a0a0a);
const PANEL: gpui::Rgba = rgba(0x141416);
const PANEL_ALT: gpui::Rgba = rgba(0x1c1c20);
const BORDER: gpui::Rgba = rgba(0x29292e);
const TEXT: gpui::Rgba = rgba(0xf2f2f5);
const MUTED: gpui::Rgba = rgba(0x9a9aa2);
const GREEN: gpui::Rgba = rgba(0x62d9a5);
const RED: gpui::Rgba = rgba(0xff5f56);
const AMBER: gpui::Rgba = rgba(0xf2bc68);
const BLUE: gpui::Rgba = rgba(0x9b72ff);
const PURPLE: gpui::Rgba = rgba(0x8135ff);
const PURPLE_HOVER: gpui::Rgba = rgba(0x9153ff);
const PURPLE_SOFT: gpui::Rgba = rgba(0x251a3d);
const ROW_HOVER: gpui::Rgba = rgba(0x202024);
const ACCOUNT_PAGE_SIZE: usize = 25;

#[derive(Debug, Parser)]
#[command(
    name = "fuckthunder-gui",
    about = "Persistent read-only IMAP workspace"
)]
struct Args {
    /// Optional account file to import at startup; the GUI also has an Import button.
    file: Option<PathBuf>,
    /// RFC 5322 message file to open without an IMAP connection.
    #[arg(long)]
    message: Option<PathBuf>,
    /// SQLite database path.
    #[arg(long)]
    database: Option<PathBuf>,
    /// Maximum concurrent account checks.
    #[arg(long, default_value_t = 8)]
    concurrency: usize,
    /// Optional proxy URL/file-compatible row to import at startup.
    #[arg(long)]
    proxy: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Workspace {
    Accounts,
    Mail,
    History,
    Proxies,
    Settings,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum AccountFilter {
    All,
    Valid,
    Attention,
    Proxied,
}

#[derive(Clone, Copy)]
enum ImportKind {
    Accounts,
    Proxies,
}

struct AccountRow {
    id: i64,
    account: Account,
    host: String,
    email_normalized: String,
    status: CheckStatus,
    last_checked_at: Option<i64>,
    proxy_id: Option<i64>,
}

impl From<StoredAccount> for AccountRow {
    fn from(record: StoredAccount) -> Self {
        let host = record.account.imap_host();
        let email_normalized = record.account.email.to_lowercase();
        Self {
            id: record.id,
            account: record.account,
            host,
            email_normalized,
            status: record.status,
            last_checked_at: record.last_checked_at,
            proxy_id: record.proxy_id,
        }
    }
}

struct MailApp {
    runtime: Arc<Runtime>,
    store: Arc<Store>,
    database_path: PathBuf,
    client: gpui::Entity<client::Client>,
    concurrency: usize,
    settings: WorkspaceSettings,
    api_server: Option<ApiServer>,
    reveal_api_token: bool,
    workspace: Workspace,
    account_filter: AccountFilter,
    account_page: usize,
    search: String,
    search_focus: gpui::FocusHandle,
    accounts: Vec<AccountRow>,
    proxies: Vec<ProxyRecord>,
    runs: Vec<RunSummary>,
    selected_account: Option<usize>,
    checking: bool,
    active_run: Option<i64>,
    run_completed: usize,
    run_total: usize,
    batch_abort: Option<AbortHandle>,
    notice: String,
    mail_query: String,
    mail_query_focus: gpui::FocusHandle,
    mail_hits: Vec<(String, MailHit)>,
    mail_scanning: bool,
}

impl MailApp {
    fn new(
        runtime: Arc<Runtime>,
        store: Arc<Store>,
        database_path: PathBuf,
        concurrency: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let settings = store.workspace_settings().unwrap_or(WorkspaceSettings {
            concurrency,
            timeout_seconds: 30,
            api_enabled: false,
            api_port: 8765,
            api_token: String::new(),
        });
        let client = cx.new(|cx| client::Client::new(runtime.clone(), store.clone(), window, cx));
        let concurrency = settings.concurrency.clamp(1, 64);
        let mut app = Self {
            runtime,
            store,
            database_path,
            client,
            concurrency,
            settings,
            api_server: None,
            reveal_api_token: false,
            workspace: Workspace::Mail,
            account_filter: AccountFilter::All,
            account_page: 0,
            search: String::new(),
            search_focus: cx.focus_handle(),
            accounts: Vec::new(),
            proxies: Vec::new(),
            runs: Vec::new(),
            selected_account: None,
            checking: false,
            active_run: None,
            run_completed: 0,
            run_total: 0,
            batch_abort: None,
            notice: "Local workspace ready · import accounts to begin".into(),
            mail_query: String::new(),
            mail_query_focus: cx.focus_handle(),
            mail_hits: Vec::new(),
            mail_scanning: false,
        };
        cx.on_app_quit(|app, cx| {
            if let Some(abort) = app.batch_abort.take() {
                abort.abort();
            }
            app.client
                .update(cx, |client, cx| client.cancel_operations(cx));
            app.api_server.take();
            async {}
        })
        .detach();
        app.reload_data();
        app.notice = format!(
            "Workspace restored · {} accounts · {} proxies",
            app.accounts.len(),
            app.proxies.len()
        );
        if app.settings.api_enabled {
            app.start_api(cx);
        }
        app
    }

    fn save_settings(&mut self, cx: &mut Context<Self>) {
        self.settings.concurrency = self.concurrency;
        match self.store.save_workspace_settings(&self.settings) {
            Ok(()) => cx.notify(),
            Err(error) => {
                self.notice = format!("Settings save failed: {error}");
                cx.notify();
            }
        }
    }

    fn start_api(&mut self, cx: &mut Context<Self>) {
        match ApiServer::start(self.store.clone(), &self.settings, &self.runtime) {
            Ok(server) => {
                self.notice = format!("Local API listening on http://{}", server.status().address);
                self.api_server = Some(server);
            }
            Err(error) => {
                self.api_server = None;
                self.notice = format!("API start failed: {error}");
            }
        }
        cx.notify();
    }

    fn stop_api(&mut self, cx: &mut Context<Self>) {
        self.api_server = None;
        self.notice = "Local API stopped".into();
        cx.notify();
    }

    fn set_api_port(&mut self, port: u16, cx: &mut Context<Self>) {
        self.api_server = None;
        self.settings.api_port = port;
        if self.settings.api_enabled {
            self.start_api(cx);
        }
        self.notice = format!("API port set to {port}");
        self.save_settings(cx);
    }

    fn set_api_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.settings.api_enabled = enabled;
        if enabled {
            self.start_api(cx);
        } else {
            self.stop_api(cx);
        }
        self.save_settings(cx);
    }

    fn clear_data(&mut self, kind: ImportKind, cx: &mut Context<Self>) {
        if self.checking {
            self.notice = "Stop active work before clearing data".into();
            cx.notify();
            return;
        }
        self.client
            .update(cx, |client, cx| client.cancel_operations(cx));
        let result = match kind {
            ImportKind::Accounts => self.store.clear_accounts(),
            ImportKind::Proxies => self.store.clear_proxies(),
        };
        match result {
            Ok(count) => {
                self.selected_account = None;
                self.reload_data();
                self.client
                    .update(cx, |client, cx| client.refresh_accounts(cx));
                self.notice = format!("Removed {count} local records");
            }
            Err(error) => self.notice = format!("Cleanup failed: {error}"),
        }
        cx.notify();
    }

    fn clear_history(&mut self, cx: &mut Context<Self>) {
        if self.checking {
            self.notice = "Stop the active run before clearing history".into();
        } else {
            match self.store.clear_history() {
                Ok(_) => {
                    self.runs.clear();
                    self.notice = "Run history cleared".into();
                }
                Err(error) => self.notice = format!("History cleanup failed: {error}"),
            }
        }
        cx.notify();
    }

    fn reload_data(&mut self) {
        match self.store.accounts() {
            Ok(accounts) => self.accounts = accounts.into_iter().map(AccountRow::from).collect(),
            Err(error) => self.notice = format!("Database read failed: {error}"),
        }
        match self.store.proxies() {
            Ok(proxies) => self.proxies = proxies,
            Err(error) => self.notice = format!("Proxy read failed: {error}"),
        }
        if let Ok(runs) = self.store.runs(100) {
            self.runs = runs;
        }
    }

    fn prompt_import(&mut self, kind: ImportKind, cx: &mut Context<Self>) {
        let (title, noun) = match kind {
            ImportKind::Accounts => ("Import email:password accounts", "accounts"),
            ImportKind::Proxies => ("Import proxy list", "proxies"),
        };
        let picker = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(title.into()),
        });
        self.notice = format!("Choose a UTF-8 {noun} file…");
        cx.notify();
        cx.spawn(async move |this, cx| {
            let paths = match picker.await {
                Ok(Ok(Some(paths))) => paths,
                _ => Vec::new(),
            };
            let Some(path) = paths.first().cloned() else {
                let _ = this.update(cx, |app, cx| {
                    app.notice = "Import cancelled".into();
                    cx.notify();
                });
                return;
            };
            let _ = this.update(cx, |app, cx| match read_text_limited(&path) {
                Ok(input) => app.import_text(kind, input, cx),
                Err(error) => {
                    app.notice = format!("Import failed: {error}");
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn import_text(&mut self, kind: ImportKind, input: String, cx: &mut Context<Self>) {
        let noun = match kind {
            ImportKind::Accounts => "accounts",
            ImportKind::Proxies => "proxies",
        };
        if input.trim().is_empty() {
            self.notice = format!("Clipboard has no {noun} to import");
            cx.notify();
            return;
        }
        if input.len() > 16 * 1024 * 1024 {
            self.notice = "Import exceeds 16 MiB".into();
            cx.notify();
            return;
        }
        self.notice = format!("Importing {noun}…");
        cx.notify();
        let store = self.store.clone();
        let runtime = self.runtime.clone();
        cx.spawn(async move |this, cx| {
            let result = runtime
                .spawn_blocking(move || -> Result<(ImportKind, ImportSummary)> {
                    let summary = match kind {
                        ImportKind::Accounts => store.import_accounts(&input),
                        ImportKind::Proxies => store.import_proxies(&input),
                    }?;
                    Ok((kind, summary))
                })
                .await;
            let _ = this.update(cx, |app, cx| {
                match result {
                    Ok(Ok((kind, summary))) => {
                        app.reload_data();
                        let noun = match kind {
                            ImportKind::Accounts => "accounts",
                            ImportKind::Proxies => "proxies",
                        };
                        app.notice = format!(
                            "Imported {} {noun} · {} duplicates · {} invalid rows",
                            summary.imported, summary.duplicates, summary.invalid
                        );
                    }
                    Ok(Err(error)) => app.notice = format!("Import failed: {error}"),
                    Err(error) => app.notice = format!("Import worker failed: {error}"),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn paste_from_clipboard(&mut self, cx: &mut Context<Self>) {
        let kind = match self.workspace {
            Workspace::Accounts | Workspace::History => ImportKind::Accounts,
            Workspace::Proxies => ImportKind::Proxies,
            Workspace::Mail | Workspace::Settings => return,
        };
        match cx.read_from_clipboard().and_then(|item| item.text()) {
            Some(input) => self.import_text(kind, input, cx),
            None => {
                self.notice = "Clipboard does not contain text".into();
                cx.notify();
            }
        }
    }

    fn start_checks(&mut self, retry_failed: bool, cx: &mut Context<Self>) {
        if self.checking {
            return;
        }
        let selected: Vec<_> = self
            .accounts
            .iter()
            .enumerate()
            .filter(|(_, row)| {
                !retry_failed
                    || matches!(
                        row.status,
                        CheckStatus::Invalid { .. } | CheckStatus::Error { .. }
                    )
            })
            .map(|(index, row)| (index, row.id))
            .collect();
        if selected.is_empty() {
            self.notice = if retry_failed {
                "No failed accounts to retry".into()
            } else {
                "Import accounts first".into()
            };
            cx.notify();
            return;
        }
        let run_id = match self.store.begin_run() {
            Ok(run_id) => run_id,
            Err(error) => {
                self.notice = format!("Cannot start run: {error}");
                cx.notify();
                return;
            }
        };
        self.checking = true;
        self.active_run = Some(run_id);
        self.run_completed = 0;
        self.run_total = selected.len();
        self.notice = format!("Run #{run_id} started · {} accounts", selected.len());
        let jobs: Vec<_> = selected
            .into_iter()
            .enumerate()
            .map(|(position, (index, id))| {
                let row = &mut self.accounts[index];
                row.status = CheckStatus::Checking;
                let proxy = if self.proxies.is_empty() {
                    None
                } else {
                    self.proxies.get(position % self.proxies.len())
                };
                let options = SessionOptions {
                    proxy: proxy.and_then(|record| parse_proxy(&record.url).ok()),
                    timeout: Duration::from_secs(self.settings.timeout_seconds),
                };
                (
                    index,
                    id,
                    row.account.clone(),
                    proxy.map(|record| record.id),
                    options,
                )
            })
            .collect();
        cx.notify();

        let (sender, mut receiver) = mpsc::unbounded_channel();
        let concurrency = self.concurrency;
        let task = self.runtime.spawn(async move {
            stream::iter(
                jobs.into_iter()
                    .map(move |(index, id, account, proxy_id, options)| {
                        let sender = sender.clone();
                        async move {
                            let status = check_account(&account, &options).await;
                            let _ = sender.send((index, id, proxy_id, status));
                        }
                    }),
            )
            .buffer_unordered(concurrency)
            .for_each(|_| futures::future::ready(()))
            .await;
        });
        self.batch_abort = Some(task.abort_handle());

        let store = self.store.clone();
        cx.spawn(async move |this, cx| {
            let mut completed = 0usize;
            let mut storage_failed = false;
            while let Some((index, account_id, proxy_id, status)) = receiver.recv().await {
                let _ = this.update(cx, |app, cx| {
                    if let Some(row) = app.accounts.get_mut(index) {
                        row.status = status.clone();
                        row.last_checked_at = Some(chrono::Utc::now().timestamp());
                        row.proxy_id = proxy_id;
                    }
                    if let Err(error) = app.store.record_check(
                        app.active_run.unwrap_or_default(),
                        account_id,
                        proxy_id,
                        &status,
                    ) {
                        storage_failed = true;
                        app.notice = format!("Cannot persist result: {error}");
                    }
                    completed += 1;
                    app.run_completed = completed;
                    if !storage_failed {
                        app.notice = format!("Run #{} · {completed} checked", run_id);
                    }
                    cx.notify();
                });
            }
            let _ = store.finish_run(run_id);
            let _ = this.update(cx, |app, cx| {
                app.checking = false;
                app.active_run = None;
                app.batch_abort = None;
                let valid = app.count_status(|status| matches!(status, CheckStatus::Valid { .. }));
                let invalid =
                    app.count_status(|status| matches!(status, CheckStatus::Invalid { .. }));
                let errors =
                    app.count_status(|status| matches!(status, CheckStatus::Error { .. }));
                app.notice = format!(
                    "Run #{run_id} finished · {completed} checked · {valid} valid · {invalid} invalid · {errors} errors"
                );
                if let Ok(runs) = app.store.runs(100) {
                    app.runs = runs;
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn stop_checks(&mut self, cx: &mut Context<Self>) {
        if let Some(abort) = self.batch_abort.take() {
            abort.abort();
            self.notice = "Stopping batch…".into();
            cx.notify();
        }
    }

    fn search_mail(&mut self, cx: &mut Context<Self>) {
        let query = self.mail_query.trim().to_owned();
        if query.is_empty() {
            self.notice = "Enter a word or phrase to search mail".into();
            cx.notify();
            return;
        }
        if self.checking || self.mail_scanning {
            self.notice = "Wait for the current run to finish".into();
            cx.notify();
            return;
        }
        let targets: Vec<_> = self
            .accounts
            .iter()
            .map(|row| (row.account.clone(), row.proxy_id))
            .collect();
        if targets.is_empty() {
            self.notice = "Import accounts before searching mail".into();
            cx.notify();
            return;
        }
        self.mail_scanning = true;
        self.mail_hits.clear();
        self.notice = format!(
            "Searching mail for \"{query}\" across {} accounts",
            targets.len()
        );
        let timeout = Duration::from_secs(self.settings.timeout_seconds);
        let proxies = self.proxies.clone();
        let jobs: Vec<_> = targets
            .into_iter()
            .enumerate()
            .map(|(position, (account, proxy_id))| {
                let proxy = proxy_id
                    .and_then(|id| proxies.iter().find(|record| record.id == id))
                    .or_else(|| proxies.get(position % proxies.len().max(1)));
                let options = SessionOptions {
                    proxy: proxy.and_then(|record| parse_proxy(&record.url).ok()),
                    timeout,
                };
                (account, options)
            })
            .collect();
        let concurrency = self.concurrency;
        let (sender, mut receiver) = mpsc::unbounded_channel();
        let task = self.runtime.spawn(async move {
            stream::iter(jobs.into_iter().map(move |(account, options)| {
                let sender = sender.clone();
                let query = query.clone();
                async move {
                    let email = account.email.clone();
                    if let Ok(hits) = search_account_mail(&account, &options, &query, 5).await {
                        for hit in hits {
                            let _ = sender.send((email.clone(), hit));
                        }
                    }
                }
            }))
            .buffer_unordered(concurrency.max(1))
            .for_each(|_| futures::future::ready(()))
            .await;
        });
        self.batch_abort = Some(task.abort_handle());
        cx.spawn(async move |this, cx| {
            while let Some((email, hit)) = receiver.recv().await {
                let _ = this.update(cx, |app, cx| {
                    app.mail_hits.push((email, hit));
                    cx.notify();
                });
            }
            let _ = this.update(cx, |app, cx| {
                app.mail_scanning = false;
                app.batch_abort = None;
                app.notice = format!("{} messages matched", app.mail_hits.len());
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn count_status(&self, predicate: impl Fn(&CheckStatus) -> bool) -> usize {
        self.accounts
            .iter()
            .filter(|row| predicate(&row.status))
            .count()
    }

    fn visible_accounts(&self) -> Vec<usize> {
        let query = self.search.trim().to_lowercase();
        self.accounts
            .iter()
            .enumerate()
            .filter(|(_, row)| {
                let matches_filter = match self.account_filter {
                    AccountFilter::All => true,
                    AccountFilter::Valid => matches!(row.status, CheckStatus::Valid { .. }),
                    AccountFilter::Attention => matches!(
                        row.status,
                        CheckStatus::Invalid { .. } | CheckStatus::Error { .. }
                    ),
                    AccountFilter::Proxied => row.proxy_id.is_some(),
                };
                let matches_search = query.is_empty()
                    || row.email_normalized.contains(&query)
                    || row.host.contains(&query);
                matches_filter && matches_search
            })
            .map(|(index, _)| index)
            .collect()
    }

    fn open_account(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let id = self.accounts[index].id;
        self.workspace = Workspace::Mail;
        self.client
            .update(cx, |client, cx| client.open_saved(id, window, cx));
    }

    fn reset_results(&mut self, cx: &mut Context<Self>) {
        if self.checking {
            return;
        }
        match self.store.reset_results() {
            Ok(()) => {
                for row in &mut self.accounts {
                    row.status = CheckStatus::Idle;
                }
                self.notice = "Current verdicts reset · history preserved".into();
            }
            Err(error) => self.notice = format!("Reset failed: {error}"),
        }
        cx.notify();
    }

    fn delete_selected_account(&mut self, cx: &mut Context<Self>) {
        if self.checking {
            return;
        }
        let Some(index) = self.selected_account else {
            self.notice = "Select an account to delete".into();
            cx.notify();
            return;
        };
        self.client
            .update(cx, |client, cx| client.cancel_operations(cx));
        match self.store.delete_account(self.accounts[index].id) {
            Ok(()) => {
                self.accounts.remove(index);
                self.selected_account = None;
                self.notice = "Account deleted from local database".into();
                self.client
                    .update(cx, |client, cx| client.refresh_accounts(cx));
            }
            Err(error) => self.notice = format!("Delete failed: {error}"),
        }
        cx.notify();
    }

    fn render_sidebar(&self, cx: &mut Context<Self>) -> gpui::Div {
        let valid = self.count_status(|status| matches!(status, CheckStatus::Valid { .. }));
        let attention = self.count_status(|status| {
            matches!(
                status,
                CheckStatus::Invalid { .. } | CheckStatus::Error { .. }
            )
        });
        div()
            .w(px(168.))
            .m_2()
            .mr_0()
            .mb(px(44.))
            .flex()
            .flex_col()
            .p_2()
            .gap_1()
            .rounded_xl()
            .bg(PANEL)
            .child(
                div()
                    .h(px(48.))
                    .px_2()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .w(px(38.))
                            .h(px(38.))
                            .rounded_lg()
                            .bg(PURPLE)
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_color(TEXT)
                            .text_lg()
                            .child("↯")
                            .with_animation(
                                "brand-pulse",
                                Animation::new(Duration::from_secs(3))
                                    .repeat()
                                    .with_easing(bounce(ease_in_out)),
                                |element, delta| element.opacity(0.72 + delta * 0.28),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .child(div().text_lg().font_weight(FontWeight::BOLD).child("Hakus"))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(MUTED)
                                    .child("Personal workspace"),
                            ),
                    ),
            )
            .child(
                div()
                    .px_3()
                    .pb_2()
                    .text_xs()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(MUTED)
                    .child("Workspace"),
            )
            .child(sidebar_button(
                "▦",
                "Accounts",
                self.workspace == Workspace::Accounts,
                Workspace::Accounts,
                cx,
            ))
            .child(sidebar_button(
                "▤",
                "Mail client",
                self.workspace == Workspace::Mail,
                Workspace::Mail,
                cx,
            ))
            .child(sidebar_button(
                "↻",
                "Run history",
                self.workspace == Workspace::History,
                Workspace::History,
                cx,
            ))
            .child(sidebar_button(
                "⌁",
                "Proxy routes",
                self.workspace == Workspace::Proxies,
                Workspace::Proxies,
                cx,
            ))
            .child(sidebar_button(
                "⚙",
                "Settings & API",
                self.workspace == Workspace::Settings,
                Workspace::Settings,
                cx,
            ))
            .child(div().flex_1())
            .child(
                div()
                    .mt_3()
                    .p_4()
                    .rounded_lg()
                    .bg(PANEL_ALT)
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(MUTED)
                            .child("Overview"),
                    )
                    .child(
                        div()
                            .pt_2()
                            .flex()
                            .flex_row()
                            .justify_between()
                            .child(sidebar_metric("Total", self.accounts.len(), TEXT))
                            .child(sidebar_metric("Valid", valid, GREEN))
                            .child(sidebar_metric("Attention", attention, AMBER)),
                    ),
            )
            .child(div().pt_3().text_xs().text_color(MUTED).child(format!(
                "{} proxies · {} threads",
                self.proxies.len(),
                self.concurrency
            )))
            .child(
                div().text_xs().text_color(rgb(0x53677b)).child(
                    self.database_path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned(),
                ),
            )
    }

    fn render_accounts(&mut self, window: &mut Window, cx: &mut Context<Self>) -> gpui::Div {
        let search_focused = self.search_focus.is_focused(window);
        let visible = self.visible_accounts();
        let pages = visible.len().max(1).div_ceil(ACCOUNT_PAGE_SIZE);
        self.account_page = self.account_page.min(pages - 1);
        let start = self.account_page * ACCOUNT_PAGE_SIZE;
        let page_indices: Vec<_> = visible
            .iter()
            .copied()
            .skip(start)
            .take(ACCOUNT_PAGE_SIZE)
            .collect();
        let page_len = page_indices.len();
        let app = cx.entity().clone();
        let list = uniform_list("account-table", page_len, move |range, _, cx| {
            range
                .map(|position| {
                    let index = page_indices[position];
                    let this = app.read(cx);
                    let row = &this.accounts[index];
                    let selected = this.selected_account == Some(index);
                    let (status, color) = status_label(&row.status);
                    let checked = row
                        .last_checked_at
                        .map_or_else(|| "Never".into(), format_short_timestamp);
                    let proxy = row
                        .proxy_id
                        .map_or_else(|| "Direct".into(), |id| format!("Proxy #{id}"));
                    let app = app.clone();
                    let open_app = app.clone();
                    div()
                        .h(px(64.))
                        .w_full()
                        .flex()
                        .flex_row()
                        .items_center()
                        .mx_2()
                        .my_1()
                        .px_4()
                        .gap_4()
                        .rounded_lg()
                        .bg(if selected { PURPLE_SOFT } else { PANEL })
                        .hover(|style| style.bg(if selected { PURPLE_SOFT } else { ROW_HOVER }))
                        .cursor_pointer()
                        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                            app.update(cx, |app, cx| {
                                app.selected_account = Some(index);
                                cx.notify();
                            });
                        })
                        .child(table_cell(format!("#{:04}", row.id), 64., MUTED))
                        .child(
                            div()
                                .w(px(330.))
                                .flex()
                                .flex_col()
                                .child(
                                    div()
                                        .text_sm()
                                        .text_color(TEXT)
                                        .child(row.account.email.clone()),
                                )
                                .child(div().text_xs().text_color(MUTED).child(row.host.clone())),
                        )
                        .child(div().w(px(130.)).child(status_pill(status, color)))
                        .child(table_cell(proxy, 110., MUTED))
                        .child(table_cell(checked, 150., MUTED))
                        .child(div().flex_1())
                        .child(icon_button("→").on_mouse_down(
                            MouseButton::Left,
                            move |_, window, cx| {
                                cx.stop_propagation();
                                open_app.update(cx, |app, cx| app.open_account(index, window, cx));
                            },
                        ))
                })
                .collect()
        })
        .h_full();

        let search = self.search.clone();
        let search_display = if search.is_empty() {
            "Search email or IMAP host…".into()
        } else {
            search
        };
        let mail_query_focused = self.mail_query_focus.is_focused(window);
        let mail_query_display = if self.mail_query.is_empty() {
            "Search mail content…".to_owned()
        } else {
            self.mail_query.clone()
        };
        let progress = if self.run_total == 0 {
            0.
        } else {
            self.run_completed as f32 / self.run_total as f32
        };
        div()
            .flex_1()
            .min_h_0()
            .w_full()
            .mt_3()
            .mr_3()
            .mb_2()
            .flex()
            .flex_col()
            .rounded_xl()
            .overflow_hidden()
            .bg(PANEL)
            .child(
                div()
                    .h(px(74.))
                    .w_full()
                    .px_6()
                    .py_3()
                    .flex()
                    .flex_row()
                    .justify_between()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(
                                div()
                                    .text_lg()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("Accounts"),
                            )
                            .child(div().text_sm().text_color(MUTED).child(format!(
                                "{} stored locally · {} matching the current view",
                                self.accounts.len(),
                                visible.len()
                            ))),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_2()
                            .child(action_button("Paste", false).on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|app, _, _, cx| app.paste_from_clipboard(cx)),
                            ))
                            .child(action_button("Import", false).on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|app, _, _, cx| {
                                    app.prompt_import(ImportKind::Accounts, cx)
                                }),
                            ))
                            .child(if self.checking {
                                action_button("Stop run", true).on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(|app, _, _, cx| app.stop_checks(cx)),
                                )
                            } else {
                                action_button("Run check  →", true).on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(|app, _, _, cx| app.start_checks(false, cx)),
                                )
                            }),
                    ),
            )
            .child(
                div()
                    .h(px(66.))
                    .w_full()
                    .px_6()
                    .pb_3()
                    .flex()
                    .flex_row()
                    .gap_3()
                    .child(overview_card(
                        "Total accounts",
                        self.accounts.len().to_string(),
                        "Stored in this workspace",
                        BLUE,
                    ))
                    .child(overview_card(
                        "Valid",
                        self.count_status(|status| matches!(status, CheckStatus::Valid { .. }))
                            .to_string(),
                        "Ready to open",
                        GREEN,
                    ))
                    .child(overview_card(
                        "Needs attention",
                        self.count_status(|status| {
                            matches!(
                                status,
                                CheckStatus::Invalid { .. } | CheckStatus::Error { .. }
                            )
                        })
                        .to_string(),
                        "Failed or transport errors",
                        AMBER,
                    ))
                    .child(overview_card(
                        "Proxy routes",
                        self.proxies.len().to_string(),
                        "Round-robin pool",
                        TEXT,
                    )),
            )
            .child(
                div()
                    .min_h(px(88.))
                    .flex_shrink_0()
                    .w_full()
                    .px_6()
                    .flex()
                    .flex_row()
                    .flex_wrap()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .id("account-search")
                            .track_focus(&self.search_focus)
                            .on_key_down(cx.listener(|app, event: &KeyDownEvent, _, cx| {
                                let key = event.keystroke.key.as_str();
                                if key == "v" && event.keystroke.modifiers.secondary() {
                                    if let Some(text) =
                                        cx.read_from_clipboard().and_then(|item| item.text())
                                    {
                                        app.search = text.trim().replace(['\r', '\n'], " ");
                                    }
                                } else if key == "backspace" {
                                    app.search.pop();
                                } else if key == "escape" {
                                    app.search.clear();
                                } else if !event.keystroke.modifiers.control
                                    && !event.keystroke.modifiers.platform
                                {
                                    if let Some(character) = &event.keystroke.key_char {
                                        app.search.push_str(character);
                                    }
                                }
                                app.account_page = 0;
                                cx.stop_propagation();
                                cx.notify();
                            }))
                            .on_mouse_down(MouseButton::Left, {
                                let focus = self.search_focus.clone();
                                move |_, window, _| window.focus(&focus)
                            })
                            .w(px(230.))
                            .h(px(32.))
                            .px_4()
                            .flex()
                            .flex_row()
                            .flex_wrap()
                            .items_center()
                            .gap_2()
                            .rounded_lg()
                            .bg(PANEL_ALT)
                            .border_1()
                            .border_color(if search_focused { PURPLE } else { PANEL_ALT })
                            .text_sm()
                            .text_color(if search_display.starts_with("Search email") {
                                MUTED
                            } else {
                                TEXT
                            })
                            .child(div().text_color(MUTED).child("⌕"))
                            .child(search_display),
                    )
                    .child(
                        div()
                            .h(px(32.))
                            .px_1()
                            .flex()
                            .flex_row()
                            .flex_wrap()
                            .items_center()
                            .gap_1()
                            .rounded_full()
                            .bg(PANEL_ALT)
                            .child(filter_button(
                                "All",
                                AccountFilter::All,
                                self.account_filter,
                                cx,
                            ))
                            .child(filter_button(
                                "Valid",
                                AccountFilter::Valid,
                                self.account_filter,
                                cx,
                            ))
                            .child(filter_button(
                                "Attention",
                                AccountFilter::Attention,
                                self.account_filter,
                                cx,
                            ))
                            .child(filter_button(
                                "Proxied",
                                AccountFilter::Proxied,
                                self.account_filter,
                                cx,
                            )),
                    )
                    .child(
                        div()
                            .id("mail-content-search")
                            .track_focus(&self.mail_query_focus)
                            .on_key_down(cx.listener(|app, event: &KeyDownEvent, _, cx| {
                                let key = event.keystroke.key.as_str();
                                if key == "enter" {
                                    app.search_mail(cx);
                                } else if key == "v" && event.keystroke.modifiers.secondary() {
                                    if let Some(text) =
                                        cx.read_from_clipboard().and_then(|item| item.text())
                                    {
                                        app.mail_query = text.trim().replace(['\r', '\n'], " ");
                                    }
                                } else if key == "backspace" {
                                    app.mail_query.pop();
                                } else if key == "escape" {
                                    app.mail_query.clear();
                                } else if !event.keystroke.modifiers.control
                                    && !event.keystroke.modifiers.platform
                                {
                                    if let Some(character) = &event.keystroke.key_char {
                                        app.mail_query.push_str(character);
                                    }
                                }
                                cx.stop_propagation();
                                cx.notify();
                            }))
                            .on_mouse_down(MouseButton::Left, {
                                let focus = self.mail_query_focus.clone();
                                move |_, window, _| window.focus(&focus)
                            })
                            .w(px(220.))
                            .h(px(32.))
                            .px_4()
                            .flex()
                            .flex_row()
                            .flex_wrap()
                            .items_center()
                            .gap_2()
                            .rounded_lg()
                            .bg(PANEL_ALT)
                            .border_1()
                            .border_color(if mail_query_focused {
                                PURPLE
                            } else {
                                PANEL_ALT
                            })
                            .text_sm()
                            .text_color(if mail_query_display.starts_with("Search mail") {
                                MUTED
                            } else {
                                TEXT
                            })
                            .child(div().text_color(MUTED).child("✉"))
                            .child(mail_query_display),
                    )
                    .child(
                        action_button(
                            if self.mail_scanning {
                                "Searching…"
                            } else {
                                "Search mail"
                            },
                            false,
                        )
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|app, _, _, cx| app.search_mail(cx)),
                        ),
                    ),
            )
            .child(
                div()
                    .h(px(48.))
                    .mx_6()
                    .mt_1()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .border_b_1()
                    .border_color(BORDER)
                    .child(
                        div()
                            .w(px(100.))
                            .text_xs()
                            .text_color(MUTED)
                            .child("Concurrency"),
                    )
                    .children([4usize, 8, 16, 32, 64].map(|threads| {
                        compact_button(threads.to_string(), self.concurrency == threads)
                            .on_mouse_down(MouseButton::Left, {
                                let app = cx.entity().clone();
                                move |_, _, cx| {
                                    app.update(cx, |app, cx| {
                                        if !app.checking {
                                            app.concurrency = threads;
                                            app.save_settings(cx);
                                        }
                                    });
                                }
                            })
                    }))
                    .child(div().flex_1())
                    .child(
                        div()
                            .w(px(210.))
                            .h(px(5.))
                            .rounded_full()
                            .bg(PANEL_ALT)
                            .child(
                                div()
                                    .h_full()
                                    .w(relative(progress))
                                    .rounded_full()
                                    .bg(if self.checking { GREEN } else { PURPLE }),
                            ),
                    )
                    .child(
                        div()
                            .w(px(92.))
                            .text_xs()
                            .text_color(if self.checking { GREEN } else { MUTED })
                            .child(if self.run_total == 0 {
                                "Ready".into()
                            } else {
                                format!(
                                    "{}/{} · {:.0}%",
                                    self.run_completed,
                                    self.run_total,
                                    progress * 100.
                                )
                            }),
                    ),
            )
            .child(
                div()
                    .h(px(44.))
                    .w_full()
                    .mt_3()
                    .px_6()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_4()
                    .text_xs()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(MUTED)
                    .child(div().w(px(64.)).child("ID"))
                    .child(div().w(px(330.)).child("Account"))
                    .child(div().w(px(130.)).child("Status"))
                    .child(div().w(px(110.)).child("Route"))
                    .child(div().w(px(150.)).child("Last checked")),
            )
            .child(if page_len == 0 {
                empty_state(
                    "NOTHING TO SHOW",
                    "Import accounts or clear the active search and filters",
                )
                .into_any_element()
            } else {
                list.into_any_element()
            })
            .children((!self.mail_hits.is_empty() || self.mail_scanning).then(|| {
                let hits: Vec<_> = self
                    .mail_hits
                    .iter()
                    .take(40)
                    .map(|(email, hit)| {
                        div()
                            .px_6()
                            .py_2()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .border_t_1()
                            .border_color(BORDER)
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(hit.subject.clone()),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(MUTED)
                                    .child(format!("{email} · {} · {}", hit.folder, hit.from)),
                            )
                    })
                    .collect();
                div()
                    .id("mail-search-results")
                    .max_h(px(220.))
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .child(div().px_6().py_2().text_xs().text_color(MUTED).child(
                        if self.mail_scanning {
                            "Searching mail…".to_owned()
                        } else {
                            format!("{} matching messages", self.mail_hits.len())
                        },
                    ))
                    .children(hits)
            }))
            .child(
                div()
                    .h(px(66.))
                    .w_full()
                    .px_6()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(action_button("← Previous", false).on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|app, _, _, cx| {
                            app.account_page = app.account_page.saturating_sub(1);
                            cx.notify();
                        }),
                    ))
                    .child(
                        div()
                            .text_sm()
                            .text_color(MUTED)
                            .child(format!("{} of {pages}", self.account_page + 1)),
                    )
                    .child(action_button("Next →", false).on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |app, _, _, cx| {
                            app.account_page = (app.account_page + 1).min(pages - 1);
                            cx.notify();
                        }),
                    ))
                    .child(div().flex_1())
                    .child(action_button("Retry failed", false).on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|app, _, _, cx| app.start_checks(true, cx)),
                    ))
                    .child(action_button("Reset", false).on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|app, _, _, cx| app.reset_results(cx)),
                    ))
                    .child(danger_button("Delete selected").on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|app, _, _, cx| app.delete_selected_account(cx)),
                    )),
            )
    }

    fn render_history(&mut self, _cx: &mut Context<Self>) -> gpui::Div {
        let runs = self.runs.clone();
        let run_count = runs.len();
        let list = uniform_list("run-history", run_count, move |range, _, _| {
            range
                .map(|index| {
                    let run = &runs[index];
                    let state = if run.finished_at.is_some() {
                        "FINISHED"
                    } else {
                        "INTERRUPTED"
                    };
                    div()
                        .h(px(104.))
                        .w_full()
                        .px_6()
                        .py_4()
                        .border_b_1()
                        .border_color(BORDER)
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .justify_between()
                                .child(
                                    div()
                                        .text_sm()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(TEXT)
                                        .child(format!("RUN #{} · {state}", run.id)),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(MUTED)
                                        .child(format_timestamp(run.started_at)),
                                ),
                        )
                        .child(
                            div()
                                .pt_3()
                                .flex()
                                .flex_row()
                                .gap_5()
                                .child(history_metric("TOTAL", run.total, BLUE))
                                .child(history_metric("VALID", run.valid, GREEN))
                                .child(history_metric("INVALID", run.invalid, RED))
                                .child(history_metric("ERRORS", run.errors, AMBER))
                                .child(div().text_xs().text_color(MUTED).child(
                                    run.finished_at.map_or_else(
                                        || "No finish timestamp".into(),
                                        |finished| {
                                            format!(
                                                "Duration {}s",
                                                (finished - run.started_at).max(0)
                                            )
                                        },
                                    ),
                                )),
                        )
                })
                .collect()
        })
        .h_full();
        div()
            .flex_1()
            .min_h_0()
            .w_full()
            .m_4()
            .mb_2()
            .flex()
            .flex_col()
            .rounded_xl()
            .overflow_hidden()
            .border_1()
            .border_color(BORDER)
            .bg(PANEL)
            .child(panel_header("CHECK HISTORY · LAST 100 RUNS"))
            .child(if run_count == 0 {
                empty_state(
                    "NO RUNS YET",
                    "Start a batch to create persistent check history",
                )
                .into_any_element()
            } else {
                list.into_any_element()
            })
    }

    fn render_proxies(&mut self, cx: &mut Context<Self>) -> gpui::Div {
        let app = cx.entity().clone();
        let count = self.proxies.len();
        let list = uniform_list("proxies", count, {
            let app = app.clone();
            move |range, _, cx| {
                range
                    .map(|index| {
                        let this = app.read(cx);
                        let proxy = &this.proxies[index];
                        let id = proxy.id;
                        let used = proxy
                            .last_used_at
                            .map_or_else(|| "NEVER USED".into(), format_timestamp);
                        let app = app.clone();
                        div()
                            .h(px(88.))
                            .w_full()
                            .px_6()
                            .py_4()
                            .border_b_1()
                            .border_color(BORDER)
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .justify_between()
                                    .child(
                                        div()
                                            .text_sm()
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(TEXT)
                                            .child(format!("PROXY #{id} · {}", proxy.endpoint())),
                                    )
                                    .child(action_button("DELETE", false).on_mouse_down(
                                        MouseButton::Left,
                                        move |_, _, cx| {
                                            app.update(cx, |app, cx| {
                                                if app.checking {
                                                    app.notice =
                                                        "Stop the batch before deleting proxies"
                                                            .into();
                                                } else {
                                                    match app.store.delete_proxy(id) {
                                                        Ok(()) => {
                                                            app.reload_data();
                                                            app.notice =
                                                                format!("Proxy #{id} deleted");
                                                        }
                                                        Err(error) => {
                                                            app.notice = format!(
                                                                "Proxy delete failed: {error}"
                                                            );
                                                        }
                                                    }
                                                }
                                                cx.notify();
                                            });
                                        },
                                    )),
                            )
                            .child(
                                div()
                                    .pt_2()
                                    .text_xs()
                                    .text_color(MUTED)
                                    .child(format!("ROUND-ROBIN ASSIGNMENT · LAST USED {used}")),
                            )
                    })
                    .collect()
            }
        })
        .h_full();
        div()
            .flex_1()
            .min_h_0()
            .w_full()
            .m_4()
            .mb_2()
            .flex()
            .flex_col()
            .rounded_xl()
            .overflow_hidden()
            .border_1()
            .border_color(BORDER)
            .bg(PANEL)
            .child(
                div()
                    .h(px(56.))
                    .w_full()
                    .px_4()
                    .py_3()
                    .flex()
                    .flex_row()
                    .justify_between()
                    .border_b_1()
                    .border_color(BORDER)
                    .child(
                        div()
                            .text_xs()
                            .text_color(MUTED)
                            .child("PROXY POOL · CREDENTIALS NEVER DISPLAYED"),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .gap_2()
                            .child(action_button("Paste", false).on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|app, _, _, cx| app.paste_from_clipboard(cx)),
                            ))
                            .child(action_button("Import proxies", true).on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|app, _, _, cx| {
                                    app.prompt_import(ImportKind::Proxies, cx)
                                }),
                            )),
                    ),
            )
            .child(if count == 0 {
                empty_state(
                    "NO PROXIES",
                    "Import URL rows or host:port:user:password · direct connections are used without a pool",
                )
                .into_any_element()
            } else {
                list.into_any_element()
            })
    }

    fn render_settings(&mut self, cx: &mut Context<Self>) -> gpui::Stateful<gpui::Div> {
        let running = self
            .api_server
            .as_ref()
            .is_some_and(|server| server.status().running);
        let address = format!("http://127.0.0.1:{}", self.settings.api_port);
        let token = if self.reveal_api_token {
            self.settings.api_token.clone()
        } else {
            format!(
                "{}••••••••{}",
                &self.settings.api_token[..self.settings.api_token.len().min(8)],
                &self.settings.api_token[self.settings.api_token.len().saturating_sub(4)..]
            )
        };
        let endpoint = address.clone();
        div()
            .id("settings-scroll")
            .flex_1()
            .min_h_0()
            .w_full()
            .mt_3()
            .mr_3()
            .mb_2()
            .p_5()
            .flex()
            .flex_col()
            .gap_4()
            .rounded_xl()
            .overflow_y_scroll()
            .bg(PANEL)
            .child(
                div()
                    .flex()
                    .flex_row()
                    .justify_between()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(
                                div()
                                    .text_2xl()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("Settings & local API"),
                            )
                            .child(div().text_sm().text_color(MUTED).child(
                                "Persistent workspace preferences · loopback-only HTTP service",
                            )),
                    )
                    .child(status_pill(
                        if running {
                            "● API running".into()
                        } else {
                            "○ API stopped".into()
                        },
                        if running { GREEN } else { MUTED },
                    )),
            )
            .child(
                div()
                    .w_full()
                    .p_5()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .rounded_xl()
                    .bg(PANEL_ALT)
                    .child(
                        div()
                            .text_lg()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Checking"),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_3()
                            .child(
                                div()
                                    .w(px(150.))
                                    .text_sm()
                                    .text_color(MUTED)
                                    .child("Concurrency"),
                            )
                            .children([4usize, 8, 16, 32, 64].map(|threads| {
                                compact_button(threads.to_string(), self.concurrency == threads)
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(move |app, _, _, cx| {
                                            if !app.checking {
                                                app.concurrency = threads;
                                                app.save_settings(cx);
                                            }
                                        }),
                                    )
                            })),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_3()
                            .child(
                                div()
                                    .w(px(150.))
                                    .text_sm()
                                    .text_color(MUTED)
                                    .child("IMAP timeout"),
                            )
                            .children([10u64, 30, 60, 120].map(|seconds| {
                                compact_button(
                                    format!("{seconds}s"),
                                    self.settings.timeout_seconds == seconds,
                                )
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(move |app, _, _, cx| {
                                        app.settings.timeout_seconds = seconds;
                                        app.save_settings(cx);
                                    }),
                                )
                            })),
                    ),
            )
            .child(
                div()
                    .w_full()
                    .p_5()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .rounded_xl()
                    .bg(PANEL_ALT)
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .justify_between()
                            .child(
                                div()
                                    .text_lg()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("Local API"),
                            )
                            .child(
                                action_button(if running { "Stop API" } else { "Start API" }, true)
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(move |app, _, _, cx| {
                                            app.set_api_enabled(!running, cx)
                                        }),
                                    ),
                            ),
                    )
                    .child(div().text_sm().text_color(MUTED).child(
                        "Bearer protected · bound to 127.0.0.1 · responses never expose passwords",
                    ))
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_3()
                            .child(div().w(px(150.)).text_sm().text_color(MUTED).child("Port"))
                            .children([8765u16, 8080, 3000].map(|port| {
                                compact_button(port.to_string(), self.settings.api_port == port)
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(move |app, _, _, cx| {
                                            app.set_api_port(port, cx)
                                        }),
                                    )
                            })),
                    )
                    .child(
                        div()
                            .h(px(48.))
                            .px_4()
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .rounded_lg()
                            .bg(PANEL)
                            .child(div().text_sm().text_color(TEXT).child(address.clone()))
                            .child(action_button("Copy URL", false).on_mouse_down(
                                MouseButton::Left,
                                move |_, _, cx| {
                                    cx.write_to_clipboard(ClipboardItem::new_string(
                                        endpoint.clone(),
                                    ));
                                },
                            )),
                    )
                    .child(
                        div()
                            .min_h(px(48.))
                            .p_3()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_2()
                            .rounded_lg()
                            .bg(PANEL)
                            .child(div().flex_1().text_xs().text_color(MUTED).child(token))
                            .child(
                                action_button(
                                    if self.reveal_api_token {
                                        "Hide"
                                    } else {
                                        "Reveal"
                                    },
                                    false,
                                )
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(|app, _, _, cx| {
                                        app.reveal_api_token = !app.reveal_api_token;
                                        cx.notify();
                                    }),
                                ),
                            )
                            .child(action_button("Copy token", false).on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|app, _, _, cx| {
                                    cx.write_to_clipboard(ClipboardItem::new_string(
                                        app.settings.api_token.clone(),
                                    ));
                                    app.notice = "API token copied".into();
                                    cx.notify();
                                }),
                            )),
                    ),
            )
            .child(
                div()
                    .w_full()
                    .p_5()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .rounded_xl()
                    .bg(PANEL_ALT)
                    .child(
                        div()
                            .text_lg()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Cleanup"),
                    )
                    .child(div().text_sm().text_color(MUTED).child(
                        "Cleanup stops active mail requests. Stop batch checks before deleting data.",
                    ))
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .gap_2()
                            .child(action_button("Clear accounts", false).on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|app, _, _, cx| {
                                    app.clear_data(ImportKind::Accounts, cx)
                                }),
                            ))
                            .child(action_button("Clear proxies", false).on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|app, _, _, cx| {
                                    app.clear_data(ImportKind::Proxies, cx)
                                }),
                            ))
                            .child(action_button("Clear history", false).on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|app, _, _, cx| app.clear_history(cx)),
                            ))
                            .child(action_button("Reset verdicts", false).on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|app, _, _, cx| app.reset_results(cx)),
                            )),
                    ),
            )
    }
}

impl Render for MailApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content = match self.workspace {
            Workspace::Accounts => self.render_accounts(window, cx).into_any_element(),
            Workspace::Mail => self.client.clone().into_any_element(),
            Workspace::History => self.render_history(cx).into_any_element(),
            Workspace::Proxies => self.render_proxies(cx).into_any_element(),
            Workspace::Settings => self.render_settings(cx).into_any_element(),
        };
        let is_client = self.workspace == Workspace::Mail;
        let workspace = div()
            .size_full()
            .on_key_down(cx.listener(|app, event: &KeyDownEvent, _, cx| {
                if app.workspace != Workspace::Mail
                    && event.keystroke.key == "v"
                    && event.keystroke.modifiers.secondary()
                {
                    app.paste_from_clipboard(cx);
                    cx.stop_propagation();
                }
            }))
            .flex()
            .flex_row()
            .font_family("Onest")
            .bg(BG)
            .text_color(TEXT)
            .when(!is_client, |view| view.child(self.render_sidebar(cx)))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .flex()
                    .flex_col()
                    .child(content)
                    .when(!is_client, |view| {
                        view.child(
                            div()
                                .h(px(34.))
                                .w_full()
                                .flex()
                                .flex_row()
                                .items_center()
                                .justify_between()
                                .px_4()
                                .border_t_1()
                                .border_color(BORDER)
                                .child(div().text_xs().text_color(MUTED).child(self.notice.clone()))
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(MUTED)
                                        .child("Read-only IMAP · Local workspace"),
                                ),
                        )
                    }),
            );
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(BG)
            .text_color(TEXT)
            .child(
                div()
                    .h(px(34.))
                    .px_4()
                    .flex()
                    .items_center()
                    .gap_3()
                    .border_b_1()
                    .border_color(BORDER)
                    .child(
                        div()
                            .text_lg()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("HAKUS"),
                    )
                    .child(
                        Button::new("mode-client")
                            .label("Mail client")
                            .ghost()
                            .on_click(cx.listener(|app, _, _, cx| {
                                app.workspace = Workspace::Mail;
                                app.client
                                    .update(cx, |client, cx| client.refresh_accounts(cx));
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("mode-checker")
                            .label("Account checker")
                            .ghost()
                            .on_click(cx.listener(|app, _, _, cx| {
                                app.client.read(cx).hide_webviews(cx);
                                app.workspace = Workspace::Accounts;
                                app.reload_data();
                                cx.notify();
                            })),
                    )
                    .child(div().flex_1())
                    .child(div().text_xs().text_color(MUTED).child(if is_client {
                        "Read-only mail workspace"
                    } else {
                        "Batch checks and workspace tools"
                    })),
            )
            .child(div().flex_1().min_h_0().child(workspace))
    }
}

fn sidebar_button(
    icon: &'static str,
    label: &'static str,
    selected: bool,
    workspace: Workspace,
    cx: &mut Context<MailApp>,
) -> gpui::Div {
    div()
        .h(px(44.))
        .w_full()
        .px_3()
        .flex()
        .flex_row()
        .items_center()
        .gap_3()
        .rounded_lg()
        .cursor_pointer()
        .bg(if selected { PURPLE_SOFT } else { PANEL })
        .hover(|style| style.bg(if selected { rgba(0x30204d) } else { PANEL_ALT }))
        .text_sm()
        .font_weight(if selected {
            FontWeight::SEMIBOLD
        } else {
            FontWeight::MEDIUM
        })
        .text_color(if selected { TEXT } else { MUTED })
        .child(
            div()
                .w(px(20.))
                .text_sm()
                .text_color(if selected { BLUE } else { MUTED })
                .child(icon),
        )
        .child(label)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |app, _, _, cx| {
                app.workspace = workspace;
                if workspace == Workspace::Mail {
                    app.client
                        .update(cx, |client, cx| client.refresh_accounts(cx));
                }
                if workspace == Workspace::History {
                    if let Ok(runs) = app.store.runs(100) {
                        app.runs = runs;
                    }
                }
                cx.notify();
            }),
        )
}

fn sidebar_metric(label: &'static str, value: usize, color: gpui::Rgba) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .child(
            div()
                .text_lg()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(color)
                .child(value.to_string()),
        )
        .child(div().text_xs().text_color(MUTED).child(label))
}

fn overview_card(
    label: &'static str,
    value: String,
    detail: &'static str,
    color: gpui::Rgba,
) -> gpui::Div {
    div()
        .flex_1()
        .h_full()
        .px_4()
        .py_3()
        .flex()
        .flex_row()
        .items_center()
        .gap_3()
        .rounded_lg()
        .bg(PANEL_ALT)
        .child(div().w(px(3.)).h(px(28.)).rounded_full().bg(color))
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_baseline()
                        .gap_2()
                        .child(
                            div()
                                .text_lg()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(TEXT)
                                .child(value),
                        )
                        .child(div().text_xs().text_color(MUTED).child(label)),
                )
                .child(div().text_xs().text_color(MUTED).child(detail)),
        )
}

fn table_cell(value: String, width: f32, color: gpui::Rgba) -> gpui::Div {
    div().w(px(width)).text_sm().text_color(color).child(value)
}

fn status_pill(value: String, color: gpui::Rgba) -> gpui::Div {
    div()
        .h(px(26.))
        .px_3()
        .flex()
        .items_center()
        .rounded_full()
        .bg(PANEL_ALT)
        .text_xs()
        .font_weight(FontWeight::MEDIUM)
        .text_color(color)
        .child(value)
}

fn filter_button(
    label: &'static str,
    filter: AccountFilter,
    selected: AccountFilter,
    cx: &mut Context<MailApp>,
) -> gpui::Div {
    compact_button(label, filter == selected).on_mouse_down(
        MouseButton::Left,
        cx.listener(move |app, _, _, cx| {
            app.account_filter = filter;
            app.account_page = 0;
            cx.notify();
        }),
    )
}

fn format_short_timestamp(timestamp: i64) -> String {
    DateTime::from_timestamp(timestamp, 0).map_or_else(
        || "Unknown".into(),
        |date| {
            date.with_timezone(&Local)
                .format("%d.%m.%y %H:%M")
                .to_string()
        },
    )
}

fn status_label(status: &CheckStatus) -> (String, gpui::Rgba) {
    match status {
        CheckStatus::Idle => ("Not checked".into(), MUTED),
        CheckStatus::Checking => ("Checking…".into(), AMBER),
        CheckStatus::Valid { messages } => (format!("Valid · {messages}"), GREEN),
        CheckStatus::Invalid { .. } => ("Invalid".into(), RED),
        CheckStatus::Error { .. } => ("Error".into(), AMBER),
    }
}

fn history_metric(label: &'static str, value: usize, color: gpui::Rgba) -> gpui::Div {
    div()
        .text_sm()
        .text_color(color)
        .child(format!("{label} {value}"))
}

fn action_button(label: impl Into<SharedString>, accent: bool) -> gpui::Div {
    div()
        .h(px(40.))
        .cursor_pointer()
        .px_4()
        .flex()
        .items_center()
        .justify_center()
        .rounded_lg()
        .bg(if accent { PURPLE } else { PANEL_ALT })
        .hover(|style| style.bg(if accent { PURPLE_HOVER } else { rgb(0x2b2b31) }))
        .text_sm()
        .font_weight(FontWeight::MEDIUM)
        .text_color(if accent { TEXT } else { rgb(0xd8d8de) })
        .child(label.into())
}

fn danger_button(label: impl Into<SharedString>) -> gpui::Div {
    div()
        .h(px(40.))
        .cursor_pointer()
        .px_4()
        .flex()
        .items_center()
        .justify_center()
        .rounded_lg()
        .bg(rgba(0x2a191c))
        .hover(|style| style.bg(rgba(0x3a1d22)))
        .text_sm()
        .font_weight(FontWeight::MEDIUM)
        .text_color(RED)
        .child(label.into())
}

fn compact_button(label: impl Into<SharedString>, selected: bool) -> gpui::Div {
    div()
        .h(px(36.))
        .min_w(px(48.))
        .px_4()
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .cursor_pointer()
        .bg(if selected { PURPLE_SOFT } else { PANEL })
        .hover(|style| style.bg(if selected { rgba(0x30204d) } else { PANEL_ALT }))
        .text_sm()
        .font_weight(if selected {
            FontWeight::SEMIBOLD
        } else {
            FontWeight::MEDIUM
        })
        .text_color(if selected { rgb(0xc4a7ff) } else { MUTED })
        .child(label.into())
}

fn icon_button(label: &'static str) -> gpui::Div {
    div()
        .size(px(36.))
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .cursor_pointer()
        .bg(PANEL_ALT)
        .hover(|style| style.bg(PURPLE))
        .text_color(TEXT)
        .child(label)
}

fn panel_header(title: impl Into<SharedString>) -> gpui::Div {
    div()
        .h(px(46.))
        .w_full()
        .px_4()
        .py_3()
        .border_b_1()
        .border_color(BORDER)
        .text_xs()
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(MUTED)
        .child(title.into())
}

fn empty_state(title: &'static str, subtitle: &'static str) -> gpui::Div {
    div()
        .flex_1()
        .p_6()
        .flex()
        .flex_col()
        .gap_2()
        .child(
            div()
                .text_sm()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(MUTED)
                .child(title),
        )
        .child(div().text_sm().text_color(MUTED).child(subtitle))
}

fn format_timestamp(timestamp: i64) -> String {
    DateTime::from_timestamp(timestamp, 0).map_or_else(
        || "UNKNOWN".into(),
        |date| {
            date.with_timezone(&Local)
                .format("%d.%m.%Y %H:%M:%S")
                .to_string()
        },
    )
}

fn read_text_limited(path: &Path) -> Result<String> {
    let metadata = fs::metadata(path).context("inspect import file")?;
    anyhow::ensure!(metadata.len() <= 16 * 1024 * 1024, "file exceeds 16 MiB");
    fs::read_to_string(path).context("read UTF-8 import file")
}

fn default_database_path() -> PathBuf {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    #[cfg(target_os = "macos")]
    let directory = home.join("Library/Application Support/FuckThunder");
    #[cfg(not(target_os = "macos"))]
    let directory = home.join(".local/share/fuckthunder");
    directory.join("workspace.db")
}

fn run(args: Args) -> Result<()> {
    let database_path = args.database.unwrap_or_else(default_database_path);
    if let Some(parent) = database_path.parent() {
        fs::create_dir_all(parent).context("create database directory")?;
    }
    let store = Arc::new(Store::open(&database_path).context("open SQLite workspace")?);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&database_path, fs::Permissions::from_mode(0o600))
            .context("restrict database permissions")?;
    }
    if let Some(path) = args.file {
        let input = read_text_limited(&path)?;
        store
            .import_accounts(&input)
            .context("import startup accounts")?;
    }
    if let Some(proxy) = args.proxy {
        store
            .import_proxies(&proxy)
            .context("import startup proxy")?;
    }
    let runtime = Arc::new(Runtime::new().context("start Tokio runtime")?);
    let concurrency = args.concurrency.clamp(1, 64);
    Application::new().run(move |cx| {
        gpui_component::init(cx);
        cx.on_action(|_: &Quit, cx| cx.quit());
        cx.bind_keys([
            KeyBinding::new("cmd-q", Quit, None),
            KeyBinding::new("ctrl-q", Quit, None),
        ]);
        cx.set_menus(vec![Menu {
            name: "Hakus".into(),
            items: vec![MenuItem::action("Quit Hakus", Quit)],
        }]);
        Theme::change(ThemeMode::Dark, None, cx);
        let theme = Theme::global_mut(cx);
        theme.colors.primary = rgb(0x75b9ff).into();
        theme.colors.primary_foreground = rgb(0x111722).into();
        theme.font_family = "Onest".into();
        cx.text_system()
            .add_fonts(vec![std::borrow::Cow::Borrowed(include_bytes!(
                "../assets/Onest.ttf"
            ))])
            .expect("load Onest font");
        let bounds = Bounds::centered(None, size(px(980.), px(680.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(size(px(800.), px(560.))),
                ..WindowOptions::default()
            },
            {
                let store = store.clone();
                let runtime = runtime.clone();
                let database_path = database_path.clone();
                let message_path = args.message.clone();
                move |window, cx| {
                    let app = cx.new(|cx| {
                        MailApp::new(
                            runtime.clone(),
                            store.clone(),
                            database_path.clone(),
                            concurrency,
                            window,
                            cx,
                        )
                    });
                    if let Some(path) = &message_path {
                        app.update(cx, |app, cx| {
                            app.client
                                .update(cx, |client, cx| client.open_message_file(path, cx))
                        });
                    }
                    cx.new(|cx| Root::new(app, window, cx))
                }
            },
        )
        .expect("open fuckthunder window");
        cx.activate(true);
    });
    Ok(())
}

fn main() -> Result<()> {
    run(Args::parse())
}
