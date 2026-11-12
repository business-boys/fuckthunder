use std::fmt;

use gpui::{
    div, prelude::*, px, App, Context, Entity, EventEmitter, Focusable, Render, Subscription,
    Window,
};
use gpui_component::{
    button::{Button, ButtonVariants},
    input::{Input, InputEvent, InputState},
    Disableable,
};

const fn rgba(hex: u32) -> gpui::Rgba {
    gpui::Rgba {
        r: ((hex >> 16) & 0xff) as f32 / 255.0,
        g: ((hex >> 8) & 0xff) as f32 / 255.0,
        b: (hex & 0xff) as f32 / 255.0,
        a: 1.0,
    }
}

const SURFACE: gpui::Rgba = rgba(0x17191d);
const INPUT: gpui::Rgba = rgba(0x20242b);
const BORDER: gpui::Rgba = rgba(0x30343b);
const TEXT: gpui::Rgba = rgba(0xf2f2f5);
const MUTED: gpui::Rgba = rgba(0x9a9aa2);
const MINT: gpui::Rgba = rgba(0x75b9ff);
const INK: gpui::Rgba = rgba(0x0a0a0a);
const RED: gpui::Rgba = rgba(0xff5f56);

#[derive(Clone)]
pub(super) struct Submit {
    pub email: String,
    pub password: String,
    pub host: Option<String>,
    pub port: u16,
    pub starttls: bool,
    pub proxy: Option<String>,
}

impl fmt::Debug for Submit {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Submit")
            .field("email", &"[redacted]")
            .field("password", &"[redacted]")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("starttls", &self.starttls)
            .field("proxy", &"[redacted]")
            .finish()
    }
}

pub(super) struct Form {
    email: Entity<InputState>,
    password: Entity<InputState>,
    host: Entity<InputState>,
    port: Entity<InputState>,
    proxy: Entity<InputState>,
    advanced: bool,
    starttls: bool,
    busy: bool,
    error: Option<String>,
    port_error: Option<String>,
    _subscriptions: Vec<Subscription>,
}

