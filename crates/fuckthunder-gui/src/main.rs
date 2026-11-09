//! GPUI desktop shell for the read-only account checker and mail viewer.

use std::{path::PathBuf, sync::Arc, time::Duration};

use anyhow::{Context as _, Result};
use clap::Parser;
use fuckthunder_core::Error;
use fuckthunder_gui::{
    check_accounts, load_folders, load_message, load_page, parse_accounts, parse_proxy, Account,
    CheckStatus, MessagePage, MessageSummary, MessageView, SessionOptions, PAGE_SIZE,
};
use gpui::{
    div, prelude::*, px, rgb, size, uniform_list, Application, Bounds, Context, FontWeight,
    MouseButton, SharedString, Window, WindowBounds, WindowOptions,
};
use tokio::{runtime::Runtime, sync::mpsc};

#[derive(Debug, Parser)]
#[command(
    name = "fuckthunder-gui",
    about = "Read-only GPUI IMAP checker and viewer"
)]
struct Args {
    /// UTF-8 file containing one email:password row per line.
    file: PathBuf,
    /// Maximum concurrent account checks.
    #[arg(long, default_value_t = 8)]
    concurrency: usize,
    /// SOCKS5H or HTTP CONNECT proxy used for every account operation.
    #[arg(long)]
    proxy: Option<String>,
}

struct AccountRow {
    account: Account,
    status: CheckStatus,
}

struct MailApp {
    runtime: Arc<Runtime>,
    options: SessionOptions,
    concurrency: usize,
    accounts: Vec<AccountRow>,
    selected_account: Option<usize>,
    folders: Vec<String>,
    selected_folder: Option<String>,
    page: MessagePage,
    selected_message: Option<u32>,
    detail: Option<MessageView>,
    checking: bool,
    busy: bool,
    notice: String,
}

impl MailApp {
    fn new(
        runtime: Arc<Runtime>,
        options: SessionOptions,
        concurrency: usize,
        accounts: Vec<Account>,
    ) -> Self {
        let count = accounts.len();
        Self {
            runtime,
            options,
            concurrency,
            accounts: accounts
                .into_iter()
                .map(|account| AccountRow {
                    account,
                    status: CheckStatus::Idle,
                })
                .collect(),
            selected_account: None,
            folders: Vec::new(),
            selected_folder: None,
            page: MessagePage::default(),
            selected_message: None,
            detail: None,
            checking: false,
            busy: false,
            notice: format!("{count} accounts loaded · read-only IMAP · 50 messages/page"),
        }
    }

    fn start_checks(&mut self, cx: &mut Context<Self>) {
        if self.checking || self.accounts.is_empty() {
            return;
        }
        self.checking = true;
        let accounts: Vec<_> = self
            .accounts
            .iter_mut()
            .enumerate()
            .map(|(index, row)| {
                row.status = CheckStatus::Checking;
                (index, row.account.clone())
            })
            .collect();
        self.notice = format!("Checking {} accounts…", accounts.len());
        cx.notify();

        let runtime = self.runtime.clone();
        let options = self.options.clone();
        let concurrency = self.concurrency;
        cx.spawn(async move |this, cx| {
            let (sender, mut receiver) = mpsc::unbounded_channel();
            let task = runtime.spawn(check_accounts(accounts, options, concurrency, sender));
            while let Some(result) = receiver.recv().await {
                let _ = this.update(cx, |app, cx| {
                    if let Some(row) = app.accounts.get_mut(result.index) {
                        row.status = result.status;
                    }
                    cx.notify();
                });
            }
            let join_error = task.await.err().map(|error| error.to_string());
            let _ = this.update(cx, |app, cx| {
                app.checking = false;
                let valid = app
                    .accounts
                    .iter()
                    .filter(|row| matches!(row.status, CheckStatus::Valid { .. }))
                    .count();
                let invalid = app
                    .accounts
                    .iter()
                    .filter(|row| matches!(row.status, CheckStatus::Invalid { .. }))
                    .count();
                let errors = app
                    .accounts
                    .iter()
                    .filter(|row| matches!(row.status, CheckStatus::Error { .. }))
                    .count();
                app.notice = join_error.map_or_else(
                    || format!("Finished · {valid} valid · {invalid} invalid · {errors} errors"),
                    |error| format!("Batch failed: {error}"),
                );
                cx.notify();
            });
        })
        .detach();
    }

