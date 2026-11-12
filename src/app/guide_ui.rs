//! 快速指引：本地检索对话与全屏手把手向导。
//!
//! 页面本身只读 `rarebox_core::guide` 的静态内容；向导用 `BoundsHandle`
//! 采集真实控件位置，挖出可点击的洞，其余区域由四块遮罩挡住。这样用户
//! 既能看到真实界面，也只能操作当前步骤需要的部分。

use std::collections::HashMap;
use std::time::Duration;

use gpui::{
    AnyElement, App, Bounds, Context, Div, ElementId, Entity, FontWeight, Hsla, IntoElement,
    ParentElement, Pixels, SharedString, Stateful, StatefulInteractiveElement as _, Styled,
    WeakEntity, Window, div, prelude::*, px, svg,
};
use material3_gpui::icon::{Icon, IconName};
use material3_gpui::interaction::BoundsHandle;
use material3_gpui::prelude::*;
use material3_gpui::theme::ActiveTheme;

use rarebox_core::guide::{
    self, Guide, GuideAction, GuideCategory, GuideGate, GuidePage, GuideScope, GuideTarget,
    SearchHit, StuckHelp,
};

use super::shell_ui::page_scroll;
use super::{Page, RareBox, ToastKind};
use crate::guide_art;
use crate::ui::{self, ActionKind};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GuideRole {
    User,
    Assistant,
}

pub(crate) struct GuideMessage {
    pub(crate) role: GuideRole,
    pub(crate) text: SharedString,
    pub(crate) hits: Vec<SearchHit>,
    pub(crate) suggestions: Vec<&'static str>,
}

pub(crate) struct GuideSession {
    pub(crate) guide: &'static Guide,
    pub(crate) step: usize,
    pub(crate) checked: Vec<Vec<bool>>,
    pub(crate) stuck_open: bool,
    pub(crate) steps_open: bool,
    /// 已高亮导航、等待自动切页的目标。
    pub(crate) pending_page: Option<Page>,
    /// 防止过期的延时任务修改新会话。
    pub(crate) switch_token: u64,
}

impl Clone for GuideSession {
    fn clone(&self) -> Self {
        Self {
            guide: self.guide,
            step: self.step,
            checked: self.checked.clone(),
            stuck_open: self.stuck_open,
            steps_open: self.steps_open,
            pending_page: self.pending_page,
            switch_token: self.switch_token,
        }
    }
}

pub(crate) struct GuideUi {
    pub(crate) query: String,
    pub(crate) query_field: Entity<TextFieldState>,
    pub(crate) messages: Vec<GuideMessage>,
    pub(crate) session: Option<GuideSession>,
    /// 因缺少设备而先启动连接教程时，连接完成后要接着打开的教程。
    pub(crate) pending_guide: Option<&'static str>,
    pub(crate) anchors: GuideAnchors,
}

impl GuideUi {
    pub(crate) fn new(query_field: Entity<TextFieldState>) -> Self {
        Self {
            query: String::new(),
            query_field,
            messages: Vec::new(),
            session: None,
            pending_guide: None,
            anchors: GuideAnchors::new(),
        }
    }
}

/// 所有可高亮控件的位置。`capture_element()` 会持续把真实布局写回这里。
pub(crate) struct GuideAnchors {
    map: HashMap<GuideTarget, BoundsHandle>,
}

impl GuideAnchors {
    fn new() -> Self {
        let mut map = HashMap::new();
        for page in GuidePage::ALL {
            map.insert(GuideTarget::Nav(page), BoundsHandle::new());
        }
        for target in [
            GuideTarget::TopbarDevice,
            GuideTarget::ConnectTransportSwitch,
            GuideTarget::ConnectDeviceList,
            GuideTarget::ConnectPairCard,
            GuideTarget::ConnectDriverCard,
            GuideTarget::ConnectHdcCard,
            GuideTarget::AppsInstallButton,
            GuideTarget::AppsInstallPanel,
            GuideTarget::HarmonyAppsInstall,
            GuideTarget::ScreenDensityRow,
            GuideTarget::ScreenApplyDensity,
            GuideTarget::ScreenResetDensity,
            GuideTarget::ScreenScreenshot,
            GuideTarget::FilesList,
            GuideTarget::FilesUpload,
            GuideTarget::RemoteStart,
            GuideTarget::SettingsAdbPath,
            GuideTarget::SettingsSaveDir,
            GuideTarget::SettingsKillServer,
        ] {
            map.insert(target, BoundsHandle::new());
        }
        Self { map }
    }

    /// 取得采集句柄；所有会用到的 target 都在 `new` 里注册过。
    pub(crate) fn handle(&self, target: GuideTarget) -> BoundsHandle {
        self.map.get(&target).cloned().unwrap_or_default()
    }
}

