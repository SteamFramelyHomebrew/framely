use super::*;
use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::prelude::FluentBuilder;

pub(super) const BG: u32 = 0x15171b;
pub(super) const PANEL: u32 = 0x1e2126;
pub(super) const EDGE: u32 = 0x353a43;
pub(super) const MUTED: u32 = 0xb0b7c1;
pub(super) const BLUE: u32 = 0x93c5ed;
pub(super) const TEXT: u32 = 0xf0f1f3;
const SELECTED: u32 = 0x253341;
const RADIUS: f32 = 8.;
fn panel() -> Div {
    div()
        .flex()
        .flex_col()
        .gap_4()
        .p_6()
        .bg(rgb(PANEL))
        .border_1()
        .border_color(rgb(EDGE))
        .rounded(px(RADIUS))
}
fn muted(text: impl Into<SharedString>) -> Div {
    div().text_sm().text_color(rgb(MUTED)).child(text.into())
}
fn badge(text: impl Into<SharedString>, active: bool) -> Div {
    div()
        .flex_none()
        .px_2()
        .py_1()
        .rounded(px(4.))
        .text_xs()
        .bg(rgb(if active { SELECTED } else { 0x292d34 }))
        .text_color(rgb(if active { BLUE } else { MUTED }))
        .child(text.into())
}
fn error_banner(error: String) -> Div {
    div()
        .flex()
        .items_start()
        .gap_2()
        .p_3()
        .rounded(px(RADIUS))
        .border_1()
        .border_color(rgb(0x8f5454))
        .bg(rgb(0x38272b))
        .text_sm()
        .text_color(rgb(0xf1b8b8))
        .child(Icon::new(IconName::CircleAlert).size_4().mt_1())
        .child(div().flex_1().min_w_0().child(error))
}
fn heading(title: &'static str, subtitle: &'static str) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_2()
        .mb_3()
        .child(
            div()
                .text_size(px(28.))
                .font_weight(FontWeight::SEMIBOLD)
                .child(title),
        )
        .child(muted(subtitle))
}
fn icon(name: IconName) -> Icon {
    Icon::new(name).size_4().text_color(rgb(MUTED))
}