impl Form {
    pub(super) fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let email = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("email@example.com or email:password")
                .validate(|value, _| !value.contains(['\r', '\n']))
        });
        let password = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Password or app password")
                .validate(|value, _| !value.contains(['\r', '\n']))
                .masked(true)
        });
        let host =
            cx.new(|cx| InputState::new(window, cx).placeholder("Automatic from email domain"));
        let port = cx.new(|cx| InputState::new(window, cx).default_value("993"));
        let proxy =
            cx.new(|cx| InputState::new(window, cx).placeholder("Direct connection (no proxy)"));
        let subscriptions =
            vec![
                cx.subscribe_in(&email, window, |this, _, event: &InputEvent, window, cx| {
                    match event {
                        InputEvent::Change => this.split_credentials(window, cx),
                        InputEvent::PressEnter { .. } => this.submit(cx),
                        _ => {}
                    }
                }),
                cx.subscribe(&password, |this, _, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::PressEnter { .. }) {
                        this.submit(cx);
                    }
                }),
                cx.subscribe(&port, |this, port, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::Change) {
                        this.port_error = parse_port(port.read(cx).value().trim())
                            .err()
                            .map(str::to_owned);
                        cx.notify();
                    }
                }),
            ];

        Self {
            email,
            password,
            host,
            port,
            proxy,
            advanced: false,
            starttls: false,
            busy: false,
            error: None,
            port_error: None,
            _subscriptions: subscriptions,
        }
    }

    pub(super) fn email(&self, cx: &App) -> String {
        self.email.read(cx).value().trim().to_owned()
    }

    fn split_credentials(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let value = self.email.read(cx).value();
        if let Some((email, password)) = value.split_once(':') {
            let email = email.trim().to_owned();
            let password = password.to_owned();
            self.email
                .update(cx, |input, cx| input.set_value(email, window, cx));
            self.password.update(cx, |input, cx| {
                input.set_value(password, window, cx);
                input.focus(window, cx);
            });
            self.error = None;
            cx.notify();
        }
    }

    fn paste_credentials(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.busy {
            return false;
        }
        let email_focused = self.email.read(cx).focus_handle(cx).is_focused(window);
        let password_focused = self.password.read(cx).focus_handle(cx).is_focused(window);
        if !email_focused && !password_focused {
            return false;
        }
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
            return false;
        };
        let text = text.trim_end_matches(['\r', '\n']);
        let Some((email, password)) = text.split_once(':') else {
            return false;
        };
        if !email.contains('@')
            || email.contains(['\r', '\n'])
            || password.is_empty()
            || password.contains(['\r', '\n'])
        {
            return false;
        }
        self.email.update(cx, |input, cx| {
            input.set_value(email.trim().to_owned(), window, cx)
        });
        self.password.update(cx, |input, cx| {
            input.set_value(password.to_owned(), window, cx)
        });
        self.submit(cx);
        true
    }

    pub(super) fn focus(&self, window: &mut Window, cx: &mut App) {
        self.email.update(cx, |email, cx| email.focus(window, cx));
    }

    pub(super) fn set_busy(&mut self, busy: bool, cx: &mut Context<Self>) {
        self.busy = busy;
        cx.notify();
    }

    pub(super) fn set_error(&mut self, error: Option<String>, cx: &mut Context<Self>) {
        self.error = error;
        cx.notify();
    }

    pub(super) fn clear_password(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.password
            .update(cx, |input, cx| input.set_value("", window, cx));
    }
    fn set_starttls(&mut self, starttls: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy || self.starttls == starttls {
            return;
        }

        self.starttls = starttls;
        self.port_error = None;
        let port = if starttls { "143" } else { "993" };
        self.port
            .update(cx, |state, cx| state.set_value(port, window, cx));
        cx.notify();
    }

    fn submit(&mut self, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }

        let port_value = self.port.read(cx).value();
        let port = match parse_port(port_value.trim()) {
            Ok(port) => port,
            Err(error) => {
                self.port_error = Some(error.to_owned());
                cx.notify();
                return;
            }
        };

        self.port_error = None;
        let email = self.email(cx);
        let password = self.password.read(cx).value().to_string();
        let proxy = non_empty(self.proxy.read(cx).value().trim());
        let host = non_empty(self.host.read(cx).value().trim());
        cx.notify();
        cx.emit(Submit {
            email,
            password,
            host,
            port,
            starttls: self.starttls,
            proxy,
        });
    }

    fn render_advanced(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let tls_form = cx.entity().clone();
        let starttls_form = cx.entity().clone();

        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap_3()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(field_label("IMAP server"))
                            .child(
                                Input::new(&self.host)
                                    .tab_index(3)
                                    .disabled(self.busy)
                                    .bg(INPUT)
                                    .border_color(BORDER)
                                    .text_color(TEXT),
                            ),
                    )
                    .child(
                        div()
                            .w(px(92.))
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(field_label("Port"))
                            .child(
                                Input::new(&self.port)
                                    .tab_index(4)
                                    .disabled(self.busy)
                                    .bg(INPUT)
                                    .border_color(BORDER)
                                    .text_color(TEXT),
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .child(field_label("Security"))
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .gap_2()
                            .child(
                                Button::new("security-tls")
                                    .label("TLS")
                                    .primary()
                                    .compact()
                                    .tab_index(5)
                                    .disabled(self.busy)
                                    .when(!self.starttls, |button| button.bg(MINT).text_color(INK))
                                    .when(self.starttls, |button| button.outline())
                                    .on_click(move |_, window, cx| {
                                        tls_form.update(cx, |form, cx| {
                                            form.set_starttls(false, window, cx);
                                        });
                                    }),
                            )
                            .child(
                                Button::new("security-starttls")
                                    .label("STARTTLS")
                                    .primary()
                                    .compact()
                                    .tab_index(6)
                                    .disabled(self.busy)
                                    .when(self.starttls, |button| button.bg(MINT).text_color(INK))
                                    .when(!self.starttls, |button| button.outline())
                                    .on_click(move |_, window, cx| {
                                        starttls_form.update(cx, |form, cx| {
                                            form.set_starttls(true, window, cx);
                                        });
                                    }),
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(field_label("Proxy URL (optional)"))
                    .child(
                        Input::new(&self.proxy)
                            .tab_index(7)
                            .disabled(self.busy)
                            .bg(INPUT)
                            .border_color(BORDER)
                            .text_color(TEXT),
                    ),
            )
    }
}

impl EventEmitter<Submit> for Form {}

impl Render for Form {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let advanced_form = cx.entity().clone();
        let connect_form = cx.entity().clone();
        let error = self.port_error.as_ref().or(self.error.as_ref());

        div()
            .id("mailbox-connection-form")
            .max_h_full()
            .capture_action(
                cx.listener(|this, _: &gpui_component::input::Paste, window, cx| {
                    if this.paste_credentials(window, cx) {
                        cx.stop_propagation();
                    } else {
                        cx.propagate();
                    }
                }),
            )
            .capture_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, window, cx| {
                if event.keystroke.modifiers.secondary()
                    && event.keystroke.key == "v"
                    && this.paste_credentials(window, cx)
                {
                    cx.stop_propagation();
                }
            }))
            .overflow_y_scroll()
            .w_full()
            .max_w(px(440.))
            .flex()
            .flex_col()
            .gap_3()
            .p_4()
            .rounded_xl()
            .border_1()
            .border_color(BORDER)
            .bg(SURFACE)
            .text_color(TEXT)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .text_lg()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child("Open a mailbox"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(MUTED)
                            .child("Paste email:password to connect instantly"),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(field_label("Email"))
                    .child(
                        Input::new(&self.email)
                            .tab_index(0)
                            .disabled(self.busy)
                            .bg(INPUT)
                            .border_color(BORDER)
                            .text_color(TEXT),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(field_label("Password"))
                    .child(
                        Input::new(&self.password)
                            .tab_index(1)
                            .disabled(self.busy)
                            .bg(INPUT)
                            .border_color(BORDER)
                            .text_color(TEXT),
                    ),
            )
            .child(
                Button::new("toggle-advanced")
                    .label(if self.advanced {
                        "Hide advanced"
                    } else {
                        "Advanced"
                    })
                    .ghost()
                    .compact()
                    .tab_index(2)
                    .disabled(self.busy)
                    .on_click(move |_, _, cx| {
                        advanced_form.update(cx, |form, cx| {
                            if !form.busy {
                                form.advanced = !form.advanced;
                                cx.notify();
                            }
                        });
                    }),
            )
            .when(self.advanced, |this| this.child(self.render_advanced(cx)))
            .when_some(error, |this, error| {
                this.child(div().text_sm().text_color(RED).child(error.to_owned()))
            })
            .child(
                Button::new("connect-mailbox")
                    .label(if self.busy {
                        "Connecting…"
                    } else {
                        "Connect"
                    })
                    .primary()
                    .loading(self.busy)
                    .disabled(self.busy)
                    .tab_index(8)
                    .w_full()
                    .bg(MINT)
                    .text_color(INK)
                    .on_click(move |_, _, cx| {
                        connect_form.update(cx, |form, cx| form.submit(cx));
                    }),
            )
            .child(
                div().text_xs().text_color(MUTED).child(
                    "Saved locally in SQLite. Read-only access. App passwords may be required.",
                ),
            )
    }
}