/// 在控件外包一层相对定位容器，让 `capture_element()` 精确覆盖控件本身。
pub(crate) fn guide_anchor(handle: &BoundsHandle, content: impl IntoElement) -> Div {
    div()
        .relative()
        .child(content)
        .child(handle.capture_element())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OverlayMode {
    Step,
    Switching(Page),
    Suspended(Page),
}

const GUIDE_BODY: f32 = 14.;
const GUIDE_SMALL: f32 = 12.5;
const GUIDE_META: f32 = 11.5;
const GUIDE_TITLE: f32 = 16.;

#[derive(Clone, Copy)]
struct GuideFeature {
    label: &'static str,
    hint: &'static str,
    icon: &'static str,
    background: Hsla,
    foreground: Hsla,
    guide: &'static str,
}

fn guide_features(cx: &App, scope: GuideScope, windows: bool) -> Vec<GuideFeature> {
    let colors = *cx.theme().colors();
    let mut features = Vec::new();
    match scope {
        GuideScope::Android => {
            features.push(GuideFeature {
                label: "连接设备",
                hint: "USB / 无线配对",
                icon: "tabler-usb",
                background: colors.primary_container,
                foreground: colors.on_primary_container,
                guide: "connect_choose",
            });
            features.push(GuideFeature {
                label: "安装应用",
                hint: "APK / 分包安装",
                icon: "tabler-package",
                background: colors.secondary_container,
                foreground: colors.on_secondary_container,
                guide: "install_apk",
            });
            features.push(GuideFeature {
                label: "调整显示",
                hint: "字太小 / DPI",
                icon: "tabler-adjustments-horizontal",
                background: colors.tertiary_container,
                foreground: colors.on_tertiary_container,
                guide: "dpi",
            });
            features.push(GuideFeature {
                label: "截图录屏",
                hint: "保存到电脑",
                icon: "tabler-camera",
                background: colors.primary_container,
                foreground: colors.on_primary_container,
                guide: "screenshot",
            });
            features.push(GuideFeature {
                label: "传文件",
                hint: "上传 / 下载",
                icon: "tabler-folder",
                background: colors.secondary_container,
                foreground: colors.on_secondary_container,
                guide: "files",
            });
            features.push(GuideFeature {
                label: "远程投屏",
                hint: "电脑操作屏幕",
                icon: "tabler-cast",
                background: colors.tertiary_container,
                foreground: colors.on_tertiary_container,
                guide: "remote",
            });
            features.push(GuideFeature {
                label: "连不上？",
                hint: "未授权 / 驱动 / 离线",
                icon: "tabler-alert-triangle",
                background: colors.error_container,
                foreground: colors.on_error_container,
                guide: "not_connecting",
            });
            if windows {
                features.push(GuideFeature {
                    label: "USB 驱动",
                    hint: "Windows 认不出",
                    icon: "tabler-brand-windows",
                    background: colors.surface_container_highest,
                    foreground: colors.on_surface,
                    guide: "driver",
                });
            }
        }
        GuideScope::Harmony => {
            features.push(GuideFeature {
                label: "连接设备",
                hint: "hdc / 无线连接",
                icon: "tabler-usb",
                background: colors.primary_container,
                foreground: colors.on_primary_container,
                guide: "harmony_connect",
            });
            features.push(GuideFeature {
                label: "安装 HAP",
                hint: "HAP / HSP / APP",
                icon: "tabler-package",
                background: colors.secondary_container,
                foreground: colors.on_secondary_container,
                guide: "install_hap",
            });
            features.push(GuideFeature {
                label: "截图录屏",
                hint: "保存到电脑",
                icon: "tabler-camera",
                background: colors.tertiary_container,
                foreground: colors.on_tertiary_container,
                guide: "screenshot",
            });
            features.push(GuideFeature {
                label: "传文件",
                hint: "上传 / 下载",
                icon: "tabler-folder",
                background: colors.primary_container,
                foreground: colors.on_primary_container,
                guide: "files",
            });
            features.push(GuideFeature {
                label: "签名安装",
                hint: "证书 / Profile",
                icon: "tabler-certificate",
                background: colors.secondary_container,
                foreground: colors.on_secondary_container,
                guide: "op_signing",
            });
            features.push(GuideFeature {
                label: "连不上？",
                hint: "hdc / 未授权 / 离线",
                icon: "tabler-alert-triangle",
                background: colors.error_container,
                foreground: colors.on_error_container,
                guide: "not_connecting",
            });
        }
    }
    features
}

fn feature_tile(
    weak: WeakEntity<RareBox>,
    _cx: &App,
    feature: GuideFeature,
    index: usize,
) -> AnyElement {
    let hover = feature.background.opacity(0.82);
    div()
        .id(ElementId::Name(format!("guide-feature-{index}").into()))
        .w(px(164.))
        .h(px(122.))
        .flex_none()
        .rounded(px(18.))
        .bg(feature.background)
        .p(px(14.))
        .flex()
        .flex_col()
        .justify_between()
        .gap(px(8.))
        .cursor_pointer()
        .hover(move |style| style.bg(hover))
        .on_click(move |_, _, cx| {
            if let Err(err) = weak.update(cx, |app, cx| app.guide_start(feature.guide, cx)) {
                eprintln!("RareBox Desktop: 启动指引功能失败: {err}");
            }
        })
        .child(
            div()
                .size(px(42.))
                .rounded(px(12.))
                .bg(feature.foreground.opacity(0.12))
                .flex()
                .items_center()
                .justify_center()
                .child(
                    Icon::new(IconName::new(feature.icon))
                        .size(px(26.))
                        .color(feature.foreground),
                ),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(
                    div()
                        .text_size(px(15.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(feature.foreground)
                        .child(feature.label),
                )
                .child(
                    div()
                        .text_size(px(GUIDE_META))
                        .line_height(px(15.))
                        .text_color(feature.foreground.opacity(0.78))
                        .child(feature.hint),
                ),
        )
        .into_any_element()
}

fn guide_tag(label: impl Into<SharedString>, background: Hsla, foreground: Hsla) -> Div {
    div()
        .flex_none()
        .px(px(9.))
        .h(px(24.))
        .rounded(px(7.))
        .bg(background)
        .text_size(px(12.5))
        .font_weight(FontWeight::MEDIUM)
        .text_color(foreground)
        .flex()
        .items_center()
        .child(label.into())
}

fn category_tabler_icon(category: GuideCategory) -> &'static str {
    match category {
        GuideCategory::Connect => "tabler-usb",
        GuideCategory::Install => "tabler-package",
        GuideCategory::Display => "tabler-adjustments-horizontal",
        GuideCategory::Files => "tabler-folder",
        GuideCategory::Advanced => "tabler-bolt",
        GuideCategory::Troubleshoot => "tabler-alert-triangle",
    }
}

impl RareBox {
    // ---------------------------------------------------------------------
    // 页面
    // ---------------------------------------------------------------------

    pub(crate) fn render_guide(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let content = self.render_guide_content(cx);
        page_scroll(
            Page::Guide,
            &self.scroll_handles[Page::Guide as usize],
            content,
        )
        .into_any_element()
    }

    fn render_guide_content(&mut self, cx: &mut Context<Self>) -> Div {
        let colors = *cx.theme().colors();
        let scope = self.guide_scope();
        let windows = cfg!(target_os = "windows");
        let weak = cx.entity().downgrade();
        let device_ready = self.guide_gate_ready(GuideGate::DeviceSelected);

        let mut suggestions: Vec<AnyElement> = Vec::new();
        if device_ready {
            for (index, suggestion) in guide::suggestions(scope, windows).into_iter().enumerate() {
                let id = ElementId::Name(format!("guide-suggestion-{index}").into());
                let weak = weak.clone();
                suggestions.push(
                    ui::filter_chip(cx, id, suggestion, false, move |_, _, cx| {
                        if let Err(err) = weak.update(cx, |app, cx| {
                            app.guide_ask(suggestion.to_string(), cx);
                        }) {
                            eprintln!("RareBox Desktop: 发送指引问题失败: {err}");
                        }
                    })
                    .into_any_element(),
                );
            }
        }

        let tiles: Vec<AnyElement> = if device_ready {
            guide_features(cx, scope, windows)
                .into_iter()
                .enumerate()
                .map(|(index, feature)| feature_tile(weak.clone(), cx, feature, index))
                .collect()
        } else {
            Vec::new()
        };

        let can_send = !self.guide.query.trim().is_empty();
        let send = ui::action_button_sized(
            cx,
            "guide-send",
            "发送",
            Some("arrow_upward"),
            ActionKind::Filled,
            can_send,
            44.,
            14.5,
            18.,
            cx.listener(|app, _, _, cx| app.guide_ask_current(cx)),
        );
        let input_card = Card::new()
            .outlined()
            .p(px(12.))
            .child(
                div()
                    .flex()
                    .items_end()
                    .gap(px(8.))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .child(self.guide.query_field.clone()),
                    )
                    .child(send),
            )
            .child(
                div()
                    .mt(px(8.))
                    .text_size(px(GUIDE_SMALL))
                    .text_color(colors.on_surface_variant)
                    .child("按 Enter 发送 · 检索只匹配内置教程，不会编造步骤"),
            );

        let feature_area = if device_ready {
            div()
                .mt(px(18.))
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(
                    div()
                        .text_size(px(GUIDE_SMALL))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(colors.on_surface_variant)
                        .child("常用操作"),
                )
                .child(div().flex().flex_wrap().gap(px(10.)).children(tiles))
        } else {
            div().mt(px(18.)).child(self.render_connect_prompt(cx))
        };
        let hero_title = if device_ready {
            "想做什么？点一下就好"
        } else {
            "先连接设备"
        };
        let hero_subtitle = if device_ready {
            "选一个色块直接开始，或在下面输入你的问题。本地图文教程，不联网；遮罩会高亮真实按钮，只让你操作当前一步。"
        } else {
            "连接后才能安装应用、调 DPI、截图和传文件。先跟着连接教程走，设备出现后其他操作会自动显示。"
        };

        let hero = Card::new()
            .outlined()
            .p(px(22.))
            .child(
                div()
                    .flex()
                    .items_start()
                    .gap(px(16.))
                    .child(
                        div()
                            .size(px(56.))
                            .flex_none()
                            .rounded(px(18.))
                            .bg(colors.primary_container)
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                Icon::new(IconName::new("tabler-message-chatbot"))
                                    .size(px(30.))
                                    .color(colors.on_primary_container),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(6.))
                            .flex_1()
                            .min_w(px(0.))
                            .child(
                                div()
                                    .text_size(px(30.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(hero_title),
                            )
                            .child(
                                div()
                                    .text_size(px(15.))
                                    .line_height(px(22.))
                                    .text_color(colors.on_surface_variant)
                                    .child(hero_subtitle),
                            ),
                    ),
            )
            .child(div().mt(px(16.)).child(input_card))
            .child(feature_area)
            .when(device_ready, |el| {
                el.child(
                    div()
                        .mt(px(16.))
                        .flex()
                        .flex_col()
                        .gap(px(7.))
                        .child(
                            div()
                                .text_size(px(GUIDE_SMALL))
                                .text_color(colors.on_surface_variant)
                                .child("也可以直接问"),
                        )
                        .child(div().flex().flex_wrap().gap(px(8.)).children(suggestions)),
                )
            });

        let greeting = if device_ready {
            "直接描述问题就行，例如「手表字太小」「安装 APK 失败」「无线配对连不上」。我会先找出内置的操作步骤，再带你一步步完成。"
        } else {
            "还没有连接设备。先点上面的「开始连接教程」；连接完成后，安装、DPI、截图等操作会自动出现。"
        };

        let mut message_views: Vec<AnyElement> = Vec::new();
        if self.guide.messages.is_empty() {
            message_views.push(
                div()
                    .flex()
                    .items_start()
                    .gap(px(10.))
                    .child(assistant_avatar(cx))
                    .child(
                        div()
                            .max_w(px(620.))
                            .px(px(14.))
                            .py(px(11.))
                            .rounded(px(14.))
                            .rounded_tl(px(4.))
                            .bg(colors.surface_container_low)
                            .text_size(px(GUIDE_BODY))
                            .line_height(px(21.))
                            .text_color(colors.on_surface)
                            .child(greeting),
                    )
                    .into_any_element(),
            );
        }
        for (message_index, message) in self.guide.messages.iter().enumerate() {
            match message.role {
                GuideRole::User => message_views.push(
                    div()
                        .flex()
                        .justify_end()
                        .child(
                            div()
                                .max_w(px(560.))
                                .px(px(14.))
                                .py(px(11.))
                                .rounded(px(14.))
                                .rounded_tr(px(4.))
                                .bg(colors.primary_container)
                                .text_size(px(GUIDE_BODY))
                                .line_height(px(21.))
                                .text_color(colors.on_primary_container)
                                .child(message.text.clone()),
                        )
                        .into_any_element(),
                ),
                GuideRole::Assistant => {
                    let mut block = div()
                        .flex()
                        .items_start()
                        .gap(px(10.))
                        .child(assistant_avatar(cx))
                        .child(
                            div()
                                .max_w(px(680.))
                                .px(px(14.))
                                .py(px(11.))
                                .rounded(px(14.))
                                .rounded_tl(px(4.))
                                .bg(colors.surface_container_low)
                                .text_size(px(GUIDE_BODY))
                                .line_height(px(21.))
                                .text_color(colors.on_surface)
                                .child(message.text.clone()),
                        );
                    if !message.hits.is_empty() {
                        let cards: Vec<AnyElement> = message
                            .hits
                            .iter()
                            .enumerate()
                            .map(|(hit_index, hit)| {
                                self.render_guide_hit(cx, message_index, hit_index, hit)
                            })
                            .collect();
                        block = block.child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(10.))
                                .flex_1()
                                .min_w(px(0.))
                                .children(cards),
                        );
                    }
                    if !message.suggestions.is_empty() {
                        let chips: Vec<AnyElement> = message
                            .suggestions
                            .iter()
                            .enumerate()
                            .map(|(chip_index, suggestion)| {
                                let suggestion = *suggestion;
                                let weak = weak.clone();
                                ui::filter_chip(
                                    cx,
                                    ElementId::Name(
                                        format!("guide-miss-{message_index}-{chip_index}").into(),
                                    ),
                                    suggestion,
                                    false,
                                    move |_, _, cx| {
                                        if let Err(err) = weak.update(cx, |app, cx| {
                                            app.guide_ask(suggestion.to_string(), cx);
                                        }) {
                                            eprintln!("RareBox Desktop: 发送建议问题失败: {err}");
                                        }
                                    },
                                )
                                .into_any_element()
                            })
                            .collect();
                        block = block.child(
                            div()
                                .mt(px(4.))
                                .flex()
                                .flex_col()
                                .gap(px(6.))
                                .child(
                                    div()
                                        .text_size(px(11.))
                                        .text_color(colors.on_surface_variant)
                                        .child("换个说法试试："),
                                )
                                .child(div().flex().flex_wrap().gap(px(6.)).children(chips)),
                        );
                    }
                    message_views.push(block.into_any_element());
                }
            }
        }

        let thread = Card::new()
            .outlined()
            .p(px(16.))
            .child(div().flex().flex_col().gap(px(14.)).children(message_views));

        let chat = div()
            .flex_1()
            .min_w(px(0.))
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(thread);

        let catalog = self.render_guide_catalog(cx);
        let mobile = cfg!(any(target_os = "android", target_os = "ios"));
        let columns = if mobile {
            div()
                .w_full()
                .flex()
                .flex_col()
                .gap(px(14.))
                .child(chat)
                .child(catalog)
        } else {
            div()
                .w_full()
                .flex()
                .items_start()
                .gap(px(18.))
                .child(chat)
                .child(div().w(px(316.)).flex_none().child(catalog))
        };

        div()
            .w_full()
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(hero)
            .child(columns)
    }

    fn render_guide_hit(
        &self,
        cx: &mut Context<Self>,
        message_index: usize,
        hit_index: usize,
        hit: &SearchHit,
    ) -> AnyElement {
        let colors = *cx.theme().colors();
        let guide = hit.guide;
        let label = if guide.tutorial { "开始" } else { "打开" };
        let button = ui::action_button_sized(
            cx,
            ElementId::Name(
                format!("guide-hit-{}-{}-{}", message_index, hit_index, guide.id).into(),
            ),
            label,
            Some(if guide.tutorial {
                "play_arrow"
            } else {
                "arrow_upward"
            }),
            ActionKind::Filled,
            true,
            44.,
            14.5,
            18.,
            {
                let guide_id = guide.id;
                cx.listener(move |app, _, _, cx| app.guide_start(guide_id, cx))
            },
        );

        let mut chips = vec![
            guide_tag(
                platform_label(guide),
                colors.secondary_container,
                colors.on_secondary_container,
            )
            .into_any_element(),
            guide_tag(
                guide.duration,
                colors.surface_container_high,
                colors.on_surface_variant,
            )
            .into_any_element(),
        ];
        if guide.tutorial {
            chips.push(
                guide_tag(
                    format!("{} 步", guide.steps.len()),
                    colors.surface_container_high,
                    colors.on_surface_variant,
                )
                .into_any_element(),
            );
        }

        Card::new()
            .outlined()
            .p(px(14.))
            .child(
                div()
                    .flex()
                    .items_start()
                    .gap(px(12.))
                    .child(
                        div()
                            .size(px(46.))
                            .flex_none()
                            .rounded(px(14.))
                            .bg(colors.secondary_container)
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                Icon::new(IconName::new(category_tabler_icon(guide.category)))
                                    .size(px(24.))
                                    .color(colors.on_secondary_container),
                            ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .child(
                                div()
                                    .text_size(px(GUIDE_TITLE))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(guide.title),
                            )
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .line_height(px(17.))
                                    .text_color(colors.on_surface_variant)
                                    .child(guide.summary),
                            )
                            .child(
                                div()
                                    .mt(px(2.))
                                    .flex()
                                    .flex_wrap()
                                    .gap(px(6.))
                                    .children(chips),
                            )
                            .when(!hit.matched.is_empty(), |el| {
                                el.child(
                                    div()
                                        .text_size(px(12.))
                                        .text_color(colors.primary)
                                        .child(format!("命中：{}", hit.matched.join(" / "))),
                                )
                            }),
                    )
                    .child(div().flex_none().child(button)),
            )
            .into_any_element()
    }

    /// 没有设备时的首屏：只保留连接入口，不显示其他功能色块。
    fn render_connect_prompt(&self, cx: &mut Context<Self>) -> Div {
        let colors = *cx.theme().colors();
        let harmony = self.is_harmony();
        let windows = cfg!(target_os = "windows");
        let connect_id = self.guide_connect_target();

        let start = ui::action_button_sized(
            cx,
            "guide-prompt-connect",
            "开始连接教程",
            Some("tabler-usb"),
            ActionKind::Filled,
            true,
            48.,
            15.,
            20.,
            cx.listener(move |app, _, _, cx| app.guide_start(connect_id, cx)),
        );
        let refresh = ui::action_button_sized(
            cx,
            "guide-prompt-refresh",
            if harmony {
                "重新检测 hdc"
            } else {
                "重新检测设备"
            },
            Some("tabler-refresh"),
            ActionKind::Outlined,
            true,
            44.,
            13.5,
            18.,
            cx.listener(move |app, _, _, cx| {
                if harmony {
                    app.refresh_hdc(cx);
                } else {
                    app.refresh_devices(cx);
                }
            }),
        );

        let mut actions = vec![start.into_any_element(), refresh.into_any_element()];
        if !harmony {
            actions.push(
                ui::action_button(
                    cx,
                    "guide-prompt-wireless",
                    "无线配对",
                    Some("tabler-wifi"),
                    ActionKind::Text,
                    true,
                    cx.listener(|app, _, _, cx| app.guide_start("connect_wireless", cx)),
                )
                .into_any_element(),
            );
        }
        if windows && !harmony {
            actions.push(
                ui::action_button(
                    cx,
                    "guide-prompt-driver",
                    "USB 驱动",
                    Some("tabler-brand-windows"),
                    ActionKind::Text,
                    true,
                    cx.listener(|app, _, _, cx| app.guide_start("driver", cx)),
                )
                .into_any_element(),
            );
        }

        div()
            .w_full()
            .rounded(px(20.))
            .bg(colors.primary_container)
            .p(px(20.))
            .flex()
            .flex_col()
            .gap(px(14.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(14.))
                    .child(
                        div()
                            .size(px(54.))
                            .flex_none()
                            .rounded(px(16.))
                            .bg(colors.on_primary_container.opacity(0.12))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                Icon::new(IconName::new("tabler-usb"))
                                    .size(px(30.))
                                    .color(colors.on_primary_container),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .flex_1()
                            .min_w(px(0.))
                            .child(
                                div()
                                    .text_size(px(18.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(colors.on_primary_container)
                                    .child("还没有检测到设备"),
                            )
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .line_height(px(19.))
                                    .text_color(colors.on_primary_container.opacity(0.8))
                                    .child(if harmony {
                                        "先连接 HarmonyOS 设备，之后 HAP 安装、截图和文件传输会自动出现。"
                                    } else {
                                        "先连接手表或手机，之后安装应用、调整 DPI、截图和传文件会自动出现。"
                                    }),
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(8.))
                    .children(actions),
            )
            .child(
                div()
                    .text_size(px(11.5))
                    .line_height(px(17.))
                    .text_color(colors.on_primary_container.opacity(0.72))
                    .child(if harmony {
                        "教程顺序：开启开发者模式 → 切换到 HarmonyOS → 在设备上允许调试"
                    } else {
                        "教程顺序：判断是否支持无线调试 → 选择 USB 或无线 → 按提示配对或连接"
                    }),
            )
    }

    fn render_guide_catalog(&self, cx: &mut Context<Self>) -> Div {
        let colors = *cx.theme().colors();
        let scope = self.guide_scope();
        let windows = cfg!(target_os = "windows");
        let weak = cx.entity().downgrade();
        let device_ready = self.guide_gate_ready(GuideGate::DeviceSelected);

        let mut groups: Vec<AnyElement> = Vec::new();
        for category in GuideCategory::ALL {
            if !device_ready
                && !matches!(
                    category,
                    GuideCategory::Connect | GuideCategory::Troubleshoot
                )
            {
                continue;
            }
            let items: Vec<&'static Guide> = guide::tutorials(scope, windows)
                .filter(|guide| guide.category == category)
                .collect();
            if items.is_empty() {
                continue;
            }
            let rows: Vec<AnyElement> = items
                .iter()
                .map(|guide| {
                    let guide = *guide;
                    let weak = weak.clone();
                    div()
                        .id(ElementId::Name(
                            format!("guide-catalog-{}", guide.id).into(),
                        ))
                        .px(px(10.))
                        .py(px(9.))
                        .rounded(px(10.))
                        .flex()
                        .items_start()
                        .gap(px(9.))
                        .cursor_pointer()
                        .hover(move |style| style.bg(colors.surface_container_high))
                        .on_click(move |_, _, cx| {
                            if let Err(err) =
                                weak.update(cx, |app, cx| app.guide_start(guide.id, cx))
                            {
                                eprintln!("RareBox Desktop: 启动教程失败: {err}");
                            }
                        })
                        .child(
                            Icon::new(IconName::new(category_tabler_icon(category))).size(px(19.)),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .flex()
                                .flex_col()
                                .gap(px(2.))
                                .child(
                                    div()
                                        .text_size(px(13.5))
                                        .font_weight(FontWeight::MEDIUM)
                                        .child(guide.title),
                                )
                                .child(
                                    div()
                                        .text_size(px(GUIDE_META))
                                        .text_color(colors.on_surface_variant)
                                        .child(format!(
                                            "{} · {}",
                                            guide.duration,
                                            platform_label(guide)
                                        )),
                                ),
                        )
                        .child(
                            Icon::new(IconName::new("chevron_right"))
                                .size(px(15.))
                                .color(colors.on_surface_variant),
                        )
                        .into_any_element()
                })
                .collect();
            groups.push(
                div()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .px(px(10.))
                            .pt(px(10.))
                            .pb(px(4.))
                            .text_size(px(GUIDE_SMALL))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(colors.on_surface_variant)
                            .child(category.label()),
                    )
                    .children(rows)
                    .into_any_element(),
            );
        }

        let quick: Vec<AnyElement> = if device_ready {
            guide::quick_operations(scope, windows)
                .map(|guide| {
                    let id = ElementId::Name(format!("guide-quick-{}", guide.id).into());
                    let guide_id = guide.id;
                    let weak = weak.clone();
                    ui::filter_chip(cx, id, guide.title, false, move |_, _, cx| {
                        if let Err(err) = weak.update(cx, |app, cx| {
                            app.guide_start(guide_id, cx);
                        }) {
                            eprintln!("RareBox Desktop: 启动快捷操作失败: {err}");
                        }
                    })
                    .into_any_element()
                })
                .collect()
        } else {
            Vec::new()
        };

        div().child(
            Card::new()
                .outlined()
                .p(px(10.))
                .child(
                    div()
                        .px(px(10.))
                        .py(px(8.))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(
                            Icon::new(IconName::new("tabler-list-check"))
                                .size(px(18.))
                                .color(colors.primary),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .child(
                                    div()
                                        .text_size(px(15.))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child("操作手册"),
                                )
                                .child(
                                    div()
                                        .text_size(px(GUIDE_SMALL))
                                        .text_color(colors.on_surface_variant)
                                        .child(if device_ready {
                                            "按分类浏览，或直接搜索"
                                        } else {
                                            "先看连接与排查"
                                        }),
                                ),
                        ),
                )
                .children(groups)
                .when(!quick.is_empty(), |el| {
                    el.child(
                        div()
                            .mt(px(6.))
                            .pt(px(10.))
                            .border_t_1()
                            .border_color(colors.outline_variant.opacity(0.45))
                            .child(
                                div()
                                    .px(px(10.))
                                    .pb(px(6.))
                                    .text_size(px(GUIDE_SMALL))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(colors.on_surface_variant)
                                    .child("快捷操作"),
                            )
                            .child(
                                div()
                                    .px(px(10.))
                                    .pb(px(6.))
                                    .flex()
                                    .flex_wrap()
                                    .gap(px(6.))
                                    .children(quick),
                            ),
                    )
                }),
        )
    }

    // ---------------------------------------------------------------------
    // 对话动作
    // ---------------------------------------------------------------------

    pub(crate) fn guide_scope(&self) -> GuideScope {
        if self.is_harmony() {
            GuideScope::Harmony
        } else {
            GuideScope::Android
        }
    }

    pub(crate) fn guide_ask_current(&mut self, cx: &mut Context<Self>) {
        let query = self.guide.query.clone();
        self.guide_ask(query, cx);
    }

    pub(crate) fn guide_ask(&mut self, query: String, cx: &mut Context<Self>) {
        let query = query.trim().to_string();
        if query.is_empty() {
            return;
        }
        let scope = self.guide_scope();
        let windows = cfg!(target_os = "windows");
        let hits = guide::search(&query, scope, windows);
        let (text, suggestions) = if hits.is_empty() {
            (
                format!("没有找到「{query}」对应的内置教程。可以换个说法，或从右侧手册目录选择。"),
                guide::suggestions(scope, windows)
                    .into_iter()
                    .take(4)
                    .collect(),
            )
        } else if hits.len() == 1 {
            ("找到 1 个可以直接跟着做的操作。".to_string(), Vec::new())
        } else {
            (
                format!("找到 {} 个相关操作，按匹配度排序。", hits.len()),
                Vec::new(),
            )
        };

        self.guide.messages.push(GuideMessage {
            role: GuideRole::User,
            text: query.clone().into(),
            hits: Vec::new(),
            suggestions: Vec::new(),
        });
        self.guide.messages.push(GuideMessage {
            role: GuideRole::Assistant,
            text: text.into(),
            hits,
            suggestions,
        });
        self.guide.query.clear();
        // 点击“发送”时输入框通常仍处于焦点，中文输入法也可能还有组合态。
        // 把清空和滚动推迟到当前点击事件之后，避免在 IME/焦点处理中重入。
        let field = self.guide.query_field.clone();
        let scroll = self.scroll_handles[Page::Guide as usize].clone();
        let weak = cx.entity().downgrade();
        cx.defer(move |cx| {
            // 先结束输入法组合态/焦点，再改输入框内容，避免点击“发送”时重入。
            if let Some(handle) = cx.active_window() {
                let _ = handle.update(cx, |_, window, cx| window.blur(cx));
            }
            let _ = field.update(cx, |state, cx| state.set_value("", cx));
            scroll.scroll_to_bottom();
            if let Err(err) = weak.update(cx, |_, cx| cx.notify()) {
                eprintln!("RareBox Desktop: 刷新指引对话失败: {err}");
            }
        });
        cx.notify();
    }

    pub(crate) fn guide_start(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(active) = guide::find(id) else {
            return;
        };
        if !active
            .platforms
            .supports(self.guide_scope(), cfg!(target_os = "windows"))
        {
            self.toast(cx, ToastKind::Error, "当前工作区不能使用这个教程");
            return;
        }
        // 没有设备时，先走连接教程；完成连接后自动回到用户本来想做的事。
        if active.requires_device() && !self.guide_gate_ready(GuideGate::DeviceSelected) {
            let connect_id = self.guide_connect_target();
            self.guide.pending_guide = Some(active.id);
            self.start_guide_internal(connect_id, cx);
            self.toast(
                cx,
                ToastKind::Info,
                format!("先完成连接，之后自动继续「{}」", active.title),
            );
            return;
        }
        self.guide.pending_guide = None;
        self.start_guide_internal(active.id, cx);
    }

    fn start_guide_internal(&mut self, id: &'static str, cx: &mut Context<Self>) {
        let Some(active) = guide::find(id) else {
            return;
        };
        let checked = active
            .steps
            .iter()
            .map(|step| vec![false; step.checklist.len()])
            .collect();
        let token = self
            .guide
            .session
            .as_ref()
            .map(|session| session.switch_token.wrapping_add(1))
            .unwrap_or(1);
        self.guide.session = Some(GuideSession {
            guide: active,
            step: 0,
            checked,
            stuck_open: false,
            steps_open: false,
            pending_page: None,
            switch_token: token,
        });
        self.guide_enter_step(cx);
    }

    /// 分支步骤：保留连接完成后的待办教程，直接进入用户选择的那一条。
    pub(crate) fn guide_choose(&mut self, id: &'static str, cx: &mut Context<Self>) {
        let Some(active) = guide::find(id) else {
            return;
        };
        if !active
            .platforms
            .supports(self.guide_scope(), cfg!(target_os = "windows"))
        {
            self.toast(cx, ToastKind::Error, "当前工作区不能使用这个教程");
            return;
        }
        self.start_guide_internal(id, cx);
    }

    /// 当前工作区用来解决“没有设备”的教程。
    ///
    /// 已探测过手表能力时直接进对应教程，不再让用户从三个入口里猜。
    pub(crate) fn guide_connect_target(&self) -> &'static str {
        if self.is_harmony() {
            return "harmony_connect";
        }
        self.snapshot
            .as_ref()
            .map(|snapshot| snapshot.debug_support().guide_id())
            .unwrap_or("connect_choose")
    }

    fn is_connection_guide(id: &str) -> bool {
        matches!(
            id,
            "connect_choose"
                | "connect_usb"
                | "connect_wireless"
                | "connect_wlan"
                | "harmony_connect"
                | "driver"
                | "not_connecting"
        )
    }

    fn guide_enter_step(&mut self, cx: &mut Context<Self>) {
        let Some(session) = self.guide.session.as_mut() else {
            return;
        };
        session.stuck_open = false;
        let target = session
            .guide
            .steps
            .get(session.step)
            .and_then(|step| step.page)
            .map(guide_page_to_ui);
        let page = target.filter(|page| *page != self.page);
        if let Some(page) = page {
            session.pending_page = Some(page);
            session.switch_token = session.switch_token.wrapping_add(1);
            let token = session.switch_token;
            self.spawn_guide_switch(token, cx);
        } else {
            session.pending_page = None;
        }
        self.guide_refresh_soon(cx);
        cx.notify();
    }

    fn spawn_guide_switch(&self, token: u64, cx: &mut Context<Self>) {
        let weak = cx.entity().downgrade();
        let executor = cx.background_executor().clone();
        cx.spawn(async move |_, cx| {
            executor.timer(Duration::from_millis(900)).await;
            if let Err(err) = weak.update(cx, |app, cx| app.guide_finish_switch(token, cx)) {
                eprintln!("RareBox Desktop: 完成向导切页失败: {err}");
            }
        })
        .detach();
    }

    fn guide_finish_switch(&mut self, token: u64, cx: &mut Context<Self>) {
        let Some(session) = self.guide.session.as_mut() else {
            return;
        };
        if session.switch_token != token {
            return;
        }
        let Some(page) = session.pending_page.take() else {
            return;
        };
        self.set_page(page, cx);
        self.guide_refresh_soon(cx);
    }

    /// 新页面刚渲染时锚点还是旧值，稍后再通知一帧让高亮落到正确位置。
    fn guide_refresh_soon(&self, cx: &mut Context<Self>) {
        let weak = cx.entity().downgrade();
        let executor = cx.background_executor().clone();
        cx.spawn(async move |_, cx| {
            executor.timer(Duration::from_millis(140)).await;
            if let Err(err) = weak.update(cx, |_, cx| cx.notify()) {
                eprintln!("RareBox Desktop: 刷新向导高亮失败: {err}");
            }
        })
        .detach();
    }

    pub(crate) fn guide_next(&mut self, cx: &mut Context<Self>) {
        let Some(session) = self.guide.session.as_ref() else {
            return;
        };
        let index = session.step;
        let step = &session.guide.steps[index];
        if !self.guide_gate_ready(step.gate) {
            self.toast(cx, ToastKind::Error, self.guide_gate_label(step.gate));
            return;
        }
        let total = session.guide.steps.len();
        if index + 1 >= total {
            let pending = self.guide.pending_guide.take();
            self.guide.session = None;
            if let Some(id) = pending {
                let title = guide::find(id).map(|guide| guide.title).unwrap_or("教程");
                if self.guide_gate_ready(GuideGate::DeviceSelected) {
                    self.start_guide_internal(id, cx);
                    self.toast(
                        cx,
                        ToastKind::Success,
                        format!("设备已连接，继续「{title}」"),
                    );
                } else {
                    self.toast(
                        cx,
                        ToastKind::Error,
                        format!("还没有检测到设备；连接完成后从快速指引打开「{title}」"),
                    );
                }
            } else {
                self.toast(cx, ToastKind::Success, "教程完成，可以继续使用 RareBox 了");
            }
            cx.notify();
            return;
        }
        if let Some(session) = self.guide.session.as_mut() {
            session.step += 1;
        }
        self.guide_enter_step(cx);
    }

    pub(crate) fn guide_prev(&mut self, cx: &mut Context<Self>) {
        let Some(session) = self.guide.session.as_mut() else {
            return;
        };
        if session.step == 0 {
            return;
        }
        session.step -= 1;
        self.guide_enter_step(cx);
    }

    pub(crate) fn guide_toggle_check(&mut self, step: usize, item: usize, cx: &mut Context<Self>) {
        if let Some(session) = self.guide.session.as_mut() {
            if let Some(row) = session.checked.get_mut(step) {
                if let Some(value) = row.get_mut(item) {
                    *value = !*value;
                }
            }
        }
        cx.notify();
    }

    pub(crate) fn guide_toggle_stuck(&mut self, cx: &mut Context<Self>) {
        if let Some(session) = self.guide.session.as_mut() {
            session.stuck_open = !session.stuck_open;
        }
        cx.notify();
    }

    pub(crate) fn guide_toggle_steps(&mut self, cx: &mut Context<Self>) {
        if let Some(session) = self.guide.session.as_mut() {
            session.steps_open = !session.steps_open;
        }
        cx.notify();
    }

    pub(crate) fn guide_interrupt(&mut self, cx: &mut Context<Self>) {
        let pending = self.guide.pending_guide.take();
        if self.guide.session.take().is_some() {
            if let Some(id) = pending {
                let title = guide::find(id).map(|guide| guide.title).unwrap_or("教程");
                self.toast(
                    cx,
                    ToastKind::Info,
                    format!("已中断连接教程；连接完成后可重新打开「{title}」"),
                );
            } else {
                self.toast(cx, ToastKind::Info, "已中断教程");
            }
        }
        cx.notify();
    }

    /// 当前教程被“没有设备”卡住时，改走连接教程，完成后回到当前教程。
    pub(crate) fn guide_connect_first(&mut self, cx: &mut Context<Self>) {
        let Some((current, title)) = self
            .guide
            .session
            .as_ref()
            .map(|session| (session.guide.id, session.guide.title))
        else {
            return;
        };
        let needs_device = guide::find(current).is_some_and(|guide| guide.requires_device());
        if Self::is_connection_guide(current) || !needs_device {
            self.set_page(Page::Connect, cx);
            return;
        }
        self.guide.pending_guide = Some(current);
        let target = self.guide_connect_target();
        self.start_guide_internal(target, cx);
        self.toast(
            cx,
            ToastKind::Info,
            format!("先完成连接，之后自动继续「{title}」"),
        );
    }

    pub(crate) fn guide_cancel_switch(&mut self, cx: &mut Context<Self>) {
        if let Some(session) = self.guide.session.as_mut() {
            session.pending_page = None;
            session.switch_token = session.switch_token.wrapping_add(1);
        }
        cx.notify();
    }

    pub(crate) fn guide_resume(&mut self, cx: &mut Context<Self>) {
        let page = self
            .guide
            .session
            .as_ref()
            .and_then(|session| session.guide.steps.get(session.step))
            .and_then(|step| step.page)
            .map(guide_page_to_ui);
        let Some(page) = page else {
            return;
        };
        if let Some(session) = self.guide.session.as_mut() {
            if self.page == page {
                session.pending_page = None;
                cx.notify();
                return;
            }
            session.pending_page = Some(page);
            session.switch_token = session.switch_token.wrapping_add(1);
            let token = session.switch_token;
            self.spawn_guide_switch(token, cx);
        }
        cx.notify();
    }

    pub(crate) fn guide_action(&mut self, action: GuideAction, cx: &mut Context<Self>) {
        match action {
            GuideAction::OpenPage(GuidePage::Connect)
                if !self.guide_gate_ready(GuideGate::DeviceSelected) =>
            {
                self.guide_connect_first(cx);
            }
            GuideAction::OpenPage(page) => self.set_page(guide_page_to_ui(page), cx),
            GuideAction::RefreshDevices => self.refresh_devices(cx),
        }
    }

    pub(crate) fn guide_gate_ready(&self, gate: GuideGate) -> bool {
        match gate {
            GuideGate::None => true,
            GuideGate::AdbReady => {
                if self.is_harmony() {
                    self.harmony.path.is_some()
                } else {
                    self.adb_path.is_some()
                }
            }
            GuideGate::DeviceSelected => {
                if self.is_harmony() {
                    self.harmony_device()
                        .is_some_and(|device| device.is_online())
                } else {
                    self.has_device()
                }
            }
        }
    }

    fn guide_gate_label(&self, gate: GuideGate) -> &'static str {
        match gate {
            GuideGate::None => "",
            GuideGate::AdbReady => "等待调试工具就绪：先完成「我在这里卡住了」里的检查",
            GuideGate::DeviceSelected => "先在顶栏选择一台在线设备，再继续",
        }
    }

    // ---------------------------------------------------------------------
    // 遮罩向导
    // ---------------------------------------------------------------------

    pub(crate) fn render_guide_overlay(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(session) = self.guide.session.clone() else {
            return div().into_any_element();
        };
        let colors = *cx.theme().colors();
        let viewport = window.viewport_size();
        let vw = f32::from(viewport.width).max(1.);
        let vh = f32::from(viewport.height).max(1.);
        let step = &session.guide.steps[session.step];
        let step_page = step.page.map(guide_page_to_ui);

        let mode = if let Some(page) = session.pending_page {
            OverlayMode::Switching(page)
        } else if let Some(page) = step_page.filter(|page| *page != self.page) {
            OverlayMode::Suspended(page)
        } else {
            OverlayMode::Step
        };

        let hole = match mode {
            OverlayMode::Switching(page) => {
                let page = ui_page_to_guide(page);
                raw_hole(
                    self.guide.anchors.handle(GuideTarget::Nav(page)).get(),
                    vw,
                    vh,
                )
            }
            OverlayMode::Suspended(_) => None,
            OverlayMode::Step => {
                if step.target == GuideTarget::None {
                    None
                } else {
                    raw_hole(self.guide.anchors.handle(step.target).get(), vw, vh)
                }
            }
        };

        let mobile = cfg!(any(target_os = "android", target_os = "ios"));
        let card_w = if mobile {
            (vw - 24.).max(240.)
        } else {
            420_f32.min(vw - 24.)
        };
        let card_h = 600_f32.min(vh - 24.);
        let (card_x, card_y) = place_card(hole, vw, vh, card_w, card_h);
        let gate_ready = self.guide_gate_ready(step.gate);
        let gate_label = self.guide_gate_label(step.gate);
        let card = self.render_guide_card(cx, &session, mode, gate_ready, gate_label);

        let scrim = colors.scrim.opacity(0.72);
        let mut layer = div().absolute().inset_0();
        if let Some(hole) = hole {
            let x0 = f32::from(hole.origin.x);
            let y0 = f32::from(hole.origin.y);
            let w = f32::from(hole.size.width);
            let h = f32::from(hole.size.height);
            let x1 = x0 + w;
            let y1 = y0 + h;
            if y0 > 0.5 {
                layer = layer.child(scrim_panel(0., 0., vw, y0, scrim));
            }
            if y1 < vh - 0.5 {
                layer = layer.child(scrim_panel(0., y1, vw, vh - y1, scrim));
            }
            if x0 > 0.5 {
                layer = layer.child(scrim_panel(0., y0, x0, h, scrim));
            }
            if x1 < vw - 0.5 {
                layer = layer.child(scrim_panel(x1, y0, vw - x1, h, scrim));
            }
            layer = layer.child(
                div()
                    .absolute()
                    .left(px(x0 - 5.))
                    .top(px(y0 - 5.))
                    .w(px(w + 10.))
                    .h(px(h + 10.))
                    .rounded(px(13.))
                    .border_2()
                    .border_color(colors.primary),
            );
            layer = layer.child(
                div()
                    .absolute()
                    .left(px(x0 - 10.))
                    .top(px(y0 - 10.))
                    .w(px(w + 20.))
                    .h(px(h + 20.))
                    .rounded(px(18.))
                    .border_1()
                    .border_color(colors.primary.opacity(0.35)),
            );
        } else {
            layer = layer.child(scrim_panel(0., 0., vw, vh, scrim));
        }

        layer
            .child(
                div()
                    .absolute()
                    .left(px(card_x))
                    .top(px(card_y))
                    .w(px(card_w))
                    .max_h(px(vh - 24.))
                    .occlude()
                    .child(card),
            )
            .into_any_element()
    }

    fn render_guide_card(
        &self,
        cx: &mut Context<Self>,
        session: &GuideSession,
        mode: OverlayMode,
        gate_ready: bool,
        gate_label: &'static str,
    ) -> Stateful<Div> {
        let colors = *cx.theme().colors();
        let weak = cx.entity().downgrade();
        let step = &session.guide.steps[session.step];
        let total = session.guide.steps.len();
        let connection_guide = Self::is_connection_guide(session.guide.id);
        let mut card = div()
            .id("guide-card-scroll")
            .w_full()
            .max_h(px(600.))
            .overflow_y_scroll()
            .rounded(px(18.))
            .bg(colors.surface_container_high)
            .shadow_lg()
            .p(px(18.))
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(guide_tag(
                        if step.choices.is_empty() {
                            format!("第 {} / {} 步", session.step + 1, total)
                        } else {
                            "选择连接方式".to_string()
                        },
                        colors.primary_container,
                        colors.on_primary_container,
                    ))
                    .child(div().flex_1())
                    .child(ui::icon_button(
                        cx,
                        "guide-steps-toggle",
                        "list",
                        30.,
                        colors.on_surface_variant,
                        cx.listener(|app, _, _, cx| app.guide_toggle_steps(cx)),
                    ))
                    .child(ui::action_button(
                        cx,
                        "guide-interrupt",
                        "中断教程",
                        Some("close"),
                        ActionKind::Text,
                        true,
                        cx.listener(|app, _, _, cx| app.guide_interrupt(cx)),
                    )),
            );

        if let Some(pending) = self.guide.pending_guide {
            let title = guide::find(pending)
                .map(|guide| guide.title)
                .unwrap_or("教程");
            card = card.child(ui::callout(
                cx,
                "tabler-usb",
                format!("完成连接后自动继续「{title}」"),
            ));
        }

        if session.steps_open {
            let rows: Vec<AnyElement> = session
                .guide
                .steps
                .iter()
                .enumerate()
                .map(|(index, item)| {
                    let current = index == session.step;
                    let done = index < session.step;
                    div()
                        .flex()
                        .items_start()
                        .gap(px(8.))
                        .px(px(8.))
                        .py(px(5.))
                        .rounded(px(8.))
                        .when(current, |el| el.bg(colors.secondary_container))
                        .child(
                            div()
                                .size(px(18.))
                                .flex_none()
                                .rounded_full()
                                .bg(if done || current {
                                    colors.primary
                                } else {
                                    colors.surface_container_highest
                                })
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(
                                    div()
                                        .text_size(px(9.5))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(if done || current {
                                            colors.on_primary
                                        } else {
                                            colors.on_surface_variant
                                        })
                                        .child(if done {
                                            "✓".to_string()
                                        } else {
                                            (index + 1).to_string()
                                        }),
                                ),
                        )
                        .child(
                            div()
                                .flex_1()
                                .text_size(px(11.5))
                                .text_color(if current {
                                    colors.on_secondary_container
                                } else {
                                    colors.on_surface_variant
                                })
                                .child(item.title),
                        )
                        .into_any_element()
                })
                .collect();
            return card.child(
                div()
                    .id("guide-steps-list")
                    .max_h(px(170.))
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .children(rows),
            );
        }

        match mode {
            OverlayMode::Switching(page) => {
                return card.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(8.))
                        .child(
                            div()
                                .text_size(px(21.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(format!("正在前往「{}」", page.label())),
                        )
                        .child(
                            div()
                                .text_size(px(GUIDE_BODY))
                                .line_height(px(21.))
                                .text_color(colors.on_surface_variant)
                                .child("侧栏里的目标已经高亮。停顿一下，马上自动切过去。"),
                        )
                        .child(
                            div()
                                .mt(px(4.))
                                .h(px(4.))
                                .w_full()
                                .rounded_full()
                                .bg(colors.surface_container_highest)
                                .child(div().h_full().w_full().rounded_full().bg(colors.primary)),
                        )
                        .child(
                            div()
                                .mt(px(6.))
                                .flex()
                                .justify_end()
                                .child(ui::action_button(
                                    cx,
                                    "guide-cancel-switch",
                                    "留在当前页",
                                    None,
                                    ActionKind::Text,
                                    true,
                                    cx.listener(|app, _, _, cx| app.guide_cancel_switch(cx)),
                                )),
                        ),
                );
            }
            OverlayMode::Suspended(page) => {
                return card.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(8.))
                        .child(
                            div()
                                .text_size(px(21.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child("教程暂停了"),
                        )
                        .child(
                            div()
                                .text_size(px(GUIDE_BODY))
                                .line_height(px(21.))
                                .text_color(colors.on_surface_variant)
                                .child(format!(
                                    "这一步需要在「{}」完成。你可以继续返回，或中断教程。",
                                    page.label()
                                )),
                        )
                        .child(
                            div()
                                .mt(px(6.))
                                .flex()
                                .items_center()
                                .gap(px(8.))
                                .child(ui::action_button(
                                    cx,
                                    "guide-resume",
                                    "继续并返回",
                                    Some("play_arrow"),
                                    ActionKind::Filled,
                                    true,
                                    cx.listener(|app, _, _, cx| app.guide_resume(cx)),
                                ))
                                .child(ui::action_button(
                                    cx,
                                    "guide-suspended-interrupt",
                                    "中断教程",
                                    None,
                                    ActionKind::Text,
                                    true,
                                    cx.listener(|app, _, _, cx| app.guide_interrupt(cx)),
                                )),
                        ),
                );
            }
            OverlayMode::Step => {}
        }

        let mut card = card.child(
            div()
                .text_size(px(21.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(colors.on_surface)
                .child(step.title),
        );

        if let Some(art) = step.art {
            if let Some(bytes) = guide_art::bytes(art) {
                card = card.child(
                    div()
                        .w_full()
                        .h(px(148.))
                        .rounded(px(12.))
                        .bg(colors.surface_container_highest)
                        .overflow_hidden()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            svg()
                                .data(bytes)
                                .w(px(300.))
                                .h(px(140.))
                                .text_color(colors.primary),
                        ),
                );
            }
        }

        card = card.child(
            div()
                .text_size(px(GUIDE_BODY))
                .line_height(px(21.))
                .text_color(colors.on_surface_variant)
                .child(step.body),
        );

        if !step.choices.is_empty() {
            // 有探测结论时，给匹配的方式打「推荐」，其余保持可点。
            let recommended = self
                .snapshot
                .as_ref()
                .map(|snapshot| snapshot.debug_support())
                .and_then(|support| support.recommended())
                .map(|transport| transport.guide_id());
            let choices: Vec<AnyElement> = step
                .choices
                .iter()
                .enumerate()
                .map(|(index, choice)| {
                    let weak = weak.clone();
                    let is_recommended = recommended == Some(choice.guide);
                    div()
                        .id(ElementId::Name(format!("guide-choice-{index}").into()))
                        .w_full()
                        .p(px(12.))
                        .rounded(px(14.))
                        .bg(if is_recommended {
                            colors.secondary_container
                        } else {
                            colors.surface_container
                        })
                        .flex()
                        .items_center()
                        .gap(px(12.))
                        .cursor_pointer()
                        .hover(move |style| style.bg(colors.surface_container_highest))
                        .on_click(move |_, _, cx| {
                            if let Err(err) = weak.update(cx, |app, cx| {
                                app.guide_choose(choice.guide, cx);
                            }) {
                                eprintln!("RareBox Desktop: 选择连接方式失败: {err}");
                            }
                        })
                        .child(
                            div()
                                .size(px(44.))
                                .flex_none()
                                .rounded(px(12.))
                                .bg(colors.secondary_container)
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(
                                    Icon::new(IconName::new(choice.icon))
                                        .size(px(24.))
                                        .color(colors.on_secondary_container),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(2.))
                                .flex_1()
                                .min_w(px(0.))
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap(px(6.))
                                        .child(
                                            div()
                                                .text_size(px(15.))
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .text_color(colors.on_surface)
                                                .child(choice.label),
                                        )
                                        .when(is_recommended, |el| {
                                            el.child(
                                                div()
                                                    .px(px(6.))
                                                    .py(px(1.))
                                                    .rounded(px(6.))
                                                    .bg(colors.primary)
                                                    .text_size(px(10.))
                                                    .text_color(colors.on_primary)
                                                    .child("推荐"),
                                            )
                                        }),
                                )
                                .child(
                                    div()
                                        .text_size(px(12.5))
                                        .line_height(px(17.))
                                        .text_color(colors.on_surface_variant)
                                        .child(choice.hint),
                                ),
                        )
                        .child(
                            Icon::new(IconName::new("chevron_right"))
                                .size(px(18.))
                                .color(colors.on_surface_variant),
                        )
                        .into_any_element()
                })
                .collect();
            card = card.child(div().flex().flex_col().gap(px(8.)).children(choices));
            card = card.child(
                div()
                    .text_size(px(12.))
                    .text_color(colors.on_surface_variant)
                    .child("点一种方式开始，随时可以中断。"),
            );
            return card;
        }

        if !step.checklist.is_empty() {
            let step_index = session.step;
            let rows: Vec<AnyElement> = step
                .checklist
                .iter()
                .enumerate()
                .map(|(item_index, item)| {
                    let checked = session
                        .checked
                        .get(step_index)
                        .and_then(|row| row.get(item_index))
                        .copied()
                        .unwrap_or(false);
                    div()
                        .id(ElementId::Name(
                            format!("guide-check-{step_index}-{item_index}").into(),
                        ))
                        .flex()
                        .items_start()
                        .gap(px(8.))
                        .cursor_pointer()
                        .on_click(cx.listener(move |app, _, _, cx| {
                            app.guide_toggle_check(step_index, item_index, cx);
                        }))
                        .child(
                            div()
                                .mt(px(1.))
                                .size(px(20.))
                                .flex_none()
                                .rounded(px(5.))
                                .border_1()
                                .border_color(if checked {
                                    colors.primary
                                } else {
                                    colors.outline
                                })
                                .bg(if checked {
                                    colors.primary
                                } else {
                                    colors.surface.opacity(0.0)
                                })
                                .flex()
                                .items_center()
                                .justify_center()
                                .when(checked, |el| {
                                    el.child(
                                        Icon::new(IconName::new("check"))
                                            .size(px(15.))
                                            .color(colors.on_primary),
                                    )
                                }),
                        )
                        .child(
                            div()
                                .flex_1()
                                .text_size(px(13.5))
                                .line_height(px(20.))
                                .text_color(if checked {
                                    colors.on_surface_variant
                                } else {
                                    colors.on_surface
                                })
                                .child(*item),
                        )
                        .into_any_element()
                })
                .collect();
            card = card.child(div().flex().flex_col().gap(px(6.)).children(rows));
        }

        if !step.stuck.is_empty() {
            card = card.child(ui::action_button_sized(
                cx,
                "guide-stuck-toggle",
                if session.stuck_open {
                    "收起详细指引"
                } else {
                    "我在这里卡住了"
                },
                Some("tabler-lifebuoy"),
                ActionKind::Text,
                true,
                44.,
                13.5,
                17.,
                cx.listener(|app, _, _, cx| app.guide_toggle_stuck(cx)),
            ));
            if session.stuck_open {
                card = card.child(render_stuck_list(cx, &weak, step.stuck, colors));
            }
        }

        if !gate_ready {
            card = card.child(ui::callout(cx, "hourglass_empty", gate_label));
            if step.gate == GuideGate::DeviceSelected && !connection_guide {
                card = card.child(ui::action_button_sized(
                    cx,
                    "guide-gate-connect",
                    "先连接设备",
                    Some("tabler-usb"),
                    ActionKind::Filled,
                    true,
                    46.,
                    14.5,
                    18.,
                    cx.listener(|app, _, _, cx| app.guide_connect_first(cx)),
                ));
            }
            card = card.child(ui::action_button(
                cx,
                "guide-gate-refresh",
                "重新检测",
                Some("refresh"),
                ActionKind::Text,
                true,
                cx.listener(|app, _, _, cx| {
                    if app.is_harmony() {
                        app.refresh_hdc(cx);
                    } else {
                        app.refresh_devices(cx);
                    }
                }),
            ));
        }

        let prev_enabled = session.step > 0;
        let next_label = if session.step + 1 >= total {
            "完成"
        } else {
            "下一步"
        };
        card = card.child(
            div()
                .mt(px(2.))
                .flex()
                .items_center()
                .gap(px(8.))
                .child(ui::action_button_sized(
                    cx,
                    "guide-prev",
                    "上一步",
                    Some("arrow_back"),
                    ActionKind::Outlined,
                    prev_enabled,
                    46.,
                    14.5,
                    18.,
                    cx.listener(|app, _, _, cx| app.guide_prev(cx)),
                ))
                .child(
                    div()
                        .flex_1()
                        .flex()
                        .justify_center()
                        .items_center()
                        .gap(px(5.))
                        .children((0..total).map(|index| {
                            div()
                                .size(px(5.))
                                .rounded_full()
                                .bg(if index == session.step {
                                    colors.primary
                                } else if index < session.step {
                                    colors.primary.opacity(0.45)
                                } else {
                                    colors.outline_variant
                                })
                        })),
                )
                .child(ui::action_button_sized(
                    cx,
                    "guide-next",
                    next_label,
                    Some("chevron_right"),
                    ActionKind::Filled,
                    gate_ready,
                    46.,
                    14.5,
                    18.,
                    cx.listener(|app, _, _, cx| app.guide_next(cx)),
                )),
        );

        card
    }
}

