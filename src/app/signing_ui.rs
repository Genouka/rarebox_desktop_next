//! 鸿蒙签名页渲染：账号、证书、设备、Profile、ACL 与七步向导。

use std::path::PathBuf;

use super::shell_ui::page_scroll;
use super::{ImportFile, LoginPhase, Page, RareBox};
use crate::signing::model::format_timestamp;
use crate::signing::{SigningBlock, Stage, StageStatus};
use crate::ui::{self, ActionKind, MONO_FONT};
use gpui::{
    AnyElement, ClipboardItem, Context, FontWeight, IntoElement, ParentElement, Styled, div,
    prelude::*, px,
};
use material3_gpui::icon::{Icon, IconName};
use material3_gpui::prelude::*;
use material3_gpui::theme::ActiveTheme;

impl RareBox {
    pub(crate) fn render_signing(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let content = div()
            .w_full()
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(self.signing_account_card(cx))
            .child(self.signing_cert_card(cx))
            .child(self.signing_device_card(cx))
            .child(self.signing_profile_card(cx))
            .child(self.signing_acl_card(cx))
            .child(self.signing_flow_card(cx))
            .child(self.signing_logs_card(cx));
        page_scroll(
            Page::Signing,
            &self.scroll_handles[Page::Signing as usize],
            content,
        )
        .into_any_element()
    }

