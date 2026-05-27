use settings::Setting;
use warp_editor::editor::NavigationKey;
use warpui::{
    elements::{
        Border, ChildView, Clipped, ConstrainedBox, Container, CornerRadius, CrossAxisAlignment,
        Element, Flex, MainAxisSize, MouseStateHandle, ParentElement, Radius, Shrinkable, Text,
    },
    ui_components::{
        button::ButtonVariant,
        components::{Coords, UiComponent, UiComponentStyles},
    },
    AppContext, Entity, SingletonEntity, TypedActionView, View, ViewContext, ViewHandle,
};

use crate::{
    appearance::Appearance,
    editor::{
        EditorView, Event as EditorEvent, PropagateAndNoOpNavigationKeys, SingleLineEditorOptions,
        TextOptions,
    },
    settings::{normalize_server_url, Sub2ApiSettings},
};

use super::{
    client::{Sub2ApiClient, Sub2ApiLoginResponse},
    session::Sub2ApiSessionStore,
};

const FIELD_WIDTH: f32 = 384.0;
const FIELD_HEIGHT: f32 = 40.0;
const BUTTON_HEIGHT: f32 = 32.0;
const FIELD_RADIUS: f32 = 4.0;
const FIELD_MARGIN_BOTTOM: f32 = 14.0;
const LABEL_MARGIN_BOTTOM: f32 = 6.0;
const FIELD_HORIZONTAL_PADDING: f32 = 10.0;
const FIELD_INNER_HEIGHT: f32 = 18.0;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Sub2ApiLoginEvent {
    LoginSucceeded,
}

#[derive(Debug, Clone)]
pub enum Sub2ApiLoginAction {
    Login,
    Register,
}

pub struct Sub2ApiLoginView {
    email_editor: ViewHandle<EditorView>,
    password_editor: ViewHandle<EditorView>,
    login_button_state: MouseStateHandle,
    register_link_state: MouseStateHandle,
    is_loading: bool,
    error: Option<String>,
}

impl Sub2ApiLoginView {
    pub fn new(ctx: &mut ViewContext<Self>) -> Self {
        let email_editor = build_text_editor(ctx, false, "name@example.com");
        let password_editor = build_text_editor(ctx, true, "");

        ctx.subscribe_to_view(&email_editor, |me, _, event, ctx| {
            me.handle_email_editor_event(event, ctx);
        });
        ctx.subscribe_to_view(&password_editor, |me, _, event, ctx| {
            me.handle_password_editor_event(event, ctx);
        });

        Self {
            email_editor,
            password_editor,
            login_button_state: MouseStateHandle::default(),
            register_link_state: MouseStateHandle::default(),
            is_loading: false,
            error: None,
        }
    }

    fn current_server_url(&self, ctx: &AppContext) -> String {
        normalize_server_url(Sub2ApiSettings::as_ref(ctx).server_url.value())
    }

    fn register_url(&self, ctx: &AppContext) -> String {
        register_url_for_server_url(&self.current_server_url(ctx))
    }

    fn try_login(&mut self, ctx: &mut ViewContext<Self>) {
        if self.is_loading {
            return;
        }

        let server_url = self.current_server_url(ctx);
        let email = self.email_editor.as_ref(ctx).buffer_text(ctx);
        let password = self.password_editor.as_ref(ctx).buffer_text(ctx);

        if email.trim().is_empty() {
            self.error = Some("请输入邮箱".to_owned());
            ctx.focus(&self.email_editor);
            ctx.notify();
            return;
        }

        if password.is_empty() {
            self.error = Some("请输入密码".to_owned());
            ctx.focus(&self.password_editor);
            ctx.notify();
            return;
        }

        Sub2ApiSettings::handle(ctx).update(ctx, |settings, ctx| {
            let _ = settings.server_url.set_value(server_url.clone(), ctx);
        });
        self.is_loading = true;
        self.error = None;
        ctx.notify();
        ctx.spawn(
            async move {
                let client = Sub2ApiClient::new(&server_url);
                let login = client
                    .login(&email, &password)
                    .await
                    .map_err(|e| e.to_string())?;
                validate_login_response(login).map(|login| (server_url, email, login))
            },
            |view, result, ctx| {
                view.is_loading = false;
                match result {
                    Ok((server_url, email, login)) => {
                        Sub2ApiSessionStore::handle(ctx).update(ctx, |store, ctx| {
                            store.set_login_response(&server_url, &email, login, ctx);
                        });
                        ctx.emit(Sub2ApiLoginEvent::LoginSucceeded);
                    }
                    Err(err) => {
                        log::warn!("sub2api login failed: {err}");
                        view.error = Some(format!("登录失败: {err}"));
                        ctx.notify();
                    }
                }
            },
        );
    }

