use fuckthunder_gui::MessageView;
use gpui::{div, prelude::*, px, rgb, ClipboardItem, Context, Window};
use gpui_component::{
    button::{Button, ButtonVariants},
    Selectable, Sizable,
};
use std::sync::Arc;
use tokio::runtime::Runtime;
const BG: u32 = 0x17191d;
const BORDER: u32 = 0x30343b;
const MUTED: u32 = 0x9ca4b1;

// WKWebView permits native evaluation while allowsContentJavaScript stays false.
// Only the body scale changes. Author widths, tables, and the DOM remain intact.
const WIDTH_FIT_SCRIPT: &str = r#"
(() => {
    let enabled = true;
    let root;
    let body;
    let originalZoom;
    let originalPriority;
    let pending = false;
    const restoreZoom = () => {
        if (originalZoom) body.style.setProperty('zoom', originalZoom, originalPriority);
        else body.style.removeProperty('zoom');
    };
    const fit = () => {
        pending = false;
        if (!enabled || !body) return;
        restoreZoom();
        const available = Math.min(window.innerWidth, root.clientWidth);
        if (available <= 0) return;
        let scale = Number.parseFloat(getComputedStyle(body).zoom) || 1;
        // Root scrollWidth measures visual overflow outside the zoomed body.
        // Recheck after scaling because percentage widths and margins can reflow.
        for (let pass = 0; pass < 4; pass++) {
            const width = root.scrollWidth;
            if (width <= available) break;
            scale *= available / width;
            body.style.setProperty('zoom', String(scale), 'important');
        }
    };
    const schedule = () => {
        if (enabled && body && !pending) {
            pending = true;
            requestAnimationFrame(fit);
        }
    };
    Object.defineProperty(window, '__hakusMailWidth', {
        value: (active) => {
            enabled = active;
            if (!body) return;
            if (enabled) fit();
            else restoreZoom();
        }
    });
    const initialize = () => {
        root = document.documentElement;
        body = document.body;
        if (!body) return;
        originalZoom = body.style.getPropertyValue('zoom');
        originalPriority = body.style.getPropertyPriority('zoom');
        const observer = new ResizeObserver(schedule);
        observer.observe(root);
        observer.observe(body);
        window.addEventListener('resize', schedule);
        document.addEventListener('load', schedule, true);
        document.addEventListener('error', schedule, true);
        schedule();
    };
    if (document.readyState === 'loading') {
        document.addEventListener('DOMContentLoaded', initialize, { once: true });
    } else initialize();
})();
"#;

pub(super) struct Viewer {
    runtime: Arc<Runtime>,
    detail: Option<Arc<MessageView>>,
    html_mode: bool,
    source_mode: bool,
    show_headers: bool,
    remote_images: bool,
    webview: Option<wry::WebView>,
    fit_width: bool,
    zoom: f64,
    status: String,
    error: Option<String>,
}
impl Viewer {
    pub(super) fn new(runtime: Arc<Runtime>, _: &mut Context<Self>) -> Self {
        Self {
            runtime,
            detail: None,
            html_mode: true,
            source_mode: false,
            show_headers: false,
            remote_images: false,
            webview: None,
            fit_width: true,
            zoom: 1.0,
            status: String::new(),
            error: None,
        }
    }
    pub(super) fn set_message(&mut self, message: Option<MessageView>, cx: &mut Context<Self>) {
        self.webview = None;
        self.detail = message.map(Arc::new);
        self.status.clear();
        self.error = None;
        self.show_headers = false;
        self.remote_images = false;
        self.source_mode = false;
        self.fit_width = true;
        self.zoom = 1.0;
        cx.notify();
    }
    fn detach_message(&self, message: Arc<MessageView>, cx: &mut gpui::App) -> anyhow::Result<()> {
        let runtime = self.runtime.clone();
        let html_mode = self.html_mode;
        let source_mode = self.source_mode;
        let show_headers = self.show_headers;
        let remote_images = self.remote_images;
        let fit_width = self.fit_width;
        let zoom = self.zoom;
        let bounds = gpui::Bounds::centered(None, gpui::size(px(860.), px(680.)), cx);
        cx.open_window(
            gpui::WindowOptions {
                window_bounds: Some(gpui::WindowBounds::Windowed(bounds)),
                window_min_size: Some(gpui::size(px(480.), px(420.))),
                ..Default::default()
            },
            move |window, cx| {
                window.set_window_title(&format!("{} — Hakus", message.subject));
                window.activate_window();
                let reader = cx.new(|cx| {
                    let mut viewer = Self::new(runtime, cx);
                    viewer.detail = Some(message);
                    viewer.html_mode = html_mode;
                    viewer.source_mode = source_mode;
                    viewer.show_headers = show_headers;
                    viewer.remote_images = remote_images;
                    viewer.fit_width = fit_width;
                    viewer.zoom = zoom;
                    viewer
                });
                let closing_reader = reader.downgrade();
                window.on_window_should_close(cx, move |_, cx| {
                    // Detach the native child before GPUI destroys its parent window.
                    let _ = closing_reader.update(cx, |viewer, _| viewer.webview = None);
                    true
                });
                cx.new(|cx| gpui_component::Root::new(reader, window, cx))
            },
        )?;
        Ok(())
    }

