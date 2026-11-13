//! Client workspace with independent, scrollable mailbox tabs.

mod form;
mod mailbox;
mod viewer;

use std::sync::Arc;

use fuckthunder_gui::Store;
use gpui::{div, prelude::*, px, rgb, Context, Entity, ScrollHandle, Subscription, Window};
use gpui_component::{
    button::{Button, ButtonVariants},
    tab::{Tab, TabBar},
    Sizable,
};
use tokio::runtime::Runtime;

use mailbox::Mailbox;

fn compact_email(email: &str) -> String {
    if email.chars().count() <= 26 {
        return email.to_owned();
    }
    match email.split_once('@') {
        Some((local, domain)) => {
            format!("{}…@{}", local.chars().take(10).collect::<String>(), domain)
        }
        None => format!("{}…", email.chars().take(25).collect::<String>()),
    }
}

struct OpenMailbox {
    id: u64,
    view: Entity<Mailbox>,
    _changes: Subscription,
}

pub(super) struct Client {
    runtime: Arc<Runtime>,
    store: Arc<Store>,
    tabs: Vec<OpenMailbox>,
    active: usize,
    next_id: u64,
    scroll: ScrollHandle,
}

impl Client {
    pub(super) fn new(
        runtime: Arc<Runtime>,
        store: Arc<Store>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut client = Self {
            runtime,
            store,
            tabs: Vec::new(),
            active: 0,
            next_id: 0,
            scroll: ScrollHandle::new(),
        };
        client.add_tab(window, cx);
        client
    }

    fn add_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let view = cx.new(|cx| Mailbox::new(self.runtime.clone(), self.store.clone(), window, cx));
        let changes = cx.observe(&view, |_, _, cx| cx.notify());
        let id = self.next_id;
        self.next_id += 1;
        self.tabs.push(OpenMailbox {
            id,
            view,
            _changes: changes,
        });
        self.activate(self.tabs.len() - 1, window, cx);
    }

    fn choose_message(&mut self, cx: &mut Context<Self>) {
        let prompt = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Open message (.eml)".into()),
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = prompt.await {
                if let Some(path) = paths.first() {
                    let path = path.clone();
                    let _ = this.update(cx, |this, cx| this.open_message_file(&path, cx));
                }
            }
        })
        .detach();
    }

    fn activate(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        for tab in &self.tabs {
            tab.view.read(cx).hide_webview(cx);
        }
        self.active = index;
        self.tabs[index]
            .view
            .update(cx, |mailbox, cx| mailbox.focus(window, cx));
        self.scroll.scroll_to_item(index);
        cx.notify();
    }

    fn open_mailbox_window(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.iter().find(|tab| tab.id == id) else {
            return;
        };
        let account = tab.view.read(cx).account_id();
        let runtime = self.runtime.clone();
        let store = self.store.clone();
        let bounds = gpui::Bounds::centered(None, gpui::size(px(980.), px(680.)), cx);
        if let Err(error) = cx.open_window(
            gpui::WindowOptions {
                window_bounds: Some(gpui::WindowBounds::Windowed(bounds)),
                window_min_size: Some(gpui::size(px(800.), px(560.))),
                ..Default::default()
            },
            move |window, cx| {
                let client = cx.new(|cx| {
                    let mut client = Self::new(runtime, store, window, cx);
                    if let Some(account) = account {
                        client.open_saved(account, window, cx);
                    }
                    client
                });
                cx.new(|cx| gpui_component::Root::new(client, window, cx))
            },
        ) {
            tab.view.update(cx, |mailbox, cx| {
                mailbox.report_window_error(error.to_string(), cx)
            });
        }
    }

    fn close_tab(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(index) = self.tabs.iter().position(|tab| tab.id == id) else {
            return;
        };
        self.tabs.remove(index);
        if self.tabs.is_empty() {
            self.add_tab(window, cx);
        } else {
            let active = if index < self.active {
                self.active - 1
            } else {
                self.active.min(self.tabs.len() - 1)
            };
            self.activate(active, window, cx);
        }
    }

    pub(super) fn open_message_file(&mut self, path: &std::path::Path, cx: &mut Context<Self>) {
        self.tabs[self.active]
            .view
            .update(cx, |mailbox, cx| mailbox.open_local_message(path, cx));
    }

    pub(super) fn hide_webviews(&self, cx: &gpui::App) {
        for tab in &self.tabs {
            tab.view.read(cx).hide_webview(cx);
        }
    }

    pub(super) fn refresh_accounts(&mut self, cx: &mut Context<Self>) {
        cx.notify();
    }

    pub(super) fn cancel_operations(&mut self, cx: &mut Context<Self>) {
        for tab in &self.tabs {
            tab.view
                .update(cx, |mailbox, cx| mailbox.cancel_operation(cx));
        }
    }

    pub(super) fn open_saved(&mut self, id: i64, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(index) = self
            .tabs
            .iter()
            .position(|tab| tab.view.read(cx).account_id() == Some(id))
        {
            self.activate(index, window, cx);
            return;
        }
        if self.tabs[self.active].view.read(cx).account_id().is_some()
            || self.tabs[self.active].view.read(cx).is_busy()
            || self.tabs[self.active].view.read(cx).is_local_message()
        {
            self.add_tab(window, cx);
        }
        self.tabs[self.active]
            .view
            .update(cx, |mailbox, cx| mailbox.open_saved(id, cx));
        cx.notify();
    }
}