    fn select_account(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.busy || index >= self.accounts.len() {
            return;
        }
        self.selected_account = Some(index);
        self.folders.clear();
        self.selected_folder = None;
        self.page = MessagePage::default();
        self.selected_message = None;
        self.detail = None;
        self.busy = true;
        self.notice = "Loading folders…".into();
        cx.notify();

        let account = self.accounts[index].account.clone();
        let options = self.options.clone();
        let runtime = self.runtime.clone();
        cx.spawn(async move |this, cx| {
            let result = runtime
                .spawn(async move { load_folders(&account, &options).await })
                .await
                .map_err(|error| Error::Protocol(error.to_string()))
                .and_then(|result| result);
            let _ = this.update(cx, |app, cx| {
                app.busy = false;
                match result {
                    Ok(folders) => {
                        app.notice = format!("{} folders", folders.len());
                        app.folders = folders.into_iter().map(|folder| folder.name).collect();
                    }
                    Err(error) => app.notice = format!("Folder load failed: {error}"),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn select_folder(&mut self, folder: String, cx: &mut Context<Self>) {
        self.selected_folder = Some(folder);
        self.page = MessagePage::default();
        self.selected_message = None;
        self.detail = None;
        self.load_page(0, cx);
    }

    fn load_page(&mut self, page: usize, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let Some(account_index) = self.selected_account else {
            return;
        };
        let Some(folder) = self.selected_folder.clone() else {
            return;
        };
        if page > 0 && page.saturating_mul(PAGE_SIZE) >= self.page.total {
            return;
        }
        self.busy = true;
        self.notice = format!("Loading page {}…", page + 1);
        self.selected_message = None;
        self.detail = None;
        cx.notify();

        let account = self.accounts[account_index].account.clone();
        let options = self.options.clone();
        let runtime = self.runtime.clone();
        cx.spawn(async move |this, cx| {
            let result = runtime
                .spawn(async move { load_page(&account, &options, &folder, page).await })
                .await
                .map_err(|error| Error::Protocol(error.to_string()))
                .and_then(|result| result);
            let _ = this.update(cx, |app, cx| {
                app.busy = false;
                match result {
                    Ok(page) => {
                        app.notice = format!(
                            "Page {}/{} · {} messages",
                            page.page + 1,
                            page.total.max(1).div_ceil(PAGE_SIZE),
                            page.total
                        );
                        app.page = page;
                    }
                    Err(error) => app.notice = format!("Message list failed: {error}"),
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
        let Some(account_index) = self.selected_account else {
            return;
        };
        let Some(folder) = self.selected_folder.clone() else {
            return;
        };
        self.busy = true;
        self.selected_message = Some(uid);
        self.detail = None;
        self.notice = format!("Loading UID {uid} with BODY.PEEK[]…");
        cx.notify();

        let account = self.accounts[account_index].account.clone();
        let options = self.options.clone();
        let runtime = self.runtime.clone();
        cx.spawn(async move |this, cx| {
            let result = runtime
                .spawn(async move { load_message(&account, &options, &folder, uid).await })
                .await
                .map_err(|error| Error::Protocol(error.to_string()))
                .and_then(|result| result);
            let _ = this.update(cx, |app, cx| {
                app.busy = false;
                match result {
                    Ok(message) => {
                        app.notice = format!("UID {uid} loaded · flags unchanged");
                        app.detail = Some(message);
                    }
                    Err(error) => app.notice = format!("Message load failed: {error}"),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn account_row(&self, index: usize) -> gpui::Div {
        let row = &self.accounts[index];
        let selected = self.selected_account == Some(index);
        let (status, color) = match &row.status {
            CheckStatus::Idle => ("IDLE".to_owned(), rgb(0x8b98a9)),
            CheckStatus::Checking => ("CHECKING".to_owned(), rgb(0xf0b35a)),
            CheckStatus::Valid { messages } => (format!("VALID · {messages}"), rgb(0x58d68d)),
            CheckStatus::Invalid { .. } => ("INVALID".to_owned(), rgb(0xff6b6b)),
            CheckStatus::Error { .. } => ("ERROR".to_owned(), rgb(0xf0b35a)),
        };
        div()
            .h(px(52.))
            .w_full()
            .px_3()
            .py_2()
            .border_b_1()
            .border_color(rgb(0x202b38))
            .bg(if selected {
                rgb(0x1b3b57)
            } else {
                rgb(0x111923)
            })
            .hover(|style| style.bg(rgb(0x192a3a)))
            .child(
                div()
                    .w_full()
                    .text_sm()
                    .text_color(rgb(0xe5edf5))
                    .child(row.account.email.clone()),
            )
            .child(div().text_xs().text_color(color).child(status))
    }

    fn message_row(&self, index: usize) -> gpui::Div {
        let message: &MessageSummary = &self.page.messages[index];
        let selected = self.selected_message == Some(message.uid);
        div()
            .h(px(76.))
            .w_full()
            .px_3()
            .py_2()
            .border_b_1()
            .border_color(rgb(0x202b38))
            .bg(if selected {
                rgb(0x1b3b57)
            } else {
                rgb(0x111923)
            })
            .hover(|style| style.bg(rgb(0x192a3a)))
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(0xe5edf5))
                    .child(message.subject.clone()),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(0x9eacbc))
                    .child(message.from.clone()),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(0x6f8094))
                    .child(format!("{} · {} bytes", message.date, message.size)),
            )
    }
}

impl Render for MailApp {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let app = cx.entity().clone();
        let accounts_list = uniform_list("accounts", self.accounts.len(), {
            let app = app.clone();
            move |range, _window, cx| {
                range
                    .map(|index| {
                        let row = app.read(cx).account_row(index);
                        let app = app.clone();
                        row.on_mouse_down(MouseButton::Left, move |_, _, cx| {
                            app.update(cx, |app, cx| app.select_account(index, cx));
                        })
                    })
                    .collect()
            }
        });

        let folders_list = uniform_list("folders", self.folders.len(), {
            let app = app.clone();
            move |range, _window, cx| {
                range
                    .map(|index| {
                        let this = app.read(cx);
                        let folder = this.folders[index].clone();
                        let selected = this.selected_folder.as_ref() == Some(&folder);
                        let row = div()
                            .h(px(40.))
                            .w_full()
                            .px_3()
                            .py_2()
                            .border_b_1()
                            .border_color(rgb(0x202b38))
                            .bg(if selected {
                                rgb(0x1b3b57)
                            } else {
                                rgb(0x111923)
                            })
                            .hover(|style| style.bg(rgb(0x192a3a)))
                            .text_sm()
                            .text_color(rgb(0xe5edf5))
                            .child(folder.clone());
                        let app = app.clone();
                        row.on_mouse_down(MouseButton::Left, move |_, _, cx| {
                            app.update(cx, |app, cx| app.select_folder(folder.clone(), cx));
                        })
                    })
                    .collect()
            }
        });

        let messages_list = uniform_list("messages", self.page.messages.len(), {
            let app = app.clone();
            move |range, _window, cx| {
                range
                    .map(|index| {
                        let row = app.read(cx).message_row(index);
                        let uid = app.read(cx).page.messages[index].uid;
                        let app = app.clone();
                        row.on_mouse_down(MouseButton::Left, move |_, _, cx| {
                            app.update(cx, |app, cx| app.open_message(uid, cx));
                        })
                    })
                    .collect()
            }
        });

        let detail = self.detail.as_ref().map_or_else(
            || {
                div().text_color(rgb(0x6f8094)).child(
                    "Select a message. Full bodies are fetched lazily; HTML is not executed.",
                )
            },
            |message| {
                div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(
                        div()
                            .text_lg()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(0xf2f6fa))
                            .child(message.subject.clone()),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(rgb(0xaab8c8))
                            .child(format!("From: {}", message.from)),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(rgb(0xaab8c8))
                            .child(format!("To: {}", message.to)),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(rgb(0x7f90a4))
                            .child(message.date.clone()),
                    )
                    .child(
                        div()
                            .pt_3()
                            .border_t_1()
                            .border_color(rgb(0x263446))
                            .text_sm()
                            .text_color(rgb(0xdde6ef))
                            .child(message.body.clone()),
                    )
            },
        );

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(0x0b1118))
            .text_color(rgb(0xdde6ef))
            .child(
                div()
                    .h(px(64.))
                    .w_full()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .px_4()
                    .border_b_1()
                    .border_color(rgb(0x263446))
                    .child(
                        div()
                            .text_lg()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("FUCKTHUNDER / READ-ONLY MAIL"),
                    )
                    .child(
                        div()
                            .px_4()
                            .py_2()
                            .rounded_md()
                            .bg(if self.checking {
                                rgb(0x314154)
                            } else {
                                rgb(0x176b52)
                            })
                            .hover(|style| style.bg(rgb(0x20876a)))
                            .child(if self.checking {
                                "CHECKING…"
                            } else {
                                "CHECK ALL"
                            })
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|app, _, _, cx| {
                                    app.start_checks(cx);
                                }),
                            ),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .flex()
                    .flex_row()
                    .child(panel("ACCOUNTS", accounts_list, 300.))
                    .child(panel("FOLDERS", folders_list, 190.))
                    .child(
                        div()
                            .w(px(380.))
                            .h_full()
                            .flex()
                            .flex_col()
                            .border_r_1()
                            .border_color(rgb(0x263446))
                            .child(panel_header(format!(
                                "MESSAGES · PAGE {}",
                                self.page.page + 1
                            )))
                            .child(div().flex_1().min_h_0().child(messages_list))
                            .child(
                                div()
                                    .h(px(44.))
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .justify_between()
                                    .px_3()
                                    .border_t_1()
                                    .border_color(rgb(0x263446))
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(rgb(0xaab8c8))
                                            .child("← PREV")
                                            .on_mouse_down(
                                                MouseButton::Left,
                                                cx.listener(|app, _, _, cx| {
                                                    app.load_page(
                                                        app.page.page.saturating_sub(1),
                                                        cx,
                                                    );
                                                }),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(rgb(0xaab8c8))
                                            .child("NEXT →")
                                            .on_mouse_down(
                                                MouseButton::Left,
                                                cx.listener(|app, _, _, cx| {
                                                    app.load_page(app.page.page + 1, cx);
                                                }),
                                            ),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .id("message-detail")
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .overflow_y_scroll()
                            .p_5()
                            .child(detail),
                    ),
            )
            .child(
                div()
                    .min_h(px(34.))
                    .w_full()
                    .px_4()
                    .py_2()
                    .border_t_1()
                    .border_color(rgb(0x263446))
                    .text_xs()
                    .text_color(rgb(0x8fa1b5))
                    .child(self.notice.clone()),
            )
    }
}

fn panel_header(title: impl Into<SharedString>) -> gpui::Div {
    div()
        .h(px(40.))
        .w_full()
        .px_3()
        .py_3()
        .border_b_1()
        .border_color(rgb(0x263446))
        .text_xs()
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(rgb(0x7f90a4))
        .child(title.into())
}

fn panel(title: &'static str, list: gpui::UniformList, width: f32) -> gpui::Div {
    div()
        .w(px(width))
        .h_full()
        .flex()
        .flex_col()
        .border_r_1()
        .border_color(rgb(0x263446))
        .child(panel_header(title))
        .child(div().flex_1().min_h_0().child(list))
}

fn run(args: Args) -> Result<()> {
    let metadata = std::fs::metadata(&args.file).context("inspect account file")?;
    anyhow::ensure!(
        metadata.len() <= 8 * 1024 * 1024,
        "account file exceeds 8 MiB"
    );
    let input = std::fs::read_to_string(&args.file).context("read account file")?;
    let accounts = parse_accounts(&input);
    let options = SessionOptions {
        proxy: args.proxy.as_deref().map(parse_proxy).transpose()?,
        timeout: Duration::from_secs(30),
    };
    let runtime = Arc::new(Runtime::new().context("start Tokio runtime")?);
    Application::new().run(move |cx| {
        let bounds = Bounds::centered(None, size(px(1560.), px(940.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..WindowOptions::default()
            },
            move |_, cx| {
                cx.new(|_| {
                    MailApp::new(
                        runtime.clone(),
                        options.clone(),
                        args.concurrency.clamp(1, 64),
                        accounts.clone(),
                    )
                })
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