    fn message_html(&self) -> Option<String> {
        let message = self.detail.as_ref()?;
        if self.html_mode {
            Some(message.html_document(self.remote_images))
        } else {
            let text = if self.source_mode {
                String::from_utf8_lossy(&message.raw).into_owned()
            } else {
                message.body.clone()
            };
            let text = text
                .replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;");
            let font = if self.source_mode {
                "13px/1.6 ui-monospace,Menlo,monospace"
            } else {
                "14px/1.65 -apple-system,sans-serif"
            };
            Some(format!("<!doctype html><meta charset=\"utf-8\"><meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; style-src 'unsafe-inline'\"><style>body{{margin:0;padding:18px;color:#e7e9ee;background:#17191d;font:{font}}}pre{{white-space:pre-wrap;overflow-wrap:anywhere;font:inherit;margin:0}}</style><pre>{text}</pre>"))
        }
    }

    fn save_file(&mut self, name: String, data: Arc<[u8]>, cx: &mut Context<Self>) {
        let safe_name = std::path::Path::new(&name)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("attachment")
            .to_owned();
        let prompt = cx.prompt_for_new_path(&std::env::temp_dir(), Some(&safe_name));
        let executor = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let result = match prompt.await {
                Ok(Ok(Some(path))) => executor
                    .spawn(async move { std::fs::write(path, data) })
                    .await
                    .map_err(|error| error.to_string()),
                Ok(Ok(None)) => return,
                Ok(Err(error)) => Err(error.to_string()),
                Err(error) => Err(error.to_string()),
            };
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(()) => this.status = "File saved".into(),
                    Err(error) => this.error = Some(format!("Save failed: {error}")),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn open_attachment(&mut self, name: String, data: Arc<[u8]>, cx: &mut Context<Self>) {
        let runtime = self.runtime.clone();
        cx.spawn(async move |this, cx| {
            let result = runtime
                .spawn_blocking(move || -> std::io::Result<()> {
                    let directory = std::env::temp_dir()
                        .join(format!("hakus-attachment-{:016x}", rand::random::<u64>()));
                    std::fs::create_dir(&directory)?;
                    let filename = std::path::Path::new(&name)
                        .file_name()
                        .unwrap_or_else(|| std::ffi::OsStr::new("attachment"));
                    let path = directory.join(filename);
                    std::fs::write(&path, data)?;
                    std::process::Command::new("open").arg(&path).spawn()?;
                    Ok(())
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(Ok(())) => {
                        this.status = "Attachment opened with the default application".into()
                    }
                    Ok(Err(error)) => this.error = Some(error.to_string()),
                    Err(error) => this.error = Some(error.to_string()),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn save_all(&mut self, cx: &mut Context<Self>) {
        let Some(message) = &self.detail else { return };
        let files = message.attachments.clone();
        let prompt = cx.prompt_for_new_path(&std::env::temp_dir(), Some("message-attachments"));
        let executor = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let result = match prompt.await {
                Ok(Ok(Some(path))) => executor
                    .spawn(async move {
                        std::fs::create_dir(&path)?;
                        for (index, file) in files.iter().enumerate() {
                            let name = std::path::Path::new(&file.name)
                                .file_name()
                                .unwrap_or_else(|| std::ffi::OsStr::new("attachment"));
                            std::fs::write(
                                path.join(format!("{}-{}", index + 1, name.to_string_lossy())),
                                &file.data,
                            )?;
                        }
                        Ok(())
                    })
                    .await
                    .map_err(|error: std::io::Error| error.to_string()),
                Ok(Ok(None)) => return,
                Ok(Err(error)) => Err(error.to_string()),
                Err(error) => Err(error.to_string()),
            };
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(()) => this.status = "Attachments saved".into(),
                    Err(error) => this.error = Some(format!("Save attachments failed: {error}")),
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn set_html_mode(
        &mut self,
        enabled: bool,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.source_mode = false;
        self.error = None;
        self.html_mode = enabled;
        self.webview = None;
        cx.notify();
    }

    pub(super) fn hide_webview(&self) {
        if let Some(view) = &self.webview {
            let _ = view.set_visible(false);
        }
    }

    fn set_zoom(&mut self, zoom: f64, fit_width: bool, cx: &mut Context<Self>) {
        self.zoom = zoom.clamp(0.25, 3.0);
        self.fit_width = fit_width;
        if let Some(view) = &self.webview {
            let script = if fit_width {
                "window.__hakusMailWidth(true)"
            } else {
                "window.__hakusMailWidth(false)"
            };
            if let Err(error) = view
                .zoom(self.zoom)
                .and_then(|_| view.evaluate_script(script))
            {
                self.error = Some(format!("Message zoom unavailable: {error}"));
            }
        }
        cx.notify();
    }

    fn sync_webview(&mut self, bounds: gpui::Bounds<gpui::Pixels>, window: &mut Window) {
        use wry::{
            dpi::{LogicalPosition, LogicalSize},
            Rect, WebViewBuilder,
        };
        if self.detail.is_none() || bounds.size.width <= px(0.) || bounds.size.height <= px(0.) {
            self.hide_webview();
            return;
        }
        let rect = Rect {
            position: LogicalPosition::new(
                f64::from(f32::from(bounds.origin.x)),
                f64::from(f32::from(bounds.origin.y)),
            )
            .into(),
            size: LogicalSize::new(
                f64::from(f32::from(bounds.size.width)),
                f64::from(f32::from(bounds.size.height)),
            )
            .into(),
        };
        if self.webview.is_none() {
            let Some(html) = self.message_html() else {
                return;
            };
            match WebViewBuilder::new()
                .with_html(html)
                .with_javascript_disabled()
                .with_incognito(true)
                .with_bounds(rect)
                .with_navigation_handler(|url| {
                    if url == "about:blank" {
                        return true;
                    }
                    if let Ok(parsed) = url::Url::parse(&url) {
                        if matches!(parsed.scheme(), "https" | "http" | "mailto") {
                            let _ = std::process::Command::new("open").arg(&url).spawn();
                        }
                    }
                    false
                })
                .with_new_window_req_handler(|url, _| {
                    if let Ok(parsed) = url::Url::parse(&url) {
                        if matches!(parsed.scheme(), "https" | "http" | "mailto") {
                            let _ = std::process::Command::new("open").arg(&url).spawn();
                        }
                    }
                    wry::NewWindowResponse::Deny
                })
                .build_as_child(window)
            {
                Ok(view) => {
                    // Wry queues native evaluation until navigation commits on macOS.
                    let script = format!(
                        "{WIDTH_FIT_SCRIPT}\nwindow.__hakusMailWidth({});",
                        self.fit_width
                    );
                    if let Err(error) = view
                        .zoom(self.zoom)
                        .and_then(|_| view.evaluate_script(&script))
                    {
                        self.error = Some(format!("Message zoom unavailable: {error}"));
                    }
                    self.webview = Some(view);
                }
                Err(error) => {
                    self.error = Some(format!("HTML view unavailable: {error}"));
                    self.html_mode = false;
                }
            }
        } else if let Some(view) = &self.webview {
            if let Err(error) = view
                .set_bounds(rect)
                .and_then(|_| view.set_visible(true))
                .and_then(|_| {
                    if self.fit_width {
                        view.evaluate_script("window.__hakusMailWidth(true)")
                    } else {
                        Ok(())
                    }
                })
            {
                self.error = Some(format!("HTML view unavailable: {error}"));
            }
        }
    }
}
impl Render for Viewer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(message) = self.detail.clone() else {
            return div().size_full().bg(rgb(BG)).child("Select a message");
        };
        let controls = div()
            .flex()
            .flex_wrap()
            .gap_1()
            .items_center()
            .child(
                Button::new("text")
                    .label("Text")
                    .selected(!self.html_mode && !self.source_mode)
                    .xsmall()
                    .ghost()
                    .on_click(
                        cx.listener(|this, _, window, cx| this.set_html_mode(false, window, cx)),
                    ),
            )
            .child(
                Button::new("html")
                    .label("HTML")
                    .selected(self.html_mode)
                    .xsmall()
                    .ghost()
                    .on_click(
                        cx.listener(|this, _, window, cx| this.set_html_mode(true, window, cx)),
                    ),
            )
            .child(
                Button::new("fit-width")
                    .label("Fit")
                    .selected(self.fit_width)
                    .xsmall()
                    .ghost()
                    .on_click(cx.listener(|this, _, _, cx| this.set_zoom(1.0, true, cx))),
            )
            .child(
                Button::new("zoom-out")
                    .label("−")
                    .xsmall()
                    .ghost()
                    .on_click(
                        cx.listener(|this, _, _, cx| this.set_zoom(this.zoom / 1.25, false, cx)),
                    ),
            )
            .child(
                Button::new("zoom-reset")
                    .label(if self.fit_width {
                        "Actual size".to_owned()
                    } else {
                        format!("{:.0}%", self.zoom * 100.0)
                    })
                    .xsmall()
                    .ghost()
                    .on_click(cx.listener(|this, _, _, cx| this.set_zoom(1.0, false, cx))),
            )
            .child(
                Button::new("zoom-in").label("+").xsmall().ghost().on_click(
                    cx.listener(|this, _, _, cx| this.set_zoom(this.zoom * 1.25, false, cx)),
                ),
            )
            .child(
                Button::new("source")
                    .label("Source")
                    .selected(self.source_mode)
                    .xsmall()
                    .ghost()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.source_mode = true;
                        this.html_mode = false;
                        this.webview = None;
                        cx.notify();
                    })),
            )
            .child(
                Button::new("headers")
                    .label("Headers")
                    .xsmall()
                    .ghost()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.show_headers = !this.show_headers;
                        cx.notify();
                    })),
            )
            .child(
                Button::new("save")
                    .label("Save .eml")
                    .xsmall()
                    .ghost()
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(message) = &this.detail {
                            this.save_file("message.eml".into(), message.raw.clone(), cx);
                        }
                    })),
            )
            .child(
                Button::new("detach")
                    .label("Detach")
                    .xsmall()
                    .ghost()
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(message) = &this.detail {
                            if let Err(error) = this.detach_message(message.clone(), cx) {
                                this.error = Some(error.to_string());
                                cx.notify();
                            }
                        }
                    })),
            )
            .child(
                Button::new("copy")
                    .label("Copy")
                    .xsmall()
                    .ghost()
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(message) = &this.detail {
                            cx.write_to_clipboard(ClipboardItem::new_string(message.body.clone()));
                        }
                    })),
            );
        let header = div()
            .flex_shrink_0()
            .p_3()
            .flex()
            .flex_col()
            .gap_1()
            .border_b_1()
            .border_color(rgb(BORDER))
            .child(
                div()
                    .text_lg()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .line_clamp(2)
                    .child(message.subject.clone()),
            )
            .child(div().text_sm().child(format!("From: {}", message.from)))
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(MUTED))
                    .child(format!("To: {}", message.to)),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(MUTED))
                    .child(message.date.clone()),
            )
            .child(controls)
            .children(self.show_headers.then(|| {
                div()
                    .id("headers-scroll")
                    .max_h(px(120.))
                    .overflow_y_scroll()
                    .text_xs()
                    .children(
                        message
                            .headers
                            .iter()
                            .map(|(key, value)| div().child(format!("{key}: {value}"))),
                    )
            }));
        let attachments = div()
            .id("attachment-strip")
            .max_h(px(70.))
            .overflow_y_scroll()
            .flex_shrink_0()
            .px_3()
            .flex()
            .flex_wrap()
            .gap_1()
            .children(message.attachments.iter().enumerate().map(|(index, file)| {
                let name = file.name.clone();
                let data = file.data.clone();
                let open_name = name.clone();
                let open_data = data.clone();
                div()
                    .flex()
                    .items_center()
                    .child(
                        Button::new(("file", index))
                            .label(format!("↓ {}", name))
                            .xsmall()
                            .ghost()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.save_file(name.clone(), data.clone(), cx)
                            })),
                    )
                    .child(
                        Button::new(("open", index))
                            .label("Open")
                            .xsmall()
                            .ghost()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.open_attachment(open_name.clone(), open_data.clone(), cx)
                            })),
                    )
            }))
            .children((!message.attachments.is_empty()).then(|| {
                Button::new("save-all")
                    .label("Save all")
                    .xsmall()
                    .ghost()
                    .on_click(cx.listener(|this, _, _, cx| this.save_all(cx)))
            }))
            .children(message.html.is_some().then(|| {
                Button::new("images")
                    .label(if self.remote_images {
                        "Block external images"
                    } else {
                        "Load external images"
                    })
                    .xsmall()
                    .ghost()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.remote_images = !this.remote_images;
                        this.webview = None;
                        cx.notify();
                    }))
            }));
        let entity = cx.entity().clone();
        let surface = gpui::canvas(
            |_, _, _| (),
            move |bounds, _, window, cx| {
                entity.update(cx, |this, cx| {
                    let had_error = this.error.is_some();
                    this.sync_webview(bounds, window);
                    if !had_error && this.error.is_some() {
                        cx.notify();
                    }
                });
            },
        )
        .size_full();
        let _ = window;
        div()
            .size_full()
            .min_h_0()
            .min_w_0()
            .bg(rgb(BG))
            .text_color(rgb(0xe7e9ee))
            .flex()
            .flex_col()
            .child(header)
            .child(attachments)
            .children((!self.status.is_empty()).then(|| {
                div()
                    .px_3()
                    .text_xs()
                    .text_color(rgb(MUTED))
                    .child(self.status.clone())
            }))
            .children(
                self.error
                    .clone()
                    .map(|error| div().px_3().text_sm().child(error)),
            )
            .child(div().flex_1().min_h_0().min_w_0().child(surface))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Export must preserve the exact message bytes and cancellation must not write a file.
    #[gpui::test]
    async fn exports_exact_bytes_and_leaves_cancelled_destination_untouched(
        cx: &mut gpui::TestAppContext,
    ) {
        let runtime = Arc::new(Runtime::new().unwrap());
        let viewer = cx.new(|cx| Viewer::new(runtime, cx));
        let path =
            std::env::temp_dir().join(format!("hakus-export-{:016x}.eml", rand::random::<u64>()));
        let raw: Arc<[u8]> =
            Arc::from(&b"Subject: Test\r\nX-Trace: one\r\nX-Trace: two\r\n\r\nbody\0\xff"[..]);
        viewer.update(cx, |viewer, cx| {
            viewer.save_file("message.eml".into(), raw.clone(), cx)
        });
        cx.simulate_new_path_selection(|_| Some(path.clone()));
        cx.condition(&viewer, |viewer, _| {
            !viewer.status.is_empty() || viewer.error.is_some()
        })
        .await;
        assert_eq!(std::fs::read(&path).unwrap(), &*raw);
        viewer.update(cx, |viewer, cx| {
            viewer.status.clear();
            viewer.save_file("cancelled.eml".into(), Arc::from(&b"replacement"[..]), cx);
        });
        cx.simulate_new_path_selection(|_| None);
        cx.run_until_parked();
        assert_eq!(std::fs::read(&path).unwrap(), &*raw);
        std::fs::remove_file(path).unwrap();
    }
    #[gpui::test]
    async fn saves_duplicate_attachment_names_without_overwrite_or_path_escape(
        cx: &mut gpui::TestAppContext,
    ) {
        let runtime = Arc::new(Runtime::new().unwrap());
        let viewer = cx.new(|cx| Viewer::new(runtime, cx));
        let path = std::env::temp_dir().join(format!("hakus-files-{:016x}", rand::random::<u64>()));
        let files = [b"first".as_slice(), b"second".as_slice()]
            .into_iter()
            .map(|data| fuckthunder_gui::Attachment {
                name: "../../invoice.txt".into(),
                data: Arc::from(data),
                ..Default::default()
            })
            .collect();
        viewer.update(cx, |viewer, cx| {
            viewer.set_message(
                Some(MessageView {
                    attachments: files,
                    ..Default::default()
                }),
                cx,
            );
            viewer.save_all(cx);
        });
        cx.simulate_new_path_selection(|_| Some(path.clone()));
        cx.condition(&viewer, |viewer, _| {
            !viewer.status.is_empty() || viewer.error.is_some()
        })
        .await;
        assert_eq!(std::fs::read(path.join("1-invoice.txt")).unwrap(), b"first");
        assert_eq!(
            std::fs::read(path.join("2-invoice.txt")).unwrap(),
            b"second"
        );
        std::fs::remove_dir_all(path).unwrap();
    }
}