fn render_stuck_list(
    cx: &App,
    weak: &WeakEntity<RareBox>,
    stuck: &'static [StuckHelp],
    colors: material3_gpui::theme::ColorScheme,
) -> Stateful<Div> {
    let rows: Vec<AnyElement> = stuck
        .iter()
        .enumerate()
        .map(|(index, help)| {
            let mut row = div()
                .flex()
                .flex_col()
                .gap(px(3.))
                .p(px(10.))
                .rounded(px(10.))
                .bg(colors.surface_container)
                .child(
                    div()
                        .text_size(px(13.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(colors.on_surface)
                        .child(help.title),
                )
                .child(
                    div()
                        .text_size(px(12.5))
                        .line_height(px(19.))
                        .text_color(colors.on_surface_variant)
                        .child(help.detail),
                );
            if let Some(action) = help.action {
                let id = ElementId::Name(format!("guide-stuck-action-{index}").into());
                row = row.child(
                    ui::action_button(
                        cx,
                        id,
                        match action {
                            GuideAction::OpenPage(page) => {
                                format!("打开「{}」", page_label(page))
                            }
                            GuideAction::RefreshDevices => "重新检测设备".to_string(),
                        },
                        Some("arrow_upward"),
                        ActionKind::Text,
                        true,
                        {
                            let weak = weak.clone();
                            move |_, _, cx| {
                                if let Err(err) = weak.update(cx, |app, cx| {
                                    app.guide_action(action, cx);
                                }) {
                                    eprintln!("RareBox Desktop: 执行排查动作失败: {err}");
                                }
                            }
                        },
                    )
                    .into_any_element(),
                );
            }
            row.into_any_element()
        })
        .collect();
    div()
        .id("guide-stuck-list")
        .max_h(px(210.))
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .gap(px(6.))
        .children(rows)
}

fn assistant_avatar(cx: &App) -> Div {
    let colors = *cx.theme().colors();
    div()
        .size(px(30.))
        .flex_none()
        .rounded(px(10.))
        .bg(colors.primary_container)
        .flex()
        .items_center()
        .justify_center()
        .child(
            Icon::new(IconName::new("help"))
                .size(px(17.))
                .color(colors.on_primary_container),
        )
}

fn scrim_panel(x: f32, y: f32, w: f32, h: f32, color: gpui::Hsla) -> Div {
    div()
        .absolute()
        .left(px(x))
        .top(px(y))
        .w(px(w.max(0.)))
        .h(px(h.max(0.)))
        .bg(color)
        .block_mouse_except_scroll()
}

fn raw_hole(bounds: Bounds<Pixels>, vw: f32, vh: f32) -> Option<Bounds<Pixels>> {
    let w = f32::from(bounds.size.width);
    let h = f32::from(bounds.size.height);
    if w < 2. || h < 2. {
        return None;
    }
    let pad = 6.;
    let x0 = (f32::from(bounds.origin.x) - pad).max(0.);
    let y0 = (f32::from(bounds.origin.y) - pad).max(0.);
    let x1 = (f32::from(bounds.origin.x) + w + pad).min(vw);
    let y1 = (f32::from(bounds.origin.y) + h + pad).min(vh);
    if x1 - x0 < 4. || y1 - y0 < 4. {
        return None;
    }
    Some(Bounds {
        origin: gpui::point(px(x0), px(y0)),
        size: gpui::size(px(x1 - x0), px(y1 - y0)),
    })
}

fn place_card(hole: Option<Bounds<Pixels>>, vw: f32, vh: f32, w: f32, h: f32) -> (f32, f32) {
    let margin = 12.;
    let Some(hole) = hole else {
        return ((vw - w) / 2., ((vh - h) / 2.).max(margin));
    };
    let x0 = f32::from(hole.origin.x);
    let y0 = f32::from(hole.origin.y);
    let x1 = x0 + f32::from(hole.size.width);
    let y1 = y0 + f32::from(hole.size.height);

    fn overlap(cx: f32, cy: f32, w: f32, h: f32, hole: Bounds<Pixels>) -> f32 {
        let x0 = f32::from(hole.origin.x);
        let y0 = f32::from(hole.origin.y);
        let x1 = x0 + f32::from(hole.size.width);
        let y1 = y0 + f32::from(hole.size.height);
        let ax0 = cx.max(x0);
        let ay0 = cy.max(y0);
        let ax1 = (cx + w).min(x1);
        let ay1 = (cy + h).min(y1);
        ((ax1 - ax0).max(0.)) * ((ay1 - ay0).max(0.))
    }

    let candidates = [
        (x1 + margin, y0 - 8.),
        (x0 - w - margin, y0 - 8.),
        ((vw - w) / 2., y1 + margin),
        ((vw - w) / 2., y0 - h - margin),
        ((vw - w) / 2., (vh - h) / 2.),
    ];
    let mut best: Option<(f32, f32, f32)> = None;
    for (cx, cy) in candidates {
        let x = cx.clamp(margin, (vw - w - margin).max(margin));
        let y = cy.clamp(margin, (vh - h - margin).max(margin));
        let score = overlap(x, y, w, h, hole);
        let better = match best {
            None => true,
            Some((_, _, best_score)) => score < best_score - f32::EPSILON,
        };
        if better {
            best = Some((x, y, score));
        }
        if score <= 0. {
            break;
        }
    }
    let Some((x, y, _)) = best else {
        return ((vw - w) / 2., ((vh - h) / 2.).max(margin));
    };
    (x, y)
}

pub(crate) fn guide_page_to_ui(page: GuidePage) -> Page {
    match page {
        GuidePage::Overview => Page::Overview,
        GuidePage::Connect => Page::Connect,
        GuidePage::Apps => Page::Apps,
        GuidePage::Screen => Page::Screen,
        GuidePage::Remote => Page::Remote,
        GuidePage::Files => Page::Files,
        GuidePage::Terminal => Page::Terminal,
        GuidePage::Logs => Page::Logs,
        GuidePage::Tools => Page::Tools,
        GuidePage::Signing => Page::Signing,
        GuidePage::Settings => Page::Settings,
    }
}

pub(crate) fn ui_page_to_guide(page: Page) -> GuidePage {
    match page {
        Page::Overview => GuidePage::Overview,
        Page::Connect => GuidePage::Connect,
        Page::Apps => GuidePage::Apps,
        Page::Screen => GuidePage::Screen,
        Page::Remote => GuidePage::Remote,
        Page::Files => GuidePage::Files,
        Page::Terminal => GuidePage::Terminal,
        Page::Logs => GuidePage::Logs,
        Page::Tools => GuidePage::Tools,
        Page::Signing => GuidePage::Signing,
        Page::Settings => GuidePage::Settings,
        Page::Guide => GuidePage::Overview,
    }
}

fn page_label(page: GuidePage) -> &'static str {
    guide_page_to_ui(page).label()
}

fn platform_label(guide: &Guide) -> String {
    if guide.platforms.windows_only {
        return "Windows".to_string();
    }
    match (guide.platforms.android, guide.platforms.harmony) {
        (true, true) => "Android / HarmonyOS".to_string(),
        (true, false) => "Android".to_string(),
        (false, true) => "HarmonyOS".to_string(),
        (false, false) => "—".to_string(),
    }
}