impl Render for Client {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tabs = self
            .tabs
            .iter()
            .map(|tab| {
                let mailbox = tab.view.read(cx);
                let prefix = if mailbox.is_busy() {
                    "◌ "
                } else if mailbox.has_error() {
                    "! "
                } else {
                    ""
                };
                let id = tab.id;
                Tab::new()
                    .label(format!("{prefix}{}", compact_email(&mailbox.title(cx))))
                    .suffix(
                        div()
                            .flex()
                            .items_center()
                            .gap_1()
                            .child(
                                Button::new(("window-mailbox", id))
                                    .label("↗")
                                    .xsmall()
                                    .ghost()
                                    .tooltip("Open mailbox in a separate window")
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        cx.stop_propagation();
                                        this.open_mailbox_window(id, cx);
                                    })),
                            )
                            .child(
                                Button::new(("close-mailbox", id))
                                    .label("×")
                                    .xsmall()
                                    .ghost()
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        cx.stop_propagation();
                                        this.close_tab(id, window, cx);
                                    })),
                            ),
                    )
                    .min_w(px(140.))
                    .max_w(px(220.))
                    .flex_shrink_0()
            })
            .collect::<Vec<_>>();
        let saved = if self.tabs[self.active].view.read(cx).is_adding() {
            self.store.accounts().unwrap_or_default()
        } else {
            Vec::new()
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(0x17191d))
            .child(
                TabBar::new("mailbox-tabs")
                    .selected_index(self.active)
                    .track_scroll(&self.scroll)
                    .menu(true)
                    .children(tabs)
                    .suffix(
                        div()
                            .flex()
                            .items_center()
                            .gap_1()
                            .child(
                                Button::new("open-message-file")
                                    .label("Open .eml")
                                    .small()
                                    .ghost()
                                    .on_click(
                                        cx.listener(|this, _, _, cx| this.choose_message(cx)),
                                    ),
                            )
                            .child(
                                Button::new("active-mailbox-window")
                                    .label("New window")
                                    .small()
                                    .ghost()
                                    .tooltip("Open the current mailbox in another window")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        let id = this.tabs[this.active].id;
                                        this.open_mailbox_window(id, cx);
                                    })),
                            )
                            .child(
                                Button::new("new-mailbox-tab")
                                    .label("+ Add mailbox")
                                    .small()
                                    .tooltip("New mailbox")
                                    .ghost()
                                    .on_click(
                                        cx.listener(|this, _, window, cx| this.add_tab(window, cx)),
                                    ),
                            ),
                    )
                    .on_click(
                        cx.listener(|this, index, window, cx| this.activate(*index, window, cx)),
                    ),
            )
            .when(!saved.is_empty(), |view| {
                view.child(
                    div()
                        .id("saved-mailboxes")
                        .h(px(40.))
                        .px_3()
                        .flex()
                        .gap_2()
                        .items_center()
                        .overflow_x_scroll()
                        .child(div().text_xs().text_color(rgb(0x9ca4b1)).child("SAVED"))
                        .children(saved.into_iter().map(|account| {
                            let id = account.id;
                            Button::new(("open-saved", id as u64))
                                .label(account.account.email)
                                .ghost()
                                .flex_shrink_0()
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.open_saved(id, window, cx)
                                }))
                        })),
                )
            })
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .child(self.tabs[self.active].view.clone()),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{TestAppContext, VisualTestContext};
    use gpui_component::{Root, Theme, ThemeMode};

    // Opening another mailbox window must not move or close the original tab.
    #[gpui::test]
    fn opens_independent_mailbox_window_without_removing_original(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
            Theme::change(ThemeMode::Dark, None, cx);
        });
        let runtime = Arc::new(Runtime::new().unwrap());
        let store = Arc::new(Store::open_in_memory().unwrap());
        let mut client = None;
        let window = cx.add_window(|window, cx| {
            let view = cx.new(|cx| Client::new(runtime, store, window, cx));
            client = Some(view.clone());
            Root::new(view, window, cx)
        });
        let client = client.unwrap();
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        visual.update(|_, cx| {
            let before = cx.windows().len();
            client.update(cx, |client, cx| {
                let id = client.tabs[0].id;
                client.open_mailbox_window(id, cx);
                assert_eq!(client.tabs.len(), 1);
                assert_eq!(client.active, 0);
            });
            assert_eq!(cx.windows().len(), before + 1);
        });
    }

    #[gpui::test]
    fn keeps_twenty_open_mailboxes_independent_when_switching_and_closing(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
            Theme::change(ThemeMode::Dark, None, cx);
        });
        let runtime = Arc::new(Runtime::new().unwrap());
        let store = Arc::new(Store::open_in_memory().unwrap());
        let mut client = None;
        let window = cx.add_window(|window, cx| {
            let view = cx.new(|cx| Client::new(runtime, store, window, cx));
            client = Some(view.clone());
            Root::new(view, window, cx)
        });
        let client = client.unwrap();
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        for index in 0..20 {
            if index > 0 {
                visual.update(|window, cx| {
                    client.update(cx, |client, cx| client.add_tab(window, cx))
                });
            }
            visual.simulate_input(&format!("mailbox{index}@example.com"));
        }
        for index in [0, 19, 7, 12, 3] {
            visual.update(|window, cx| {
                client.update(cx, |client, cx| client.activate(index, window, cx));
                let view = client.read(cx).tabs[index].view.clone();
                view.update(cx, |mailbox, _| {
                    mailbox.set_context(&format!("query-{index}"), index as u32 + 100)
                });
            });
            visual.update(|window, cx| {
                client.update(cx, |client, cx| client.activate(index, window, cx))
            });
            visual.read(|cx| {
                let client = client.read(cx);
                assert_eq!(
                    client.tabs[client.active].view.read(cx).title(cx),
                    format!("mailbox{index}@example.com")
                );
                assert_eq!(
                    client.tabs[client.active].view.read(cx).context(),
                    (format!("query-{index}").as_str(), Some(index as u32 + 100))
                );
            });
        }
        visual.update(|window, cx| {
            client.update(cx, |client, cx| {
                let id = client.tabs[0].id;
                client.close_tab(id, window, cx);
            })
        });
        visual.read(|cx| {
            let client = client.read(cx);
            assert_eq!(
                client.tabs[client.active].view.read(cx).title(cx),
                "mailbox3@example.com"
            );
        });
        visual.update(|window, cx| {
            client.update(cx, |client, cx| {
                while client.tabs.len() > 1 {
                    let id = client.tabs[client.active].id;
                    client.close_tab(id, window, cx);
                }
                let id = client.tabs[0].id;
                client.close_tab(id, window, cx);
            })
        });
        visual.read(|cx| {
            let client = client.read(cx);
            assert_eq!(
                client.tabs[client.active].view.read(cx).title(cx),
                "New mailbox"
            );
        });
    }
}