    fn handle_email_editor_event(&mut self, event: &EditorEvent, ctx: &mut ViewContext<Self>) {
        match event {
            EditorEvent::Navigate(NavigationKey::Tab)
            | EditorEvent::Navigate(NavigationKey::ShiftTab) => ctx.focus(&self.password_editor),
            EditorEvent::Enter => self.try_login(ctx),
            EditorEvent::Edited(_) => {
                self.error = None;
                ctx.notify();
            }
            _ => {}
        }
    }

    fn handle_password_editor_event(&mut self, event: &EditorEvent, ctx: &mut ViewContext<Self>) {
        match event {
            EditorEvent::Navigate(NavigationKey::Tab)
            | EditorEvent::Navigate(NavigationKey::ShiftTab) => ctx.focus(&self.email_editor),
            EditorEvent::Enter => self.try_login(ctx),
            EditorEvent::Edited(_) => {
                self.error = None;
                ctx.notify();
            }
            _ => {}
        }
    }

    fn render_editor(
        label: &str,
        editor: &ViewHandle<EditorView>,
        appearance: &Appearance,
    ) -> Box<dyn Element> {
        let theme = appearance.theme();
        let input = Container::new(
            Flex::row()
                .with_main_axis_size(MainAxisSize::Max)
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_child(
                    Shrinkable::new(1., editor_element(editor, FIELD_INNER_HEIGHT)).finish(),
                )
                .finish(),
        )
        .with_background(theme.surface_1())
        .with_border(Border::all(1.).with_border_fill(theme.outline()))
        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(FIELD_RADIUS)))
        .with_padding_left(FIELD_HORIZONTAL_PADDING)
        .with_padding_right(FIELD_HORIZONTAL_PADDING)
        .finish();

        Container::new(
            Flex::column()
                .with_cross_axis_alignment(CrossAxisAlignment::Start)
                .with_child(
                    Container::new(text(label, appearance))
                        .with_margin_bottom(LABEL_MARGIN_BOTTOM)
                        .finish(),
                )
                .with_child(
                    ConstrainedBox::new(input)
                        .with_width(FIELD_WIDTH)
                        .with_height(FIELD_HEIGHT)
                        .finish(),
                )
                .finish(),
        )
        .with_margin_bottom(FIELD_MARGIN_BOTTOM)
        .finish()
    }

    fn render_register_prompt(&self, appearance: &Appearance) -> Box<dyn Element> {
        Flex::row()
            .with_main_axis_size(MainAxisSize::Min)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(text("还没有账户？ 前往 ", appearance))
            .with_child(
                appearance
                    .ui_builder()
                    .button(ButtonVariant::Link, self.register_link_state.clone())
                    .with_style(UiComponentStyles {
                        font_size: Some(appearance.ui_font_size()),
                        padding: Some(Coords::uniform(0.)),
                        ..Default::default()
                    })
                    .with_text_label("注册".to_owned())
                    .build()
                    .on_click(|ctx, _, _| {
                        ctx.dispatch_typed_action(Sub2ApiLoginAction::Register);
                    })
                    .finish(),
            )
            .finish()
    }
}

impl Entity for Sub2ApiLoginView {
    type Event = Sub2ApiLoginEvent;
}

impl TypedActionView for Sub2ApiLoginView {
    type Action = Sub2ApiLoginAction;

    fn handle_action(&mut self, action: &Self::Action, ctx: &mut ViewContext<Self>) {
        match action {
            Sub2ApiLoginAction::Login => {
                self.try_login(ctx);
            }
            Sub2ApiLoginAction::Register => {
                ctx.open_url(&self.register_url(ctx));
            }
        }
    }
}