impl Installer {
    fn device_page(&self, cx: &mut Context<Self>) -> AnyElement {
        let locked = self.busy || self.connection.is_some();
        let mut devices = div()
            .id("device-list")
            .flex()
            .flex_col()
            .gap_2()
            .flex_1()
            .max_h(px(330.))
            .overflow_y_scroll();
        if self.devices.is_empty() {
            devices = devices.child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .items_center()
                    .justify_center()
                    .gap_3()
                    .py_10()
                    .child(
                        Icon::new(IconName::Monitor)
                            .size_8()
                            .text_color(rgb(0xb0b7c1)),
                    )
                    .child(div().text_sm().child(if self.scanning {
                        "正在寻找你的 Frame…"
                    } else {
                        "还没有发现 Frame"
                    }))
                    .child(muted("开始扫描，或直接输入设备 IP")),
            );
        }
        for device in &self.devices {
            let ip = device.ip.clone();
            let selected = self.host.read(cx).value().as_ref() == device.ip;
            devices = devices.child(
                div()
                    .id(SharedString::from(format!("device-{}", device.ip)))
                    .flex()
                    .items_center()
                    .gap_3()
                    .p_4()
                    .rounded(px(RADIUS))
                    .bg(rgb(if selected { SELECTED } else { BG }))
                    .border_1()
                    .border_color(rgb(if selected { 0x7599b5 } else { EDGE }))
                    .cursor_pointer()
                    .child(
                        Icon::new(IconName::Monitor)
                            .size_5()
                            .text_color(rgb(if selected { BLUE } else { MUTED })),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(device.name.clone()),
                            )
                            .child(muted(device.ip.clone())),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(if device.ssh { BLUE } else { MUTED }))
                            .child(if device.ssh {
                                "SSH 可用"
                            } else {
                                "SSH 未开启"
                            }),
                    )
                    .on_click(cx.listener(move |view, _, window, cx| {
                        if view.busy || view.connection.is_some() {
                            return;
                        }
                        view.host
                            .update(cx, |input, cx| input.set_value(ip.clone(), window, cx));
                        view.probe = None;
                        view.error = None;
                        cx.notify();
                    })),
            );
        }
        let discovery = div()
            .flex()
            .flex_col()
            .gap_4()
            .pt_6()
            .pr_6()
            .w(px(340.))
            .flex_none()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .h(px(28.))
                    .child(icon(IconName::Wifi))
                    .child(
                        div()
                            .flex_1()
                            .font_weight(FontWeight::MEDIUM)
                            .child("附近设备"),
                    )
                    .child(badge(format!("{} 台", self.devices.len()), false)),
            )
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(
                        Button::new("scan")
                            .h(px(42.))
                            .label(if self.scanning {
                                "扫描中…"
                            } else {
                                "扫描局域网"
                            })
                            .loading(self.scanning)
                            .disabled(self.busy || self.scanning)
                            .on_click(cx.listener(|view, _, _, cx| view.scan(cx))),
                    )
                    .child(
                        Button::new("stop")
                            .h(px(42.))
                            .ghost()
                            .label("停止")
                            .disabled(!self.scanning)
                            .on_click(cx.listener(|view, _, _, cx| {
                                view.cancelled.store(true, Ordering::Relaxed);
                                view.scan_id += 1;
                                view.scanning = false;
                                view.status = "扫描已停止".into();
                                cx.notify();
                            })),
                    ),
            )
            .child(devices)
            .child(
                Button::new("scan-options")
                    .ghost()
                    .text_sm()
                    .justify_start()
                    .px_0()
                    .label(if self.scan_options {
                        "收起扫描设置"
                    } else {
                        "扫描设置"
                    })
                    .on_click(cx.listener(|view, _, _, cx| {
                        view.scan_options = !view.scan_options;
                        cx.notify();
                    })),
            )
            .when(self.scan_options, |this| {
                this.child(field("扫描范围", &self.cidr, self.busy || self.scanning))
            });
        let mut connection = panel()
            .flex_1()
            .min_w_0()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .h(px(28.))
                    .child(icon(IconName::Link))
                    .child(div().font_weight(FontWeight::MEDIUM).child("设备连接")),
            )
            .child(
                div()
                    .flex()
                    .gap_3()
                    .child(div().flex_1().min_w_0().child(field(
                        "设备 IP / 主机名",
                        &self.host,
                        locked,
                    )))
                    .child(div().w(px(92.)).child(field("端口", &self.port, locked))),
            )
            .child(
                div()
                    .flex()
                    .gap_3()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(field("用户名", &self.user, locked)),
                    )
                    .child(div().flex_1().min_w_0().child(field(
                        "密码",
                        &self.password,
                        self.busy,
                    ))),
            );
        if let Some(probe) = &self.probe {
            connection = connection
                .when_some(self.error.clone(), |this, error| {
                    this.child(error_banner(error)).child(
                        div()
                            .text_size(px(12.))
                            .text_color(rgb(BLUE))
                            .child(probe.fingerprint.clone()),
                    )
                })
                .when(self.error.is_none(), |this| {
                    this.child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .p_3()
                            .bg(rgb(BG))
                            .rounded(px(5.))
                            .child(
                                div()
                                    .flex()
                                    .gap_2()
                                    .items_center()
                                    .child(icon(IconName::ShieldCheck))
                                    .child(div().text_sm().child(if probe.known {
                                        "设备指纹与上次一致"
                                    } else {
                                        "首次连接，请核对设备指纹"
                                    })),
                            )
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(rgb(BLUE))
                                    .child(probe.fingerprint.clone()),
                            ),
                    )
                })
                .child(
                    Button::new("connect")
                        .h(px(42.))
                        .primary()
                        .label(if self.connection_stage.is_some() {
                            "正在连接…"
                        } else {
                            "确认指纹并连接"
                        })
                        .loading(self.connection_stage.is_some())
                        .disabled(locked)
                        .on_click(cx.listener(|view, _, _, cx| view.connect(cx))),
                );
        } else {
            connection = connection
                .when_some(self.error.clone(), |this, error| {
                    this.child(error_banner(error))
                })
                .child(
                    Button::new("probe")
                        .h(px(42.))
                        .primary()
                        .label(if self.busy {
                            "正在连接…"
                        } else {
                            "检查 SSH 连接"
                        })
                        .loading(self.busy)
                        .disabled(locked)
                        .on_click(cx.listener(|view, _, _, cx| view.probe(cx))),
                );
        }
        if let Some(stage) = &self.connection_stage {
            connection = connection.child(
                div()
                    .p_3()
                    .rounded(px(5.))
                    .bg(rgb(0x263541))
                    .text_sm()
                    .text_color(rgb(BLUE))
                    .child(stage.clone()),
            );
        }
        if self.connection.is_some() {
            connection = connection.child(badge("已连接，权限验证通过", true)).child(
                Button::new("disconnect")
                    .ghost()
                    .label("断开连接")
                    .disabled(self.busy)
                    .on_click(cx.listener(|view, _, window, cx| {
                        view.connection = None;
                        view.probe = None;
                        view.error = None;
                        view.status = "已断开连接".into();
                        view.password
                            .update(cx, |input, cx| input.set_value("", window, cx));
                        cx.notify();
                    })),
            );
        }
        div()
            .flex()
            .flex_col()
            .gap_5()
            .child(heading(
                "连接你的 Frame",
                "在同一网络中找到设备，使用开发者模式密码登录。",
            ))
            .child(
                div()
                    .flex()
                    .items_stretch()
                    .gap_5()
                    .child(discovery)
                    .child(connection),
            )
            .into_any_element()
    }
    fn release_page(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut list = div()
            .id("release-list")
            .flex()
            .flex_col()
            .max_h(px(300.))
            .overflow_y_scroll();
        for (index, release) in self
            .releases
            .iter()
            .enumerate()
            .filter(|(_, r)| self.show_testing || !r.prerelease)
        {
            let selected = self.selected == Some(index);
            list = list.child(
                div()
                    .id(SharedString::from(format!("version-{index}")))
                    .flex()
                    .items_center()
                    .gap_3()
                    .p_3()
                    .rounded(px(6.))
                    .border_b_1()
                    .border_color(rgb(EDGE))
                    .bg(rgb(if selected { SELECTED } else { PANEL }))
                    .cursor_pointer()
                    .child(div().w(px(16.)).child(if selected {
                        Icon::new(IconName::Check).size_4().text_color(rgb(BLUE))
                    } else {
                        icon(IconName::Package)
                    }))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .flex_1()
                            .child(
                                div()
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(release.tag_name.clone()),
                            )
                            .child(muted(
                                release
                                    .published_at
                                    .as_deref()
                                    .unwrap_or("")
                                    .split('T')
                                    .next()
                                    .unwrap_or("")
                                    .to_owned(),
                            )),
                    )
                    .child(badge(
                        if release.prerelease {
                            "测试版"
                        } else {
                            "正式版"
                        },
                        !release.prerelease,
                    ))
                    .on_click(cx.listener(move |view, _, _, cx| {
                        if !view.busy {
                            view.selected = Some(index);
                            view.confirmation = None;
                            cx.notify();
                        }
                    })),
            );
        }
        if self.releases.is_empty() {
            list = list.child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap_3()
                    .py_16()
                    .when(self.release_error.is_some(), |this| this.py_8())
                    .child(
                        Icon::new(IconName::Package)
                            .size_8()
                            .text_color(rgb(0xb0b7c1)),
                    )
                    .child(if self.busy {
                        "正在加载发行版本…"
                    } else {
                        "暂无可安装的在线版本"
                    })
                    .when_some(self.release_error.clone(), |this, error| {
                        this.child(error_banner(error).w_full())
                    })
                    .child(muted(if self.busy {
                        "请稍候"
                    } else {
                        "可刷新重试，或选择本地安装包"
                    })),
            );
        }
        let mut source = panel().gap_3().flex_1().min_w_0().child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    Button::new("release-mode")
                        .ghost()
                        .label("在线版本")
                        .bg(rgb(if !self.local { SELECTED } else { PANEL }))
                        .text_color(rgb(if !self.local { BLUE } else { MUTED }))
                        .selected(!self.local)
                        .disabled(self.busy)
                        .on_click(cx.listener(|view, _, _, cx| {
                            view.local = false;
                            view.confirmation = None;
                            cx.notify();
                        })),
                )
                .child(
                    Button::new("local-mode")
                        .ghost()
                        .label("本地安装包")
                        .bg(rgb(if self.local { SELECTED } else { PANEL }))
                        .text_color(rgb(if self.local { BLUE } else { MUTED }))
                        .selected(self.local)
                        .disabled(self.busy)
                        .on_click(cx.listener(|view, _, _, cx| {
                            view.local = true;
                            view.confirmation = None;
                            cx.notify();
                        })),
                )
                .child(div().flex_1())
                .child(
                    Button::new("refresh")
                        .ghost()
                        .label(if self.busy { "加载中…" } else { "刷新" })
                        .loading(self.busy)
                        .disabled(self.busy)
                        .on_click(cx.listener(|view, _, _, cx| view.releases(cx))),
                ),
        );
        if self.local {
            for (kind, id, title, subtitle, path) in [
                (
                    FileKind::Archive,
                    "archive",
                    "Framely 压缩包",
                    "选择 Linux ARM64 的 .tar.gz 文件",
                    self.archive.as_ref(),
                ),
                (
                    FileKind::Checksums,
                    "checksum",
                    "外部校验文件",
                    "选择同一次发布的 SHA256SUMS 或 .sha256",
                    self.checksums.as_ref(),
                ),
            ] {
                source = source.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_4()
                        .p_4()
                        .bg(rgb(BG))
                        .rounded(px(RADIUS))
                        .border_1()
                        .border_color(rgb(EDGE))
                        .child(Icon::new(IconName::File).size_5().text_color(rgb(MUTED)))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap_2()
                                .flex_1()
                                .min_w_0()
                                .child(div().font_weight(FontWeight::MEDIUM).child(title))
                                .child(muted(subtitle))
                                .child(
                                    div().text_xs().text_color(rgb(MUTED)).truncate().child(
                                        path.map(|p| p.display().to_string())
                                            .unwrap_or("尚未选择文件".into()),
                                    ),
                                ),
                        )
                        .child(
                            Button::new(id)
                                .h(px(38.))
                                .label("选择文件")
                                .disabled(self.busy)
                                .on_click(cx.listener(move |view, _, _, cx| view.file(kind, cx))),
                        ),
                );
            }
        } else {
            source = source
                .child(list)
                .when(!self.releases.is_empty(), |this| {
                    this.when_some(self.release_error.clone(), |this, error| {
                        this.child(error_banner(error))
                    })
                })
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .items_center()
                        .child(muted("测试版默认隐藏"))
                        .child(div().flex_1())
                        .child(
                            Button::new("testing")
                                .ghost()
                                .label(if self.show_testing {
                                    "隐藏测试版"
                                } else {
                                    "显示测试版"
                                })
                                .disabled(self.busy)
                                .on_click(cx.listener(|view, _, _, cx| {
                                    view.show_testing = !view.show_testing;
                                    cx.notify();
                                })),
                        ),
                )
                .child(
                    div()
                        .border_t_1()
                        .border_color(rgb(EDGE))
                        .pt_3()
                        .child(field("发行仓库", &self.repo, self.busy)),
                );
        }
        let selected = self.selected.and_then(|i| self.releases.get(i));
        let details = panel()
            .w(px(320.))
            .flex_none()
            .child(
                div()
                    .flex()
                    .gap_2()
                    .items_center()
                    .child(icon(IconName::FileText))
                    .child(div().font_weight(FontWeight::MEDIUM).child("版本说明")),
            )
            .child(
                div()
                    .text_xl()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(if self.local {
                        "本地发行包".to_owned()
                    } else {
                        selected
                            .map(|r| r.tag_name.clone())
                            .unwrap_or("尚未选择".into())
                    }),
            )
            .child(
                div()
                    .id("release-notes")
                    .max_h(px(300.))
                    .overflow_y_scroll()
                    .text_sm()
                    .line_height(px(24.))
                    .text_color(rgb(MUTED))
                    .child(if self.local {
                        "本机校验通过后上传到 Frame，并在设备上再次校验。".to_owned()
                    } else {
                        selected
                            .and_then(|r| r.body.clone())
                            .unwrap_or("选择左侧版本，查看更新内容。".into())
                    }),
            )
            .child(
                div()
                    .pt_3()
                    .border_t_1()
                    .border_color(rgb(EDGE))
                    .child(muted("安装和更新都会保留现有设置与插件数据")),
            );
        div()
            .flex()
            .flex_col()
            .gap_5()
            .child(heading(
                "选择 Framely 版本",
                "使用官方发行版本，或导入已经下载的安装包和校验文件。",
            ))
            .child(
                div()
                    .flex()
                    .items_start()
                    .gap_5()
                    .child(source)
                    .child(details),
            )
            .into_any_element()
    }
    fn maintenance_page(&self, cx: &mut Context<Self>) -> AnyElement {
        let device = self.connection.as_ref().and_then(|c| {
            c.try_lock()
                .ok()
                .map(|c| (c.host.clone(), c.version.clone()))
        });
        let (host, version) = device.unwrap_or_else(|| {
            if self.preview {
                ("192.168.1.42".into(), "0.4.0".into())
            } else {
                ("尚未连接设备".into(), "未知".into())
            }
        });
        let target = if !maintenance::requires_package(&self.chosen_action) {
            if self.chosen_action == "rollback" {
                self.device_state
                    .as_ref()
                    .and_then(|state| state.previous_version.clone())
                    .unwrap_or("没有可回滚的版本".into())
            } else {
                "当前安装".into()
            }
        } else if self.local {
            self.archive
                .as_ref()
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or("尚未选择安装包".into())
        } else {
            self.selected
                .and_then(|i| self.releases.get(i))
                .map(|r| r.tag_name.clone())
                .unwrap_or("尚未选择版本".into())
        };
        let mut operations = panel()
            .gap_2()
            .flex_1()
            .min_w_0()
            .child(div().font_weight(FontWeight::MEDIUM).child("选择操作"));
        for (action, name, description, symbol) in [
            (
                "install",
                "安装",
                "在 Frame 上安装所选版本",
                IconName::Download,
            ),
            (
                "update",
                "更新",
                "切换版本，保留设置和插件数据",
                IconName::RefreshCw,
            ),
            (
                "repair",
                "修复安装",
                "恢复账号、服务和开机启动",
                IconName::Wrench,
            ),
            ("rollback", "回滚", "恢复上一发行版本", IconName::Undo),
            (
                "uninstall",
                "卸载",
                "先卸载全部插件，再移除 Framely",
                IconName::Trash,
            ),
        ] {
            if !self.action_available(action) {
                continue;
            }
            let selected = self.chosen_action == action;
            operations =
                operations.child(
                    div()
                        .id(action)
                        .flex()
                        .items_center()
                        .gap_3()
                        .px_4()
                        .py_2()
                        .rounded(px(6.))
                        .border_1()
                        .border_color(rgb(if selected { 0x7599b5 } else { PANEL }))
                        .bg(rgb(if selected { SELECTED } else { PANEL }))
                        .cursor_pointer()
                        .child(Icon::new(symbol).size_4().text_color(rgb(
                            if action == "uninstall" {
                                0xefa9a8
                            } else if selected {
                                BLUE
                            } else {
                                MUTED
                            },
                        )))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap_1()
                                .flex_1()
                                .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(name))
                                .child(muted(description)),
                        )
                        .when(selected, |this| {
                            this.child(Icon::new(IconName::Check).size_4().text_color(rgb(BLUE)))
                        })
                        .on_click(cx.listener(move |view, _, _, cx| {
                            if !view.busy {
                                view.chosen_action = action.into();
                                view.confirmation = None;
                                cx.notify();
                            }
                        })),
                );
        }
        if self.connection.is_none() && !self.preview {
            operations = operations.child(muted("请先连接设备，检查当前安装状态。"));
        } else if self.same_update() {
            operations = operations.child(muted("所选版本已安装，请选择“修复安装”。"));
        }
        let summary = panel()
            .w(px(320.))
            .flex_none()
            .child(
                div()
                    .flex()
                    .gap_2()
                    .items_center()
                    .child(icon(IconName::Monitor))
                    .child(div().font_weight(FontWeight::MEDIUM).child("操作概览")),
            )
            .child(muted("目标设备"))
            .child(host)
            .child(muted("当前版本"))
            .child(version)
            .child(muted("目标发行"))
            .child(target)
            .child(
                div()
                    .border_t_1()
                    .border_color(rgb(EDGE))
                    .pt_3()
                    .child(muted("安装包会在电脑和设备上分别校验")),
            );
        let mut body = div()
            .flex()
            .flex_col()
            .gap_5()
            .child(heading(
                "准备安装与维护",
                "确认设备、版本和操作。执行过程会显示进度与日志。",
            ))
            .child(
                div()
                    .flex()
                    .items_start()
                    .gap_5()
                    .child(operations)
                    .child(summary),
            );
        if let Some((action, result)) = &self.operation_result {
            let success = result.is_ok();
            let description = match result {
                Ok(()) if action == "uninstall" => {
                    "Framely 已卸载。重新连接设备后可再次安装。".to_owned()
                }
                Ok(()) => "操作已成功完成。请重新连接设备，读取最新版本和可用操作。".to_owned(),
                Err(error) => error.clone(),
            };
            let feedback = panel()
                .border_color(rgb(if success { 0x56866b } else { 0x8f5454 }))
                .bg(rgb(if success { 0x21372b } else { 0x38272b }))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            Icon::new(if success {
                                IconName::Check
                            } else {
                                IconName::CircleAlert
                            })
                            .size_5(),
                        )
                        .child(div().font_weight(FontWeight::SEMIBOLD).child(format!(
                            "{}{}",
                            action_label(action),
                            if success { "完成" } else { "失败" }
                        ))),
                )
                .child(div().text_sm().child(description))
                .child(
                    Button::new("reconnect-result")
                        .primary()
                        .label("重新连接设备")
                        .on_click(cx.listener(|view, _, _, cx| view.navigate(0, cx))),
                );
            body = div().flex().flex_col().gap_5().child(feedback);
        }
        if let Some(action) = &self.confirmation {
            let message = if action == "uninstall" {
                "将停用并卸载所有插件，然后移除 Framely。插件卸载失败时保留本体；已保存的数据保留。"
                    .into()
            } else {
                format!(
                    "确认{}？设备上的 Framely 服务可能会暂时中断。",
                    action_label(action)
                )
            };
            body = body.child(
                panel()
                    .border_color(rgb(0x7599b5))
                    .child(div().text_sm().child(message))
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(
                                Button::new("confirm")
                                    .primary()
                                    .label("确认执行")
                                    .disabled(self.busy)
                                    .on_click(cx.listener(|view, _, _, cx| view.operate(cx))),
                            )
                            .child(
                                Button::new("cancel")
                                    .ghost()
                                    .label("取消")
                                    .disabled(self.busy)
                                    .on_click(cx.listener(|view, _, _, cx| {
                                        view.confirmation = None;
                                        cx.notify();
                                    })),
                            ),
                    ),
            );
        }
        if !self.logs.is_empty() {
            body = body.child(
                panel()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .child(div().flex_1().text_sm().child("操作日志"))
                            .child(Button::new("copy-logs").ghost().label("复制").on_click(
                                cx.listener(|view, _, _, cx| {
                                    cx.write_to_clipboard(ClipboardItem::new_string(
                                        view.logs.join("\n"),
                                    ))
                                }),
                            )),
                    )
                    .child(
                        div()
                            .id("logs")
                            .max_h(px(180.))
                            .overflow_y_scroll()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .children(self.logs.iter().map(|line| muted(line.clone()))),
                    ),
            );
        }
        body.into_any_element()
    }
}
fn action_label(action: &str) -> &'static str {
    match action {
        "update" => "更新",
        "repair" => "修复安装",
        "rollback" => "回滚",
        "uninstall" => "卸载",
        _ => "安装",
    }
}
impl Render for Installer {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let body = match self.page {
            0 => self.device_page(cx),
            1 => self.release_page(cx),
            _ => self.maintenance_page(cx),
        };
        let navigation = div()
            .flex()
            .items_center()
            .gap_2()
            .px_8()
            .h(px(60.))
            .flex_none()
            .border_b_1()
            .border_color(rgb(EDGE))
            .children(
                [
                    ("连接设备", IconName::Monitor),
                    ("选择版本", IconName::Package),
                    ("安装与维护", IconName::Wrench),
                ]
                .into_iter()
                .enumerate()
                .map(|(index, (name, symbol))| {
                    let active = self.page == index;
                    Button::new(SharedString::from(format!("step-{index}")))
                        .ghost()
                        .h(px(38.))
                        .px_4()
                        .icon(symbol)
                        .label(name)
                        .text_color(rgb(if active { BLUE } else { MUTED }))
                        .bg(rgb(if active { SELECTED } else { BG }))
                        .on_click(cx.listener(move |view, _, _, cx| view.navigate(index, cx)))
                }),
            )
            .child(div().flex_1())
            .when(self.connection.is_some(), |this| {
                this.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            Icon::new(IconName::ShieldCheck)
                                .size_4()
                                .text_color(rgb(BLUE)),
                        )
                        .child(muted(self.connected_host.clone())),
                )
            });
        let mut footer = div()
            .rounded_b(px(8.))
            .flex()
            .items_center()
            .gap_4()
            .px_8()
            .py_4()
            .border_t_1()
            .border_color(rgb(EDGE))
            .bg(rgb(BG))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .flex_1()
                    .min_w_0()
                    .child(div().text_xs().text_color(rgb(MUTED)).truncate().child(
                        if self.error.is_some()
                            || (self.page == 1 && !self.local && self.release_error.is_some())
                        {
                            "操作未完成，请查看错误详情。".to_owned()
                        } else {
                            self.status.clone()
                        },
                    )),
            );
        if self.page < 2 {
            let label = if self.page == 0 && self.connection.is_none() {
                "先连接设备"
            } else if self.page == 0 {
                if self
                    .device_state
                    .as_ref()
                    .is_some_and(|state| state.present)
                {
                    "维护操作"
                } else {
                    "选择版本"
                }
            } else if !self.selection_ready() && self.local {
                "请选择两个文件"
            } else if !self.selection_ready() {
                "请选择版本"
            } else {
                "选择操作"
            };
            footer = footer.child(
                Button::new("next-step")
                    .h(px(42.))
                    .primary()
                    .label(label)
                    .disabled(
                        self.busy
                            || (self.page == 0 && self.connection.is_none())
                            || (self.page == 1 && !self.selection_ready()),
                    )
                    .on_click(cx.listener(|view, _, _, cx| {
                        let next = if view.page == 0
                            && view
                                .device_state
                                .as_ref()
                                .is_some_and(|state| state.present)
                        {
                            2
                        } else {
                            view.page + 1
                        };
                        view.navigate(next, cx);
                    })),
            );
        } else {
            if self.connection.is_some() {
                let url = format!(
                    "http://{}:15915",
                    self.connected_host
                        .rsplit_once(':')
                        .map(|(h, _)| h)
                        .unwrap_or(&self.connected_host)
                );
                footer = footer.child(
                    Button::new("manager")
                        .ghost()
                        .label("管理面板")
                        .on_click(move |_, _, cx| cx.open_url(&url)),
                );
            }
            if self.operation_result.is_none() {
                footer = footer.child(
                    Button::new("execute")
                        .h(px(42.))
                        .primary()
                        .label(if self.busy {
                            "正在执行…"
                        } else {
                            if self.connection.is_none() && !self.preview {
                                "先连接设备"
                            } else if self.same_update() {
                                "该版本已安装"
                            } else if maintenance::requires_package(&self.chosen_action)
                                && !self.selection_ready()
                            {
                                "选择版本"
                            } else {
                                action_label(&self.chosen_action)
                            }
                        })
                        .disabled(
                            self.busy
                                || self.connection.is_none()
                                || !self.action_available(&self.chosen_action)
                                || self.same_update(),
                        )
                        .on_click(cx.listener(|view, _, _, cx| {
                            view.request_operation(cx);
                        })),
                );
            }
        }
        div()
            .size_full()
            .rounded(px(8.))
            .overflow_hidden()
            .flex()
            .flex_col()
            .bg(rgb(BG))
            .text_color(rgb(TEXT))
            .child(
                TitleBar::new()
                    .rounded_t(px(8.))
                    .h(px(54.))
                    .bg(rgb(BG))
                    .border_color(rgb(EDGE))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .flex_1()
                            .h_full()
                            .px_5()
                            .child(
                                div()
                                    .w(px(26.))
                                    .h(px(26.))
                                    .border_2()
                                    .border_color(rgb(BLUE))
                                    .rounded(px(5.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(
                                        div().w(px(10.)).h(px(10.)).bg(rgb(BLUE)).rounded(px(2.)),
                                    ),
                            )
                            .child(
                                div()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_lg()
                                    .child("Framely"),
                            )
                            .child(div().text_sm().text_color(rgb(MUTED)).child("安装器"))
                            .child(div().flex_1())
                            .when(self.preview, |this| {
                                this.child(badge("界面预览 · 模拟数据", false))
                            }),
                    ),
            )
            .child(navigation)
            .child(
                div()
                    .id("page-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(
                        div()
                            .w_full()
                            .max_w(px(1120.))
                            .mx_auto()
                            .px_8()
                            .py_7()
                            .when(self.page != 0, |this| {
                                this.when_some(self.error.clone(), |this, error| {
                                    this.child(div().mb_5().child(error_banner(error)))
                                })
                            })
                            .child(body),
                    ),
            )
            .child(footer)
    }
}
