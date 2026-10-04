use super::*;
use gpui_kit::component::dialog::DialogButtonProps;
use gpui_kit::prelude::FluentBuilder;

#[derive(Clone, PartialEq, Eq)]
pub(super) enum Prompt {
    Operation(String),
    Notice(String, String),
    Outcome(String, Result<(), String>),
}

impl Installer {
    pub(super) fn queue_prompt(&mut self, prompt: Prompt) {
        if !self.prompts.contains(&prompt) {
            self.prompts.push_back(prompt);
        }
    }

    pub(super) fn notice(&mut self, title: impl Into<String>, message: impl Into<String>) {
        self.queue_prompt(Prompt::Notice(title.into(), message.into()));
    }

    pub(super) fn present_prompt(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if window.has_active_dialog(cx) {
            return;
        }
        let Some(prompt) = self.prompts.pop_front() else {
            return;
        };
        if let Prompt::Operation(action) = &prompt {
            if self.confirmation.as_ref() != Some(action) {
                return;
            }
        }
        let device = self.connected_host.clone();
        let version = if self.local {
            self.archive.as_ref().map(|p| p.display().to_string())
        } else {
            self.selected
                .and_then(|i| self.releases.get(i))
                .map(|r| r.tag_name.clone())
        };
        let view = cx.entity().downgrade();
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let close_view = view.clone();
            let action_view = view.clone();
            let (title, message, ok, cancel) = match &prompt {
                Prompt::Operation(action) => (
                    format!("确认{}？", ui::action_label(action)),
                    if action == "uninstall" {
                        "将停用并卸载所有插件，然后移除 Framely。插件卸载失败时保留本体；已保存的数据保留。".into()
                    } else {
                        "设备上的 Framely 服务可能会暂时中断，插件和已保存的数据会保留。".into()
                    },
                    format!("确认{}", ui::action_label(action)),
                    true,
                ),
                Prompt::Notice(title, message) => (title.clone(), message.clone(), "知道了".into(), false),
                Prompt::Outcome(action, result) => (
                    format!("{}{}", ui::action_label(action), if result.is_ok() { "完成" } else { "失败" }),
                    match result {
                        Ok(()) if action == "uninstall" => "Framely 已卸载。已保存的数据保留；重新连接后可再次安装。".into(),
                        Ok(()) => "操作已完成。请重新连接设备，读取最新版本和可用操作。".into(),
                        Err(error) => format!("{error}\n\n可关闭弹窗查看日志，重新连接设备后再试。"),
                    },
                    "返回设备连接".into(),
                    false,
                ),
            };
            let close_prompt = prompt.clone();
            let action_prompt = prompt.clone();
            dialog
                .width(px(540.))
                .close_button(true)
                .title(title)
                .child(
                    div().id("installer-prompt-content").flex().flex_col().gap_3()
                        .max_h(px(360.)).overflow_y_scroll().text_sm()
                        .when(matches!(prompt, Prompt::Operation(_)), |body| {
                            body.child(format!("设备：{device}"))
                                .when(matches!(&prompt, Prompt::Operation(action) if maintenance::requires_package(action)), |body| {
                                    body.child(format!("版本 / 安装包：{}", version.as_deref().unwrap_or("未选择")))
                                })
                        })
                        .child(message),
                )
                .button_props(DialogButtonProps::default().ok_text(ok).cancel_text("取消").show_cancel(cancel))
                .on_ok(move |_, _, cx| {
                    let _ = action_view.update(cx, |view, cx| match &action_prompt {
                        Prompt::Operation(action) if view.confirmation.as_ref() == Some(action) => view.operate(cx),
                        Prompt::Outcome(_, _) => view.navigate(0, cx),
                        _ => {}
                    });
                    true
                })
                .on_close(move |_, _, cx| {
                    let _ = close_view.update(cx, |view, cx| {
                        if let Prompt::Operation(action) = &close_prompt {
                            if view.confirmation.as_ref() == Some(action) {
                                view.confirmation = None;
                            }
                        }
                        cx.notify();
                    });
                })
        });
    }
}