    fn signing_account_card(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = *cx.theme().colors();
        let auth = self.signing.service.auth.clone();
        let phase = self.signing.login.clone();
        let busy = self.signing.busy;
        let mut card = Card::new().outlined().p(px(20.)).child(ui::section_header(
            cx,
            "华为开发者账号",
            Some("登录后才能在 AGC 申请证书、注册设备与 Profile"),
            None,
        ));

        if let Some(auth) = auth {
            let logged_name = if auth.nick_name.is_empty() {
                auth.user_id.clone()
            } else {
                auth.nick_name.clone()
            };
            let team_id = auth.team_id_or_user().to_string();
            let mut teams = div().flex().flex_wrap().items_center().gap(px(8.));
            for team in &self.signing.service.teams {
                let id = team.id.clone();
                teams = teams.child(ui::filter_chip(
                    cx,
                    format!("signing-team-{}", team.id),
                    if team.name.is_empty() {
                        team.id.clone()
                    } else {
                        team.name.clone()
                    },
                    team.id == team_id,
                    cx.listener(move |app, _, _, cx| app.signing_select_team(id.clone(), cx)),
                ));
            }
            card = card
                .child(
                    div()
                        .mt(px(14.))
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .gap(px(12.))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(2.))
                                .child(
                                    div()
                                        .text_size(px(15.))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(colors.on_surface)
                                        .child(logged_name),
                                )
                                .child(
                                    div()
                                        .text_size(px(11.))
                                        .text_color(colors.on_surface_variant)
                                        .child(format!("UID {} · Team {team_id}", auth.user_id)),
                                ),
                        )
                        .child(div().flex_1())
                        .child(ui::action_button(
                            cx,
                            "signing-refresh",
                            "刷新资源",
                            Some("refresh"),
                            ActionKind::Tonal,
                            !busy,
                            cx.listener(|app, _, _, cx| app.refresh_signing(cx)),
                        ))
                        .child(ui::action_button(
                            cx,
                            "signing-logout",
                            "退出登录",
                            Some("link_off"),
                            ActionKind::Text,
                            !busy,
                            cx.listener(|app, _, _, cx| app.signing_logout(cx)),
                        )),
                )
                .child(
                    div()
                        .mt(px(14.))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(
                            div()
                                .text_size(px(10.5))
                                .text_color(colors.on_surface_variant)
                                .child("团队"),
                        )
                        .child(teams),
                );
        } else {
            let mut actions = div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(ui::action_button(
                    cx,
                    "signing-login",
                    if matches!(phase, LoginPhase::Waiting) {
                        "取消登录"
                    } else {
                        "登录华为开发者账号"
                    },
                    Some("link"),
                    ActionKind::Filled,
                    !busy,
                    cx.listener(|app, _, _, cx| app.signing_login(cx)),
                ));
            if matches!(phase, LoginPhase::Waiting) {
                actions = actions.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .child(LoadingIndicator::new("signing-login-wait").size(px(14.)))
                        .child(
                            div()
                                .text_size(px(11.))
                                .text_color(colors.primary)
                                .child("等待浏览器回调…"),
                        ),
                );
            }
            card = card.child(
                div()
                    .mt(px(14.))
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    .child(
                        div()
                            .text_size(px(12.))
                            .line_height(px(19.))
                            .text_color(colors.on_surface_variant)
                            .child("登录会打开系统浏览器。完成后回到本页即可；token 保存在应用数据目录，仅用于访问 AGC。若网络受限，请关闭代理并使用国内网络。"),
                    )
                    .child(actions)
                    .when_some(self.signing.error.clone(), |el, error| {
                        el.child(
                            div()
                                .text_size(px(11.))
                                .text_color(colors.error)
                                .child(error),
                        )
                    }),
            );
        }

        if let LoginPhase::Failed(error) = phase {
            card = card.child(
                div()
                    .mt(px(12.))
                    .text_size(px(11.))
                    .text_color(colors.error)
                    .child(error),
            );
        }

        card.into_any_element()
    }

    fn signing_cert_card(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = *cx.theme().colors();
        let logged = self.signing.service.is_logged_in();
        let busy = self.signing.busy;
        let imported = self.signing.use_imported;

        let mode_chips = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(8.))
            .child(ui::filter_chip(
                cx,
                "signing-mode-auto",
                "自动申请（调试）",
                !imported,
                cx.listener(|app, _, _, cx| {
                    if app.signing.use_imported {
                        app.signing_toggle_imported(cx);
                    }
                }),
            ))
            .child(ui::filter_chip(
                cx,
                "signing-mode-import",
                "导入材料（发布/自定义）",
                imported,
                cx.listener(|app, _, _, cx| {
                    if !app.signing.use_imported {
                        app.signing_toggle_imported(cx);
                    }
                }),
            ));

        let mut card = Card::new()
            .outlined()
            .p(px(20.))
            .child(ui::section_header(
                cx,
                "签名证书",
                Some(if imported {
                    "使用 DevEco / AGC 生成的 .p12、.cer、.p7b"
                } else {
                    "本地生成 P-256 密钥；调试证书由 AGC 签发"
                }),
                None,
            ))
            .child(div().mt(px(12.)).child(mode_chips));

        if imported {
            let file_row = |label: &'static str,
                            path: &Option<PathBuf>,
                            which: ImportFile| {
                let name = path
                    .as_ref()
                    .map(|path| {
                        path.file_name()
                            .map(|name| name.to_string_lossy().to_string())
                            .unwrap_or_else(|| path.display().to_string())
                    })
                    .unwrap_or_else(|| "未选择".to_string());
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .w(px(82.))
                            .flex_none()
                            .text_size(px(10.))
                            .text_color(colors.on_surface_variant)
                            .child(label),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .truncate()
                            .text_size(px(11.))
                            .text_color(if path.is_some() {
                                colors.on_surface
                            } else {
                                colors.on_surface_variant
                            })
                            .child(name),
                    )
                    .child(ui::action_button(
                        cx,
                        format!("signing-import-{}", label),
                        "选择",
                        Some("folder_open"),
                        ActionKind::Text,
                        !busy,
                        cx.listener(move |app, _, _, cx| app.signing_pick_import_file(which, cx)),
                    ))
            };

            card = card
                .child(
                    div()
                        .mt(px(14.))
                        .flex()
                        .flex_col()
                        .gap(px(7.))
                        .child(file_row(
                            "密钥库",
                            &self.signing.import_keystore,
                            ImportFile::Keystore,
                        ))
                        .child(file_row(
                            "应用证书",
                            &self.signing.import_cert,
                            ImportFile::Cert,
                        ))
                        .child(file_row(
                            "Profile",
                            &self.signing.import_profile,
                            ImportFile::Profile,
                        )),
                )
                .child(
                    div()
                        .mt(px(12.))
                        .flex()
                        .flex_wrap()
                        .items_end()
                        .gap(px(10.))
                        .child(
                            div()
                                .w(px(220.))
                                .child(self.signing_import_alias_field.clone()),
                        )
                        .child(
                            div()
                                .w(px(220.))
                                .child(self.signing_import_password_field.clone()),
                        ),
                )
                .child(
                    div()
                        .mt(px(10.))
                        .text_size(px(10.))
                        .line_height(px(16.))
                        .text_color(colors.on_surface_variant)
                        .child("口令同时用于密钥库与私钥（DevEco 默认相同）。发布证书/Profile 请先在 AGC 网站申请并下载；本工具不会调用未公开的发布接口。"),
                );
            return card.into_any_element();
        }

        let identity = self.signing.service.identity(self.signing.kind).cloned();
        let certs: Vec<_> = self
            .signing
            .service
            .certs
            .iter()
            .filter(|cert| cert.cert_type == self.signing.kind.cert_type())
            .cloned()
            .collect();
        let reset_label = if self.signing.reset_cert {
            "确认删除并重新申请"
        } else {
            "重置证书"
        };
        let reset_kind = if self.signing.reset_cert {
            ActionKind::Danger
        } else {
            ActionKind::Text
        };

        card = card.child(
            div()
                .mt(px(12.))
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(8.))
                .child(div().flex_1())
                .child(ui::action_button(
                    cx,
                    "signing-apply-cert",
                    "申请 / 更新调试证书",
                    Some("verified"),
                    ActionKind::Tonal,
                    logged && !busy,
                    cx.listener(|app, _, _, cx| app.signing_apply_cert(false, cx)),
                ))
                .child(ui::action_button(
                    cx,
                    "signing-reset-cert",
                    reset_label,
                    Some("restart_alt"),
                    reset_kind,
                    logged && !busy,
                    cx.listener(|app, _, _, cx| {
                        if app.signing.reset_cert {
                            app.signing_apply_cert(true, cx);
                        } else {
                            app.signing_toggle_reset_cert(cx);
                        }
                    }),
                )),
        );

        if let Some(identity) = identity {
            card = card.child(
                div()
                    .mt(px(14.))
                    .rounded(px(12.))
                    .bg(colors.surface_container_low)
                    .p(px(12.))
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(info_row(cx, "证书名称", &identity.cert_name, false))
                    .child(info_row(cx, "证书 ID", &identity.cert_id, true))
                    .child(info_row(
                        cx,
                        "有效期至",
                        &format_timestamp(identity.cert_expires_at),
                        false,
                    ))
                    .child(info_row(cx, "密钥库", &identity.keystore, true)),
            );
        } else {
            card = card.child(
                div()
                    .mt(px(12.))
                    .text_size(px(11.))
                    .text_color(colors.on_surface_variant)
                    .child("本地还没有调试签名身份。点击「申请 / 更新调试证书」会生成 P-256 密钥库和 CSR。"),
            );
        }

        card = card.child(
            div()
                .mt(px(10.))
                .text_size(px(10.))
                .line_height(px(16.))
                .text_color(colors.on_surface_variant)
                .child("「重置证书」会删除 AGC 上的同名证书，以及 RareBox 创建的调试 Profile；请确认后再执行。发布材料请使用「导入材料」。"),
        );

        if !certs.is_empty() {
            card = card.child(
                div()
                    .mt(px(14.))
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(
                        div()
                            .text_size(px(10.5))
                            .text_color(colors.on_surface_variant)
                            .child("AGC 云端调试证书"),
                    )
                    .children(certs.into_iter().take(6).map(|cert| {
                        let expired = cert.is_expired();
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                Icon::new(IconName::new(if expired {
                                    "error"
                                } else {
                                    "check_circle"
                                }))
                                .size(px(15.))
                                .color(if expired {
                                    colors.error
                                } else {
                                    colors.tertiary
                                }),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .text_size(px(11.5))
                                    .text_color(colors.on_surface)
                                    .truncate()
                                    .child(cert.cert_name.clone()),
                            )
                            .child(
                                div()
                                    .text_size(px(10.5))
                                    .text_color(colors.on_surface_variant)
                                    .child(if expired {
                                        "已过期".to_string()
                                    } else {
                                        format!("至 {}", format_timestamp(cert.expire_time))
                                    }),
                            )
                    })),
            );
        }

        card.into_any_element()
    }

    fn signing_device_card(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = *cx.theme().colors();
        let udid = self
            .signing
            .udid
            .clone()
            .or_else(|| {
                self.harmony
                    .snapshot
                    .as_ref()
                    .map(|snapshot| snapshot.udid.clone())
            })
            .filter(|udid| !udid.is_empty());
        let devices = self.signing.service.devices.clone();
        let logged = self.signing.service.is_logged_in();
        let busy = self.signing.busy;
        let imported = self.signing.use_imported;

        let mut card = Card::new()
            .outlined()
            .p(px(20.))
            .child(ui::section_header(
                cx,
                "调试设备",
                Some(if imported {
                    "导入的调试 Profile 仍会校验当前设备 UDID"
                } else {
                    "调试 Profile 会绑定当前设备的 UDID"
                }),
                None,
            ))
            .child(
                div()
                    .mt(px(14.))
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(8.))
                    .child(info_row(
                        cx,
                        "当前设备 UDID",
                        udid.as_deref().unwrap_or("未读取"),
                        true,
                    ))
                    .child(div().flex_1())
                    .child(ui::action_button(
                        cx,
                        "signing-load-udid",
                        "读取 UDID",
                        Some("smartphone"),
                        ActionKind::Text,
                        !busy,
                        cx.listener(|app, _, _, cx| app.load_signing_udid(cx)),
                    ))
                    .when(!imported, |el| {
                        el.child(ui::action_button(
                            cx,
                            "signing-register-device",
                            "注册当前设备",
                            Some("add_circle"),
                            ActionKind::Tonal,
                            logged && !busy && udid.is_some(),
                            cx.listener(|app, _, _, cx| app.signing_register_device(cx)),
                        ))
                    }),
            );

        if !imported && !devices.is_empty() {
            card = card.child(
                div()
                    .mt(px(14.))
                    .flex()
                    .flex_col()
                    .gap(px(5.))
                    .child(
                        div()
                            .text_size(px(10.5))
                            .text_color(colors.on_surface_variant)
                            .child(format!("AGC 已注册设备（{}）", devices.len())),
                    )
                    .children(devices.into_iter().take(8).map(|device| {
                        let current = udid.as_deref() == Some(device.udid.as_str());
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(Icon::new(IconName::new("smartphone")).size(px(15.)).color(
                                if current {
                                    colors.primary
                                } else {
                                    colors.on_surface_variant
                                },
                            ))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .truncate()
                                    .text_size(px(11.5))
                                    .text_color(colors.on_surface)
                                    .child(device.device_name.clone()),
                            )
                            .when(current, |el| {
                                el.child(ui::tag(
                                    "当前设备",
                                    colors.primary_container,
                                    colors.on_primary_container,
                                ))
                            })
                            .child(
                                div()
                                    .max_w(px(280.))
                                    .truncate()
                                    .text_size(px(9.5))
                                    .text_color(colors.on_surface_variant)
                                    .child(device.udid.clone()),
                            )
                    })),
            );
        }

        card.into_any_element()
    }

    fn signing_profile_card(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = *cx.theme().colors();
        let Some(meta) = self.signing.package.as_ref() else {
            return Card::new()
                .outlined()
                .p(px(20.))
                .child(ui::section_header(
                    cx,
                    "Profile",
                    Some("先选择 HAP/HSP，才能确定 bundle 与 ACL"),
                    None,
                ))
                .child(
                    div()
                        .mt(px(12.))
                        .text_size(px(11.))
                        .text_color(colors.on_surface_variant)
                        .child("尚未选择安装包。"),
                )
                .into_any_element();
        };
        let bundle = meta.bundle_name.clone();
        let cached = self
            .signing
            .local_profiles
            .iter()
            .find(|profile| profile.package_name == bundle && profile.kind == self.signing.kind)
            .cloned();
        let profiles: Vec<_> = self
            .signing
            .service
            .profiles
            .iter()
            .filter(|profile| {
                profile.package_name == bundle
                    || (profile.package_name.is_empty() && profile.provision_name.contains(&bundle))
            })
            .cloned()
            .collect();

        let mut card = Card::new()
            .outlined()
            .p(px(20.))
            .child(ui::section_header(
                cx,
                "Profile",
                Some(if self.signing.use_imported {
                    "使用导入的 .p7b；本工具不会申请或修改 Profile"
                } else {
                    "Profile 绑定 bundle、证书、ACL 与调试设备"
                }),
                None,
            ))
            .child(
                div()
                    .mt(px(12.))
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(info_row(cx, "Bundle", &bundle, true))
                    .child(info_row(
                        cx,
                        "版本",
                        &format!("{} ({})", meta.version_name, meta.version_code),
                        false,
                    ))
                    .child(info_row(
                        cx,
                        "模块",
                        &format!(
                            "{}{}{}",
                            if meta.module_name.is_empty() {
                                "—"
                            } else {
                                meta.module_name.as_str()
                            },
                            if meta.module_type.is_empty() {
                                String::new()
                            } else {
                                format!(" · {}", meta.module_type)
                            },
                            if meta.device_types.is_empty() {
                                String::new()
                            } else {
                                format!(" · {}", meta.device_types.join("/"))
                            }
                        ),
                        false,
                    ))
                    .when_some(meta.min_api, |el, api| {
                        el.child(info_row(cx, "最低 API", &api.to_string(), false))
                    }),
            );

        if self.signing.use_imported {
            let profile_name = self
                .signing
                .import_profile
                .as_ref()
                .map(|path| {
                    path.file_name()
                        .map(|name| name.to_string_lossy().to_string())
                        .unwrap_or_else(|| path.display().to_string())
                })
                .unwrap_or_else(|| "未选择".to_string());
            card = card
                .child(info_row(cx, "导入 Profile", &profile_name, true))
                .child(
                    div()
                        .mt(px(10.))
                        .text_size(px(10.))
                        .line_height(px(16.))
                        .text_color(colors.on_surface_variant)
                        .child("签名前会校验 Profile 的 bundle、证书指纹、设备 UDID、ACL 和有效期；不匹配会直接报错，不会尝试修改导入的 Profile。"),
                );
            return card.into_any_element();
        }

        card = card.child(
            div()
                .mt(px(12.))
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(8.))
                .child(ui::toggle_chip(
                    cx,
                    "signing-force-profile",
                    "强制重新申请 Profile",
                    self.signing.refresh_profile,
                    cx.listener(|app, _, _, cx| app.signing_toggle_refresh_profile(cx)),
                ))
                .child(div().flex_1())
                .child(ui::action_button(
                    cx,
                    "signing-refresh-profile",
                    "刷新云端列表",
                    Some("refresh"),
                    ActionKind::Text,
                    self.signing.service.is_logged_in() && !self.signing.busy,
                    cx.listener(|app, _, _, cx| app.refresh_signing(cx)),
                )),
        );

        if let Some(cached) = cached {
            card = card.child(
                div()
                    .mt(px(12.))
                    .rounded(px(12.))
                    .bg(colors.surface_container_low)
                    .p(px(12.))
                    .flex()
                    .flex_col()
                    .gap(px(5.))
                    .child(info_row(cx, "本地 Profile", &cached.file, true))
                    .child(info_row(
                        cx,
                        "有效期至",
                        &format_timestamp(cached.expires_at),
                        false,
                    ))
                    .child(info_row(
                        cx,
                        "ACL / 设备",
                        &format!("{} 项 / {} 台", cached.acl.len(), cached.udids.len()),
                        false,
                    )),
            );
        }

        if !profiles.is_empty() {
            card = card.child(
                div()
                    .mt(px(12.))
                    .flex()
                    .flex_col()
                    .gap(px(5.))
                    .child(
                        div()
                            .text_size(px(10.5))
                            .text_color(colors.on_surface_variant)
                            .child("AGC 云端 Profile"),
                    )
                    .children(profiles.into_iter().take(8).map(|profile| {
                        let expired = profile.is_expired();
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                Icon::new(IconName::new(if expired {
                                    "error"
                                } else {
                                    "check_circle"
                                }))
                                .size(px(15.))
                                .color(if expired {
                                    colors.error
                                } else {
                                    colors.tertiary
                                }),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .truncate()
                                    .text_size(px(11.5))
                                    .text_color(colors.on_surface)
                                    .child(profile.provision_name.clone()),
                            )
                            .child(
                                div()
                                    .text_size(px(10.))
                                    .text_color(colors.on_surface_variant)
                                    .child(format!(
                                        "{} · type {} · {} 台设备 · {}",
                                        if profile.cert_name.is_empty() {
                                            "证书".to_string()
                                        } else {
                                            profile.cert_name.clone()
                                        },
                                        profile.provision_type,
                                        profile.device_list.len(),
                                        format_timestamp(profile.expire_time)
                                    )),
                            )
                            .when(profile.acl_permission_audit_state != 0, |el| {
                                el.child(ui::tag(
                                    "ACL 审核中",
                                    colors.tertiary_container,
                                    colors.on_tertiary_container,
                                ))
                            })
                    })),
            );
        }

        card.into_any_element()
    }

    fn signing_acl_card(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = *cx.theme().colors();
        let Some(meta) = self.signing.package.as_ref() else {
            return Card::new()
                .outlined()
                .p(px(20.))
                .child(ui::section_header(
                    cx,
                    "ACL 权限",
                    Some("从 module.json 自动提取，可手动增减"),
                    None,
                ))
                .child(
                    div()
                        .mt(px(12.))
                        .text_size(px(11.))
                        .text_color(colors.on_surface_variant)
                        .child("尚未选择安装包。"),
                )
                .into_any_element();
        };
        let auto: Vec<String> = meta.acl.clone();
        let non_acl = meta.permissions.len().saturating_sub(auto.len());
        let selected = self.signing.acl.clone();
        let items = self.signing.filtered_acl();

        let mut card = Card::new().outlined().p(px(20.)).child(ui::section_header(
            cx,
            "ACL 权限",
            Some("只有 AGC 白名单内的权限会写入 Profile"),
            None,
        ));

        if self.signing.use_imported {
            let auto = meta.acl.clone();
            let mut list = div().mt(px(12.)).flex().flex_wrap().gap(px(6.));
            for permission in &auto {
                list = list.child(ui::tag(
                    permission.clone(),
                    colors.surface_container_high,
                    colors.on_surface_variant,
                ));
            }
            return card
                .child(
                    div()
                        .mt(px(10.))
                        .text_size(px(10.5))
                        .text_color(colors.on_surface_variant)
                        .child(format!(
                            "导入模式：module.json 声明了 {} 项 ACL，导入的 Profile 必须全部覆盖；本工具不会修改 Profile。",
                            auto.len()
                        )),
                )
                .child(list)
                .into_any_element();
        }

        card = card.child(
                div()
                    .mt(px(10.))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .child(self.signing_acl_field.clone()),
                    )
                    .child(ui::tag(
                        format!("已选 {}", selected.len()),
                        colors.secondary_container,
                        colors.on_secondary_container,
                    ))
                    .child(ui::tag(
                        format!("自动 {} · 非 ACL {}", auto.len(), non_acl),
                        colors.surface_container_high,
                        colors.on_surface_variant,
                    )),
            )
            .child(
                div()
                    .mt(px(12.))
                    .id("signing-acl-list")
                    .max_h(px(260.))
                    .overflow_y_scroll()
                    .restrict_scroll_to_axis()
                    .flex()
                    .flex_wrap()
                    .gap(px(6.))
                    .children(items.into_iter().enumerate().map(|(index, permission)| {
                        let checked = selected.iter().any(|item| item == permission);
                        let name = permission.to_string();
                        ui::toggle_chip(
                            cx,
                            ("signing-acl", index),
                            permission,
                            checked,
                            cx.listener(move |app, _, _, cx| {
                                app.signing_toggle_acl(name.clone(), cx)
                            }),
                        )
                    })),
            )
            .child(
                div()
                    .mt(px(10.))
                    .text_size(px(10.5))
                    .line_height(px(17.))
                    .text_color(colors.on_surface_variant)
                    .child("AGC 可能要求受限 ACL 先通过审核；若安装时报 9568289，请到 AGC 查看 ACL 审核状态。"),
            );

        if selected.is_empty() {
            card = card.child(
                div()
                    .mt(px(8.))
                    .text_size(px(10.5))
                    .text_color(colors.on_surface_variant)
                    .child("未选择任何 ACL。普通权限不需要 Profile 授予。"),
            );
        }

        card.into_any_element()
    }

    fn signing_flow_card(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = *cx.theme().colors();
        let busy = self.signing.busy;
        let logged = self.signing.service.is_logged_in();
        let imported = self.signing.use_imported;
        let imported_ready = self.signing.imported_material().is_some();
        let inputs = self.signing.inputs.clone();
        let package = self.signing.package.clone();
        let can_start =
            !busy && !inputs.is_empty() && (if imported { imported_ready } else { logged });

        let mut card = Card::new().outlined().p(px(20.)).child(ui::section_header(
            cx,
            "签名并安装",
            Some(if imported {
                "使用导入的 .p12 / .cer / .p7b 签名，再通过 hdc install 安装"
            } else {
                "自动申请调试证书/Profile，签名后通过 hdc install 安装"
            }),
            None,
        ));

        let input_names: Vec<String> = inputs
            .iter()
            .map(|path| {
                path.file_name()
                    .map(|name| name.to_string_lossy().to_string())
                    .unwrap_or_else(|| path.display().to_string())
            })
            .collect();

        card = card.child(
            div()
                .mt(px(12.))
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(8.))
                .child(ui::action_button(
                    cx,
                    "signing-pick",
                    if inputs.is_empty() {
                        "选择 HAP / HSP"
                    } else {
                        "重新选择"
                    },
                    Some("add_circle"),
                    ActionKind::Tonal,
                    !busy,
                    cx.listener(|app, _, _, cx| app.signing_pick_hap(cx)),
                ))
                .when(!input_names.is_empty(), |el| {
                    el.child(
                        div()
                            .text_size(px(11.))
                            .text_color(colors.on_surface_variant)
                            .child(input_names.join("、")),
                    )
                })
                .when_some(package.as_ref(), |el, meta| {
                    el.child(ui::tag(
                        format!("{} · ACL {}", meta.bundle_name, self.signing.acl.len()),
                        colors.surface_container_high,
                        colors.on_surface_variant,
                    ))
                })
                .child(ui::tag(
                    if imported {
                        "导入材料"
                    } else {
                        "自动调试签名"
                    },
                    if imported {
                        colors.tertiary_container
                    } else {
                        colors.secondary_container
                    },
                    if imported {
                        colors.on_tertiary_container
                    } else {
                        colors.on_secondary_container
                    },
                )),
        );

        card = card.child(
            div()
                .mt(px(12.))
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(8.))
                .child(ui::toggle_chip(
                    cx,
                    "signing-replace",
                    "覆盖安装 -r",
                    self.signing.replace,
                    cx.listener(|app, _, _, cx| app.signing_toggle_replace(false, cx)),
                ))
                .child(ui::toggle_chip(
                    cx,
                    "signing-shared",
                    "共享包 -s",
                    self.signing.shared,
                    cx.listener(|app, _, _, cx| app.signing_toggle_replace(true, cx)),
                ))
                .child(div().w(px(12.)))
                .child(ui::filter_chip(
                    cx,
                    "signing-block-v3",
                    SigningBlock::V3.label(),
                    self.signing.block == SigningBlock::V3,
                    cx.listener(|app, _, _, cx| app.signing_switch_block(SigningBlock::V3, cx)),
                ))
                .child(ui::filter_chip(
                    cx,
                    "signing-block-v2",
                    SigningBlock::V2.label(),
                    self.signing.block == SigningBlock::V2,
                    cx.listener(|app, _, _, cx| app.signing_switch_block(SigningBlock::V2, cx)),
                ))
                .child(div().flex_1())
                .child(ui::action_button(
                    cx,
                    "signing-start",
                    if busy {
                        "正在签名并安装…"
                    } else {
                        "开始签名并安装"
                    },
                    Some("play_arrow"),
                    ActionKind::Filled,
                    can_start,
                    cx.listener(|app, _, _, cx| app.signing_start(cx)),
                )),
        );

        card = card.child(div().mt(px(14.)).flex().flex_col().gap(px(8.)).children(
            Stage::ALL.into_iter().map(|stage| {
                let status = self.signing.stage_status(stage);
                let message = self.signing.stage_message(stage).to_string();
                let (icon, color) = match status {
                    StageStatus::Pending => ("info", colors.on_surface_variant),
                    StageStatus::Running => ("hourglass_empty", colors.primary),
                    StageStatus::Success => ("check_circle", colors.tertiary),
                    StageStatus::Failed => ("error", colors.error),
                };
                div()
                    .flex()
                    .items_start()
                    .gap(px(9.))
                    .child(
                        div()
                            .mt(px(1.))
                            .flex_none()
                            .child(Icon::new(IconName::new(icon)).size(px(16.)).color(color)),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(1.))
                            .child(
                                div()
                                    .text_size(px(11.5))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(colors.on_surface)
                                    .child(stage.label()),
                            )
                            .when(!message.is_empty(), |el| {
                                el.child(
                                    div()
                                        .text_size(px(10.5))
                                        .line_height(px(16.))
                                        .text_color(if status == StageStatus::Failed {
                                            colors.error
                                        } else {
                                            colors.on_surface_variant
                                        })
                                        .child(message),
                                )
                            }),
                    )
                    .when(status == StageStatus::Running, |el| {
                        el.child(div().mt(px(1.)).child(
                            LoadingIndicator::new(("signing-stage", stage.index())).size(px(13.)),
                        ))
                    })
            }),
        ));

        if let Some(outcome) = &self.signing.outcome {
            let mut reports = div().mt(px(12.)).flex().flex_col().gap(px(5.));
            for report in &outcome.reports {
                reports = reports.child(
                    div()
                        .flex()
                        .items_start()
                        .gap(px(8.))
                        .child(
                            Icon::new(IconName::new(if report.success {
                                "check_circle"
                            } else {
                                "error"
                            }))
                            .size(px(15.))
                            .color(if report.success {
                                colors.tertiary
                            } else {
                                colors.error
                            }),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .child(
                                    div()
                                        .text_size(px(11.))
                                        .text_color(colors.on_surface)
                                        .child(report.title.clone()),
                                )
                                .child(
                                    div()
                                        .text_size(px(10.5))
                                        .line_height(px(16.))
                                        .text_color(colors.on_surface_variant)
                                        .child(report.detail.clone()),
                                ),
                        ),
                );
            }
            card = card.child(reports);
        }

        if let Some(path) = &self.signing.last_signed {
            let path_for_reveal = path.clone();
            card = card.child(
                div()
                    .mt(px(12.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .truncate()
                            .text_size(px(10.5))
                            .text_color(colors.on_surface_variant)
                            .child(path.display().to_string()),
                    )
                    .child(ui::action_button(
                        cx,
                        "signing-reveal",
                        "所在文件夹",
                        Some("folder_open"),
                        ActionKind::Text,
                        true,
                        move |_, _, cx| cx.reveal_path(&path_for_reveal),
                    )),
            );
        }

        if let Some(error) = &self.signing.error {
            card = card.child(
                div()
                    .mt(px(12.))
                    .rounded(px(10.))
                    .bg(colors.error_container)
                    .p(px(10.))
                    .text_size(px(10.5))
                    .line_height(px(16.))
                    .text_color(colors.on_error_container)
                    .child(error.clone()),
            );
        }

        card.into_any_element()
    }

    fn signing_logs_card(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = *cx.theme().colors();
        let logs = self.signing.logs.clone();
        let text = logs.join("\n");
        let mut card = Card::new().outlined().p(px(16.)).child(ui::section_header(
            cx,
            "签名日志",
            Some("仅本次运行；不包含 token 或密钥口令"),
            Some(
                ui::action_button(
                    cx,
                    "signing-toggle-logs",
                    if self.signing.show_logs {
                        "收起"
                    } else {
                        "展开"
                    },
                    Some("subject"),
                    ActionKind::Text,
                    true,
                    cx.listener(|app, _, _, cx| app.signing_toggle_logs(cx)),
                )
                .into_any_element(),
            ),
        ));
        if self.signing.show_logs {
            card = card
                .child(
                    div()
                        .mt(px(10.))
                        .flex()
                        .justify_end()
                        .child(ui::action_button(
                            cx,
                            "signing-copy-logs",
                            "复制日志",
                            Some("content_copy"),
                            ActionKind::Text,
                            !logs.is_empty(),
                            move |_, _, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
                            },
                        )),
                )
                .child(
                    div()
                        .mt(px(8.))
                        .id("signing-log-list")
                        .max_h(px(240.))
                        .overflow_y_scroll()
                        .restrict_scroll_to_axis()
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .children(logs.into_iter().map(|line| {
                            div()
                                .font_family(MONO_FONT)
                                .text_size(px(9.5))
                                .line_height(px(14.))
                                .text_color(colors.on_surface_variant)
                                .child(line)
                        })),
                );
        }
        card.into_any_element()
    }
}

fn info_row(cx: &gpui::App, label: &str, value: &str, mono: bool) -> impl IntoElement {
    let colors = *cx.theme().colors();
    div()
        .flex()
        .items_center()
        .gap(px(8.))
        .min_w(px(0.))
        .child(
            div()
                .w(px(82.))
                .flex_none()
                .text_size(px(10.))
                .text_color(colors.on_surface_variant)
                .child(label.to_string()),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .truncate()
                .text_size(px(11.))
                .when(mono, |el| el.font_family(MONO_FONT))
                .text_color(colors.on_surface)
                .child(value.to_string()),
        )
}