impl View for Sub2ApiLoginView {
    fn ui_name() -> &'static str {
        "Sub2ApiLoginView"
    }

    fn render(&self, app: &AppContext) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        let mut content = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Start)
            .with_child(Self::render_editor("邮箱", &self.email_editor, appearance))
            .with_child(Self::render_editor(
                "密码",
                &self.password_editor,
                appearance,
            ));

        if let Some(error) = &self.error {
            content.add_child(
                Container::new(error_text(error, appearance))
                    .with_margin_bottom(10.)
                    .finish(),
            );
        }

        content
            .with_child(
                ConstrainedBox::new({
                    let mut button = appearance
                        .ui_builder()
                        .button(ButtonVariant::Basic, self.login_button_state.clone())
                        .with_style(UiComponentStyles {
                            font_size: Some(appearance.ui_font_body()),
                            height: Some(BUTTON_HEIGHT),
                            padding: Some(Coords::default().left(8.).right(8.)),
                            border_radius: Some(CornerRadius::with_all(Radius::Pixels(
                                FIELD_RADIUS,
                            ))),
                            ..Default::default()
                        })
                        .with_centered_text_label(
                            if self.is_loading {
                                "登录中..."
                            } else {
                                "登录"
                            }
                            .to_owned(),
                        );
                    if self.is_loading {
                        button = button.disabled();
                    }
                    button
                        .build()
                        .on_click(|ctx, _, _| {
                            ctx.dispatch_typed_action(Sub2ApiLoginAction::Login);
                        })
                        .finish()
                })
                .with_width(FIELD_WIDTH)
                .with_height(BUTTON_HEIGHT)
                .finish(),
            )
            .with_child(
                Container::new(self.render_register_prompt(appearance))
                    .with_margin_top(12.)
                    .finish(),
            )
            .finish()
    }
}

fn register_url_for_server_url(server_url: &str) -> String {
    format!("{}/register", normalize_server_url(server_url))
}

fn validate_login_response(response: Sub2ApiLoginResponse) -> Result<Sub2ApiLoginResponse, String> {
    if response.token.trim().is_empty() {
        Err("服务器未返回访问令牌".to_owned())
    } else {
        Ok(response)
    }
}

fn text(text: impl Into<String>, appearance: &Appearance) -> Box<dyn Element> {
    Text::new(
        text.into(),
        appearance.ui_font_family(),
        appearance.ui_font_size(),
    )
    .with_color(appearance.theme().active_ui_text_color().into())
    .finish()
}

fn error_text(text: impl Into<String>, appearance: &Appearance) -> Box<dyn Element> {
    Text::new(
        text.into(),
        appearance.ui_font_family(),
        appearance.ui_font_size(),
    )
    .with_color(appearance.theme().ui_error_color().into())
    .finish()
}

fn editor_element(editor: &ViewHandle<EditorView>, height: f32) -> Box<dyn Element> {
    ConstrainedBox::new(Clipped::new(ChildView::new(editor).finish()).finish())
        .with_height(height)
        .finish()
}

fn build_text_editor(
    ctx: &mut ViewContext<Sub2ApiLoginView>,
    is_password: bool,
    placeholder: &'static str,
) -> ViewHandle<EditorView> {
    ctx.add_typed_action_view(move |ctx| {
        let appearance = Appearance::as_ref(ctx);
        let options = SingleLineEditorOptions {
            is_password,
            text: TextOptions::ui_text(Some(appearance.ui_font_size()), appearance),
            propagate_and_no_op_vertical_navigation_keys: PropagateAndNoOpNavigationKeys::Always,
            ..Default::default()
        };
        let mut editor = EditorView::single_line(options, ctx);
        editor.set_placeholder_text(placeholder, ctx);
        editor
    })
}

#[cfg(test)]
mod tests {
    use super::{register_url_for_server_url, validate_login_response};
    use crate::sub2api::client::Sub2ApiLoginResponse;

    #[test]
    fn register_url_uses_normalized_server_url() {
        assert_eq!(
            register_url_for_server_url(" https://www.axaihub.com/// "),
            "https://www.axaihub.com/register"
        );
    }

    #[test]
    fn login_response_without_token_is_error() {
        let result = validate_login_response(Sub2ApiLoginResponse {
            token: String::new(),
            refresh_token: String::new(),
            expires_in: None,
        });

        assert!(result.is_err());
    }
}