fn field_label(label: &'static str) -> gpui::Div {
    div()
        .text_xs()
        .font_weight(gpui::FontWeight::MEDIUM)
        .text_color(MUTED)
        .child(label)
}

fn non_empty(value: &str) -> Option<String> {
    (!value.is_empty()).then(|| value.to_owned())
}

fn parse_port(value: &str) -> Result<u16, &'static str> {
    value
        .parse::<u16>()
        .ok()
        .filter(|port| *port != 0)
        .ok_or("Enter a port from 1 to 65535.")
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;
    use gpui::{TestAppContext, VisualTestContext};
    use gpui_component::{Root, Theme, ThemeMode};

    #[gpui::test]
    async fn pasted_combined_credentials_connect_without_enter(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
            Theme::change(ThemeMode::Dark, None, cx);
        });
        let mut form = None;
        let window = cx.add_window(|window, cx| {
            let input = cx.new(|cx| Form::new(window, cx));
            form = Some(input.clone());
            Root::new(input, window, cx)
        });
        let form = form.unwrap();
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        visual.update(|window, cx| form.update(cx, |form, cx| form.focus(window, cx)));
        let mut events = visual.events(&form);
        visual.write_to_clipboard(gpui::ClipboardItem::new_string(
            "person@example.com:app:password".into(),
        ));
        visual.simulate_keystrokes("cmd-v");
        let submission = events.next().await.unwrap();
        assert_eq!(submission.email, "person@example.com");
        assert_eq!(submission.password, "app:password");
        assert_eq!(submission.port, 993);
        assert!(!submission.starttls);
    }

    #[gpui::test]
    async fn separate_fields_connect_on_enter(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
            Theme::change(ThemeMode::Dark, None, cx);
        });
        let mut form = None;
        let window = cx.add_window(|window, cx| {
            let input = cx.new(|cx| Form::new(window, cx));
            form = Some(input.clone());
            Root::new(input, window, cx)
        });
        let form = form.unwrap();
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        visual.update(|window, cx| form.update(cx, |form, cx| form.focus(window, cx)));
        let mut events = visual.events(&form);
        visual.simulate_input("person@example.com");
        visual.update(|window, cx| {
            let password = form.read(cx).password.clone();
            password.update(cx, |input, cx| input.focus(window, cx));
        });
        visual.simulate_input("separate:password");
        visual.simulate_keystrokes("enter");
        let submission = events.next().await.unwrap();
        assert_eq!(submission.email, "person@example.com");
        assert_eq!(submission.password, "separate:password");
    }

    #[gpui::test]
    async fn ordinary_password_paste_does_not_connect(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
            Theme::change(ThemeMode::Dark, None, cx);
        });
        let mut form = None;
        let window = cx.add_window(|window, cx| {
            let input = cx.new(|cx| Form::new(window, cx));
            form = Some(input.clone());
            Root::new(input, window, cx)
        });
        let form = form.unwrap();
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        visual.update(|window, cx| {
            let password = form.read(cx).password.clone();
            password.update(cx, |input, cx| input.focus(window, cx));
        });
        let mut events = visual.events(&form);
        visual.write_to_clipboard(gpui::ClipboardItem::new_string("password-only".into()));
        visual.simulate_keystrokes("cmd-v");
        visual.read(|cx| {
            assert_eq!(form.read(cx).password.read(cx).value(), "password-only");
        });
        assert!(events.try_recv().is_err());
    }

    #[test]
    fn rejects_invalid_server_ports() {
        for invalid in ["", "0", "65536", "-1", "smtp"] {
            assert!(parse_port(invalid).is_err(), "accepted {invalid}");
        }
        assert_eq!(parse_port("65535"), Ok(65535));
    }
}
